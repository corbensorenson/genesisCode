#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

source "$ROOT_DIR/scripts/lib/cargo_target_dir.sh"
genesis_configure_cargo_target_dir \
  "$ROOT_DIR" \
  "selfhost-readiness" \
  ".genesis/build/cargo" \
  "GENESIS_SELFHOST_READINESS_CARGO_TARGET_DIR"

REPORT_PATH="${GENESIS_SELFHOST_READINESS_REPORT:-.genesis/perf/selfhost_readiness_report.json}"
HISTORY_PATH="${GENESIS_SELFHOST_READINESS_HISTORY:-.genesis/perf/selfhost_readiness_history.jsonl}"
BUDGET_MS="${GENESIS_SELFHOST_READINESS_BUDGET_MS:-600000}"
P95_MIN_SAMPLES="${GENESIS_SELFHOST_READINESS_P95_MIN_SAMPLES:-1}"
STRICT_MODE="${GENESIS_SELFHOST_READINESS_STRICT:-0}"

if [[ ! "$BUDGET_MS" =~ ^[0-9]+$ || "$BUDGET_MS" -le 0 ]]; then
  echo "selfhost-readiness: GENESIS_SELFHOST_READINESS_BUDGET_MS must be a positive integer" >&2
  exit 2
fi
if [[ ! "$P95_MIN_SAMPLES" =~ ^[0-9]+$ || "$P95_MIN_SAMPLES" -le 0 ]]; then
  echo "selfhost-readiness: GENESIS_SELFHOST_READINESS_P95_MIN_SAMPLES must be a positive integer" >&2
  exit 2
fi
if [[ "$STRICT_MODE" != "0" && "$STRICT_MODE" != "1" ]]; then
  echo "selfhost-readiness: GENESIS_SELFHOST_READINESS_STRICT must be 0 or 1" >&2
  exit 2
fi

DEFAULT_GENESIS_BIN="$CARGO_TARGET_DIR/debug/genesis"
GENESIS_BIN="${GENESIS_BIN:-$DEFAULT_GENESIS_BIN}"
if [[ ! -x "$GENESIS_BIN" ]]; then
  cargo build -p gc_cli >/dev/null
fi
if [[ ! -x "$GENESIS_BIN" ]]; then
  echo "selfhost-readiness: expected genesis binary not found at $GENESIS_BIN" >&2
  exit 1
fi

TMP_DIR="$(mktemp -d)"
cleanup() {
  rm -rf "$TMP_DIR"
}
trap cleanup EXIT

DASHBOARD_JSON="$TMP_DIR/selfhost_dashboard.json"
# `selfhost-dashboard` always emits markdown; direct it into the temp dir so this
# readiness check never mutates the committed dashboard as a side effect.
DASHBOARD_MD="$TMP_DIR/SELFHOST_CUTOVER.md"
"$GENESIS_BIN" \
  --selfhost-artifact "selfhost/toolchain.gc" \
  --json \
  selfhost-dashboard \
  --markdown "$DASHBOARD_MD" \
  >"$DASHBOARD_JSON"

START_NS="$(python3 - <<'PY'
import time
print(time.time_ns())
PY
)"

python3 - "$ROOT_DIR" "$REPORT_PATH" "$HISTORY_PATH" "$BUDGET_MS" "$P95_MIN_SAMPLES" "$GENESIS_BIN" "$DASHBOARD_JSON" "$START_NS" "$STRICT_MODE" <<'PY'
import datetime as dt
import json
import math
import os
import pathlib
import re
import subprocess
import sys
import tempfile
import time
from typing import Any, Optional

root = pathlib.Path(sys.argv[1])
report_path = pathlib.Path(sys.argv[2])
history_path = pathlib.Path(sys.argv[3])
budget_ms = int(sys.argv[4])
p95_min_samples = int(sys.argv[5])
genesis_bin = pathlib.Path(sys.argv[6])
dashboard_path = pathlib.Path(sys.argv[7])
start_ns = int(sys.argv[8])
strict_mode = sys.argv[9] == "1"

if p95_min_samples < 1:
    raise SystemExit("selfhost-readiness: p95_min_samples must be >= 1")

def write_text_atomic(path: pathlib.Path, text: str) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    fd, tmp_name = tempfile.mkstemp(
        prefix=f".{path.name}.tmp-",
        dir=str(path.parent),
    )
    tmp_path = pathlib.Path(tmp_name)
    try:
        with os.fdopen(fd, "w", encoding="utf-8") as handle:
            handle.write(text)
            handle.flush()
            os.fsync(handle.fileno())
        os.replace(tmp_path, path)
    finally:
        if tmp_path.exists():
            tmp_path.unlink()

