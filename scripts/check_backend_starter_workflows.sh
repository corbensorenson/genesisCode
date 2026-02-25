#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

REPORT_PATH="${GENESIS_BACKEND_STARTER_REPORT:-.genesis/perf/backend_starter_workflows_report.json}"
HISTORY_PATH="${GENESIS_BACKEND_STARTER_HISTORY:-.genesis/perf/backend_starter_workflows_history.jsonl}"
DEFAULT_DEBUG_DIR="${CARGO_TARGET_DIR:-$ROOT_DIR/target}/debug"
GENESIS_BIN="${GENESIS_BIN:-$DEFAULT_DEBUG_DIR/genesis}"

if [[ ! -x "$GENESIS_BIN" ]]; then
  cargo build -p gc_cli >/dev/null
fi

python3 - "$ROOT_DIR" "$GENESIS_BIN" "$REPORT_PATH" "$HISTORY_PATH" <<'PY'
import datetime as dt
import json
import pathlib
import re
import subprocess
import tempfile
import time
import sys

root = pathlib.Path(sys.argv[1]).resolve()
genesis_bin = pathlib.Path(sys.argv[2]).resolve()
report_path = root / sys.argv[3]
history_path = root / sys.argv[4]

def run_cmd(argv, cwd):
    proc = subprocess.run(
        argv,
        cwd=str(cwd),
        capture_output=True,
        text=True,
        check=False,
    )
    return {
        "argv": argv,
        "cwd": str(cwd),
        "code": proc.returncode,
        "stdout": proc.stdout,
        "stderr": proc.stderr,
        "ok": proc.returncode == 0,
    }

caps_body = "allow = []\n"

smoke_cases = [
    (
        "backend_bridge_dns.gc",
        "(def prog (core/effect::perform 'io/net::dns-resolve {:name \"localhost\"} (fn (x) (core/effect::pure x)))) prog\n",
        [":addrs", ':backend "first-party-backend-bridge"'],
    ),
    (
        "backend_bridge_db.gc",
        "(def prog (core/effect::perform 'io/db::connect {:target \"sqlite://data/backend_starter.db\"} (fn (x) (core/effect::pure x)))) prog\n",
        [":connection-id", ':backend "first-party-backend-bridge"'],
    ),
    (
        "backend_bridge_process.gc",
        "(def prog (core/effect::perform 'sys/process::exec {:program \"echo\" :args [\"starter-ok\"] :env {}} (fn (x) (core/effect::pure x)))) prog\n",
        [":stdout", ':backend "first-party-backend-bridge"'],
    ),
    (
        "backend_bridge_crypto.gc",
        "(def prog (core/effect::perform 'core/crypto::hash {:algorithm \"sha256\" :data \"abc\"} (fn (x) (core/effect::pure x)))) prog\n",
        [":digest", ':backend "first-party-backend-bridge"'],
    ),
    (
        "backend_bridge_plugin.gc",
        "(def prog (core/effect::perform 'host/plugin::command {:plugin \"demo\" :command \"run\" :payload {:ok true}} (fn (x) (core/effect::pure x)))) prog\n",
        [":plugin", ':backend "first-party-backend-bridge"'],
    ),
    (
        "backend_bridge_ffi.gc",
        "(def prog (core/effect::perform 'host/ffi::buffer-pin {:abi-id \"genesis/ffi.memory.v1\" :bytes \"abc\"} (fn (x) (core/effect::pure x)))) prog\n",
        [":handle", ':backend "first-party-backend-bridge"'],
    ),
]