dashboard_doc = json.loads(dashboard_path.read_text(encoding="utf-8"))
if dashboard_doc.get("kind") != "genesis/selfhost-dashboard-v0.2":
    raise SystemExit(
        "selfhost-readiness: unexpected selfhost-dashboard response kind: "
        + repr(dashboard_doc.get("kind"))
    )
summary = dashboard_doc.get("data", {}).get("summary", {})

def parse_percent(raw: Any, field: str) -> float:
    if not isinstance(raw, str) or not raw.endswith("%"):
        raise SystemExit(f"selfhost-readiness: invalid percent field `{field}`: {raw!r}")
    try:
        return float(raw[:-1])
    except ValueError as exc:
        raise SystemExit(f"selfhost-readiness: invalid numeric percent for `{field}`: {raw!r}") from exc

def dim_runtime_routing() -> dict[str, Any]:
    routed = parse_percent(summary.get("selfhost_routed_percent"), "selfhost_routed_percent")
    default = parse_percent(summary.get("selfhost_default_percent"), "selfhost_default_percent")
    fast_ok = bool(summary.get("fast_path_default_ok", False))
    ok = routed >= 100.0 and default >= 100.0 and fast_ok
    score = int(min(100.0, max(0.0, round((routed + default) / 2.0))))
    if not fast_ok:
        score = max(0, score - 20)
    if ok:
        score = 100
    return {
        "ok": ok,
        "score": score,
        "max_score": 100,
        "selfhost_routed_percent": routed,
        "selfhost_default_percent": default,
        "fast_path_default_ok": fast_ok,
        "selfhost_routed_commands": summary.get("selfhost_routed_commands"),
        "total_commands": summary.get("total_commands"),
    }

def iter_rust_source_files() -> list[pathlib.Path]:
    files: list[pathlib.Path] = []
    crates = root / "crates"
    for dirpath, _, filenames in os.walk(crates):
        for name in filenames:
            if not name.endswith(".rs"):
                continue
            files.append(pathlib.Path(dirpath) / name)
    files.sort()
    return files

def dim_parity_isolation() -> dict[str, Any]:
    production_refs = 0
    production_files: list[str] = []
    parity_refs = 0
    parity_files: list[str] = []
    token = "CoreformFrontend::Rust"
    for path in iter_rust_source_files():
        rel = path.relative_to(root).as_posix()
        text = path.read_text(encoding="utf-8")
        matches = text.count(token)
        if matches <= 0:
            continue
        is_test = "/tests/" in rel
        is_parity = rel.endswith("_parity.rs") or rel.startswith("crates/gc_cli_driver_parity/")
        if is_test or is_parity:
            parity_refs += matches
            parity_files.append(rel)
        else:
            production_refs += matches
            production_files.append(rel)
    ok = production_refs == 0
    score = 100 if ok else 0
    return {
        "ok": ok,
        "score": score,
        "max_score": 100,
        "production_rust_frontend_ref_count": production_refs,
        "parity_rust_frontend_ref_count": parity_refs,
        "production_ref_files": production_files[:20],
        "parity_ref_files": parity_files[:20],
    }

def run_help(*args: str) -> str:
    proc = subprocess.run(
        [str(genesis_bin), *args, "--help"],
        cwd=root,
        check=True,
        capture_output=True,
        text=True,
    )
    return proc.stdout

def help_assertions(help_text: str, surface: str) -> list[str]:
    errors: list[str] = []
    if "Accepted value: selfhost." not in help_text:
        errors.append(f"{surface}:missing-selfhost")
    if "Accepted value: artifact-only." not in help_text:
        errors.append(f"{surface}:missing-artifact-only")
    if "Accepted values: selfhost, rust." in help_text:
        errors.append(f"{surface}:unexpected-rust-frontend")
    return errors

def dim_bootstrap_mode_strictness() -> dict[str, Any]:
    checks: list[str] = []
    errors: list[str] = []
    root_help = run_help()
    checks.append("genesis --help")
    errors.extend(help_assertions(root_help, "genesis-help"))
    fmt_help = run_help("fmt")
    checks.append("genesis fmt --help")
    errors.extend(help_assertions(fmt_help, "genesis-fmt-help"))

    wasi_bin = genesis_bin.parent / "genesis_wasi"
    if wasi_bin.exists() and wasi_bin.is_file():
        for subargs, surface in (([], "genesis-wasi-help"), (["fmt"], "genesis-wasi-fmt-help")):
            proc = subprocess.run(
                [str(wasi_bin), *subargs, "--help"],
                cwd=root,
                check=True,
                capture_output=True,
                text=True,
            )
            checks.append(f"{wasi_bin.name} {' '.join(subargs)} --help".strip())
            errors.extend(help_assertions(proc.stdout, surface))

    ok = not errors
    return {
        "ok": ok,
        "score": 100 if ok else 0,
        "max_score": 100,
        "checks": checks,
        "errors": errors,
    }

def dim_deprecated_bootstrap_refs() -> dict[str, Any]:
    pattern = re.compile(r"old_bootstrap/rust_semantics|legacy_program_builders")
    count = 0
    files: list[str] = []
    for path in iter_rust_source_files():
        rel = path.relative_to(root).as_posix()
        text = path.read_text(encoding="utf-8")
        if not pattern.search(text):
            continue
        count += len(pattern.findall(text))
        files.append(rel)
    ok = count == 0
    score = 100 if ok else max(0, 100 - min(100, count * 20))
    return {
        "ok": ok,
        "score": score,
        "max_score": 100,
        "deprecated_reference_count": count,
        "deprecated_reference_files": files[:20],
    }

def tail_text(raw: str, max_chars: int = 320) -> str:
    text = (raw or "").strip()
    if len(text) <= max_chars:
        return text
    return text[-max_chars:]

def read_critical_report(
    report_rel: str,
    expected_kind: str,
    label: str,
) -> tuple[bool, dict[str, Any], Optional[str]]:
    report_path = root / report_rel
    if not report_path.is_file():
        return False, {"path": report_rel, "ok": False, "missing": True}, f"{label}:missing"
    try:
        doc = json.loads(report_path.read_text(encoding="utf-8"))
    except json.JSONDecodeError:
        return (
            False,
            {"path": report_rel, "ok": False, "decode_error": True},
            f"{label}:json-decode",
        )
    if doc.get("kind") != expected_kind:
        return (
            False,
            {
                "path": report_rel,
                "ok": False,
                "kind": doc.get("kind"),
                "expected_kind": expected_kind,
            },
            f"{label}:kind-mismatch",
        )
    report_ok = bool(doc.get("ok", False))
    detail = {
        "path": report_rel,
        "ok": report_ok,
        "kind": doc.get("kind"),
    }
    if not report_ok:
        detail["fail_reasons"] = doc.get("fail_reasons")
        return False, detail, f"{label}:report-not-ok"
    return True, detail, None

def dim_critical_gate_truth() -> dict[str, Any]:
    checks: list[str] = []
    errors: list[str] = []
    reports: dict[str, Any] = {}
    critical_specs = [
        (
            "agent_capability_gauntlet",
            ".genesis/perf/agent_capability_gauntlet_report.json",
            "genesis/agent-capability-gauntlet-v0.1",
            "agent-capability-gauntlet",
        ),
        (
            "production_cli_help_surface",
            ".genesis/perf/production_cli_help_surface_report.json",
            "genesis/production-cli-help-surface-v0.1",
            "production-cli-help-surface",
        ),
        (
            "gpu_gfx_headroom_conformance",
            ".genesis/perf/gpu_gfx_headroom_conformance_report.json",
            "genesis/gpu-gfx-headroom-conformance-v0.1",
            "gpu-gfx-headroom-conformance",
        ),
    ]
    for key, report_rel, expected_kind, label in critical_specs:
        checks.append(label)
        ok, detail, error = read_critical_report(report_rel, expected_kind, label)
        reports[key] = detail
        if not ok and error is not None:
            errors.append(error)

    runtime_pipeline_cmd = [
        "bash",
        str(root / "scripts/check_gcpm_target_runtime_pipelines.sh"),
    ]
    checks.append("gcpm-target-runtime-pipelines")
    runtime_proc = subprocess.run(
        runtime_pipeline_cmd,
        cwd=root,
        capture_output=True,
        text=True,
    )
    runtime_ok = runtime_proc.returncode == 0
    reports["gcpm_target_runtime_pipelines"] = {
        "ok": runtime_ok,
        "exit_code": runtime_proc.returncode,
        "stdout_tail": tail_text(runtime_proc.stdout),
        "stderr_tail": tail_text(runtime_proc.stderr),
    }
    if not runtime_ok:
        errors.append("gcpm-target-runtime-pipelines:check-failed")

    ok = not errors
    return {
        "ok": ok,
        "score": 100 if ok else 0,
        "max_score": 100,
        "checks": checks,
        "errors": errors,
        "reports": reports,
    }

open_upgrade_ids = sorted(
    set(
        re.findall(
            r"^- \[ \] (P\d+\.\d+)\b",
            (root / "upgrade_plan.md").read_text(encoding="utf-8"),
            flags=re.MULTILINE,
        )
    )
)

dimensions = {
    "runtime_routing_coverage": dim_runtime_routing(),
    "parity_only_surface_isolation": dim_parity_isolation(),
    "bootstrap_mode_strictness": dim_bootstrap_mode_strictness(),
    "deprecated_bootstrap_reference_count": dim_deprecated_bootstrap_refs(),
    "critical_gate_truth": dim_critical_gate_truth(),
}
dimension_ok = all(bool(row.get("ok")) for row in dimensions.values())
score = int(round(sum(int(row["score"]) for row in dimensions.values()) / len(dimensions)))