start = time.perf_counter()
with tempfile.TemporaryDirectory(prefix="backend-starter-workflows-") as tmp_dir:
    tmp_root = pathlib.Path(tmp_dir)
    workspace = tmp_root / "workspace"
    workspace.mkdir(parents=True, exist_ok=True)
    (workspace / "caps.toml").write_text(caps_body, encoding="utf-8")

    scaffold = run_cmd(
        [
            str(genesis_bin),
            "--json",
            "gcpm",
            "--caps",
            "caps.toml",
            "scaffold",
            "--archetype",
            "service",
            "--name",
            "backend-starter",
            "--root",
            "app",
        ],
        workspace,
    )
    if not scaffold["ok"]:
        raise SystemExit(
            json.dumps(
                {
                    "kind": "genesis/backend-starter-workflows-v0.1",
                    "ok": False,
                    "error": "scaffold-failed",
                    "scaffold": scaffold,
                }
            )
        )

    app_dir = workspace / "app"
    env_run = run_cmd(
        [
            str(genesis_bin),
            "gcpm",
            "--caps",
            "caps.toml",
            "env",
            "--profile",
            "backend",
            "--runtime-backend",
            "profile-headless",
        ],
        app_dir,
    )
    if not env_run["ok"]:
        raise SystemExit(
            json.dumps(
                {
                    "kind": "genesis/backend-starter-workflows-v0.1",
                    "ok": False,
                    "error": "env-failed",
                    "env": env_run,
                }
            )
        )

    env_value = env_run["stdout"].strip()
    if not env_value:
        raise SystemExit("backend-starter-workflows: empty gcpm env output")

    def capture_string(key):
        match = re.search(rf"{re.escape(key)}\s+\"([^\"]+)\"", env_value)
        return match.group(1) if match else None

    bridge_ready = bool(re.search(r":backend-bridge-ready\s+true\b", env_value))
    bridge_cmd = capture_string(":backend-bridge-cmd")
    bridge_sha = capture_string(":backend-bridge-sha256")
    effective_caps = capture_string(":caps-policy-effective")
    if not bridge_ready:
        raise SystemExit(
            json.dumps(
                {
                    "kind": "genesis/backend-starter-workflows-v0.1",
                    "ok": False,
                    "error": "bridge-not-ready",
                    "env_value": env_value,
                }
            )
        )
    if not isinstance(bridge_cmd, str) or not bridge_cmd:
        raise SystemExit("backend-starter-workflows: missing backend bridge cmd")
    if not isinstance(bridge_sha, str) or not bridge_sha.startswith("sha256:"):
        raise SystemExit("backend-starter-workflows: missing backend bridge sha256 pin")
    if not isinstance(effective_caps, str) or not effective_caps:
        raise SystemExit("backend-starter-workflows: missing effective caps path")

    bridge_path = pathlib.Path(bridge_cmd)
    if not bridge_path.is_absolute():
        bridge_path = (app_dir / bridge_path).resolve()
    if not bridge_path.is_file():
        raise SystemExit(
            f"backend-starter-workflows: backend bridge command path does not exist: {bridge_path}"
        )
    if ".genesis/runtime/backend/" not in bridge_cmd.replace("\\", "/"):
        raise SystemExit(
            "backend-starter-workflows: backend bridge command is not bundled under .genesis/runtime/backend"
        )

    smoke_results = []
    for file_name, source, required_markers in smoke_cases:
        path = app_dir / file_name
        path.write_text(source, encoding="utf-8")
        log_name = f"{file_name}.gclog"
        run_result = run_cmd(
            [
                str(genesis_bin),
                "run",
                file_name,
                "--caps",
                effective_caps,
                "--log",
                log_name,
            ],
            app_dir,
        )
        replay_result = run_cmd(
            [
                str(genesis_bin),
                "replay",
                file_name,
                "--log",
                log_name,
            ],
            app_dir,
        )
        run_stdout = run_result.get("stdout", "")
        marker_failures = [
            marker for marker in required_markers if marker not in run_stdout
        ]
        smoke_results.append(
            {
                "file": file_name,
                "run_ok": run_result["ok"],
                "replay_ok": replay_result["ok"],
                "required_markers": required_markers,
                "missing_markers": marker_failures,
                "run_stdout": run_stdout.strip(),
                "run_stderr": run_result.get("stderr", "").strip(),
                "replay_stderr": replay_result.get("stderr", "").strip(),
                "ok": run_result["ok"] and replay_result["ok"] and not marker_failures,
            }
        )

elapsed_ms = int((time.perf_counter() - start) * 1000)
all_smoke_ok = all(row.get("ok", False) for row in smoke_results)
ok = bridge_ready and all_smoke_ok

report = {
    "kind": "genesis/backend-starter-workflows-v0.1",
    "ok": ok,
    "generated_at": dt.datetime.now(dt.timezone.utc).isoformat(timespec="seconds"),
    "elapsed_ms": elapsed_ms,
    "genesis_bin": str(genesis_bin),
    "bridge_ready": bridge_ready,
    "bridge_cmd": str(bridge_path),
    "bridge_sha256": bridge_sha,
    "effective_caps": effective_caps,
    "smoke_cases": smoke_results,
    "smoke_case_count": len(smoke_results),
    "smoke_case_failures": [row["file"] for row in smoke_results if not row["ok"]],
}

report_path.parent.mkdir(parents=True, exist_ok=True)
report_path.write_text(json.dumps(report, indent=2) + "\n", encoding="utf-8")

history_path.parent.mkdir(parents=True, exist_ok=True)
with history_path.open("a", encoding="utf-8") as fh:
    fh.write(json.dumps(report, separators=(",", ":")) + "\n")

if not ok:
    raise SystemExit(
        "backend-starter-workflows: failure(s): "
        + ", ".join(report["smoke_case_failures"])
    )

print("backend-starter-workflows: ok")
PY