elapsed_ms = int((time.time_ns() - start_ns) / 1_000_000)

history_rows: list[dict[str, Any]] = []
if history_path.is_file():
    for raw in history_path.read_text(encoding="utf-8").splitlines():
        line = raw.strip()
        if not line:
            continue
        try:
            row = json.loads(line)
        except json.JSONDecodeError:
            continue
        if (
            isinstance(row, dict)
            and row.get("kind") == "genesis/selfhost-readiness-v0.1"
            and isinstance(row.get("elapsed_ms"), int)
            and isinstance(row.get("budget_ms"), int)
            and int(row.get("budget_ms")) == budget_ms
        ):
            history_rows.append(row)

history_samples = len(history_rows) + 1
elapsed_samples = sorted(int(row["elapsed_ms"]) for row in history_rows) + [elapsed_ms]
p95_idx = max(0, math.ceil(0.95 * len(elapsed_samples)) - 1)
history_p95_ms = sorted(elapsed_samples)[p95_idx]
history_p95_enforced = history_samples >= p95_min_samples
history_p95_ok = (not history_p95_enforced) or (history_p95_ms <= budget_ms)
elapsed_budget_ok = elapsed_ms <= budget_ms
closure_ok = len(open_upgrade_ids) == 0

fail_reasons: list[str] = []
if not dimension_ok:
    fail_reasons.append("dimension-failure")
if not closure_ok:
    fail_reasons.append("open-upgrade-plan-ids")
if not elapsed_budget_ok:
    fail_reasons.append("elapsed-budget")
if not history_p95_ok:
    fail_reasons.append("history-p95-budget")

ok = dimension_ok and closure_ok and elapsed_budget_ok and history_p95_ok
now_utc = dt.datetime.now(dt.timezone.utc).replace(microsecond=0).isoformat()

previous_elapsed_ms = None
if report_path.is_file():
    try:
        prev = json.loads(report_path.read_text(encoding="utf-8"))
        if isinstance(prev, dict) and isinstance(prev.get("elapsed_ms"), int):
            previous_elapsed_ms = int(prev["elapsed_ms"])
    except json.JSONDecodeError:
        previous_elapsed_ms = None

report = {
    "kind": "genesis/selfhost-readiness-v0.1",
    "ok": ok,
    "score_percent": score,
    "elapsed_ms": elapsed_ms,
    "budget_ms": budget_ms,
    "history_samples": history_samples,
    "history_p95_ms": history_p95_ms,
    "history_p95_enforced": history_p95_enforced,
    "history_p95_ok": history_p95_ok,
    "p95_min_samples": p95_min_samples,
    "fail_reasons": fail_reasons,
    "dashboard_kind": dashboard_doc.get("kind"),
    "dashboard_ok": bool(dashboard_doc.get("ok", False)),
    "dashboard_markdown": dashboard_doc.get("data", {}).get("markdown"),
    "unresolved_upgrade_plan_ids": open_upgrade_ids,
    "closure_ok": closure_ok,
    "dimensions": dimensions,
    "timestamp_utc": now_utc,
}
if previous_elapsed_ms is not None:
    report["previous_elapsed_ms"] = previous_elapsed_ms
    report["elapsed_delta_ms"] = elapsed_ms - previous_elapsed_ms

history_entry = {
    "kind": report["kind"],
    "ok": report["ok"],
    "score_percent": report["score_percent"],
    "elapsed_ms": report["elapsed_ms"],
    "budget_ms": report["budget_ms"],
    "timestamp_utc": report["timestamp_utc"],
    "unresolved_upgrade_plan_ids": len(open_upgrade_ids),
}

report_path.parent.mkdir(parents=True, exist_ok=True)
history_path.parent.mkdir(parents=True, exist_ok=True)
write_text_atomic(report_path, json.dumps(report, indent=2, sort_keys=True) + "\n")
history_line = json.dumps(history_entry, sort_keys=True) + "\n"
history_fd = os.open(history_path, os.O_APPEND | os.O_CREAT | os.O_WRONLY, 0o644)
try:
    os.write(history_fd, history_line.encode("utf-8"))
finally:
    os.close(history_fd)

print(
    "selfhost-readiness: "
    f"report={report_path} score={score} ok={ok} "
    f"open_upgrade_ids={len(open_upgrade_ids)}"
)

if not ok:
    message = "selfhost-readiness: not-ready reasons: " + ", ".join(fail_reasons)
    if strict_mode:
        raise SystemExit(message)
    print(message)
PY
