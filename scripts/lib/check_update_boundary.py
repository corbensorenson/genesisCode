#!/usr/bin/env python3
"""Inventory and ratchet GenesisCode check/update boundary behavior."""

from __future__ import annotations

import argparse
import copy
from hashlib import sha256
import json
import os
from pathlib import Path
import re
import sys
import tempfile
from typing import Any, Dict, Iterable, List, Mapping, Sequence


ROOT = Path(__file__).resolve().parents[2]
POLICY_PATH = ROOT / "policies/check_update_boundary_v0.1.json"
REPORT_PATH = ROOT / "docs/spec/CHECK_UPDATE_BOUNDARY_AUDIT_v0.1.json"
CHECK_GLOB = "check_*.sh"

COMPILE_RE = re.compile(
    r"(?:^|[;&|\s])(?:cargo\s+(?:build|test|run|check|clippy|metadata|package)|rustc\s|lake\s|lean\s|npm\s|pnpm\s|yarn\s)"
)
PYTHON_COMPILE_RE = re.compile(
    r"[\"']cargo[\"']\s*,\s*[\"'](?:build|test|run|check|clippy|metadata|package)[\"']"
)
DYNAMIC_COMPILE_SUBJECT_RE = re.compile(r"boundary:\s*dynamic-compilation-subject")
NETWORK_RE = re.compile(
    r"^\s*(?:(?:if|elif|while)\s+!?\s*)?(?:[A-Z_][A-Z0-9_]*=[^\s]+\s+)*"
    r"(?:curl\s|wget\s|git\s+(?:fetch|pull|clone)\s|cargo\s+(?:fetch|install)\s)"
)
PYTHON_NETWORK_RE = re.compile(
    r"[\"'](?:curl|wget)[\"']\s*,|[\"']git[\"']\s*,\s*[\"'](?:fetch|pull|clone)[\"']"
)
UPDATE_CALL_RE = re.compile(
    r"^\s*(?:(?:if|elif|while)\b[^;]*;\s*then\s*)?(?:bash|exec|source)\s+(?:\"?\$ROOT_DIR/\"?)?scripts/update_[A-Za-z0-9_.-]+\.sh\b"
)
CHECK_CALL_RE = re.compile(
    r"(?:bash|exec)\s+(?:\"?\$ROOT_DIR/\"?)?scripts/(check_[A-Za-z0-9_.-]+\.sh)\b"
)
LOCAL_SCRIPT_CALL_RE = re.compile(
    r"(?:bash|exec)\s+(?:\"?\$ROOT_DIR/)?scripts/([A-Za-z0-9_./-]+\.sh)\b"
)
SOURCE_SCRIPT_CALL_RE = re.compile(
    r"^\s*(?:source|\.)\s+(?:\"?\$(?:ROOT_DIR|ROOT)/)?scripts/([A-Za-z0-9_./-]+\.sh)\b"
)
DEFAULT_REFRESH_TRUE_RE = re.compile(r"\$\{[^}\n]*REFRESH[^}\n]*:-1\}")
REFRESH_CONTROL_RE = re.compile(r"\b[A-Z0-9_]*REFRESH[A-Z0-9_]*\b")
PERSISTENT_OUTPUT_RE = re.compile(
    r"^\s*[A-Z0-9_]*(?:REPORT|HISTORY|OUT)[A-Z0-9_]*=.*\.genesis/perf(?:/|\b)"
)
WRITE_CANDIDATE_RE = re.compile(
    r"(?:write_text\(|write_bytes\(|json\.dump\(|os\.replace\(|mkdir\(|mkdir\s+-p|\btee\s|>>|\brm\s+-|\btouch\s)"
)


class BoundaryError(ValueError):
    pass


def duplicate_safe_object(pairs: Sequence[tuple[str, Any]]) -> Dict[str, Any]:
    result: Dict[str, Any] = {}
    for key, value in pairs:
        if key in result:
            raise BoundaryError(f"duplicate JSON key: {key}")
        result[key] = value
    return result


def load_json(path: Path) -> Any:
    try:
        return json.loads(
            path.read_text(encoding="utf-8"), object_pairs_hook=duplicate_safe_object
        )
    except FileNotFoundError as exc:
        raise BoundaryError(f"missing file: {path.relative_to(ROOT)}") from exc
    except json.JSONDecodeError as exc:
        raise BoundaryError(
            f"invalid JSON in {path.relative_to(ROOT)}:{exc.lineno}:{exc.colno}: {exc.msg}"
        ) from exc


def active_lines(source: str) -> List[str]:
    return [
        line
        for line in source.splitlines()
        if line.strip() and not line.lstrip().startswith("#")
    ]


def active_shell_lines(source: str) -> List[str]:
    """Return active shell lines while excluding heredoc payloads."""
    result: List[str] = []
    delimiters: List[tuple[str, bool]] = []
    heredoc_re = re.compile(
        r"<<(?P<strip>-)?\s*(?P<quote>[\"']?)(?P<delimiter>[A-Za-z_][A-Za-z0-9_]*)"
        r"(?P=quote)"
    )
    for raw_line in source.splitlines():
        if delimiters:
            delimiter, strip_tabs = delimiters[0]
            candidate = raw_line.lstrip("\t") if strip_tabs else raw_line
            if candidate == delimiter:
                delimiters.pop(0)
            continue
        stripped = raw_line.strip()
        if stripped and not raw_line.lstrip().startswith("#"):
            result.append(raw_line)
        for match in heredoc_re.finditer(raw_line):
            delimiters.append(
                (match.group("delimiter"), match.group("strip") is not None)
            )
    return result


def unique_matches(pattern: re.Pattern[str], lines: Iterable[str]) -> List[str]:
    values = []
    for line in lines:
        match = pattern.search(line)
        if match:
            values.append(match.group(1) if match.groups() else line.strip())
    return sorted(set(values))


def executable_command_lines(lines: Iterable[str]) -> List[str]:
    result = []
    for line in lines:
        stripped = line.strip()
        if re.search(r"\b(?:grep|require_[a-z0-9_]*pattern)\b", stripped):
            continue
        if re.match(r"^(?:echo|printf)\b", stripped):
            continue
        if re.match(r"^[a-z_][a-z0-9_]*\s*=", stripped):
            continue
        result.append(line)
    return result


def classify(path: Path, compilation: bool, check_calls: Sequence[str]) -> str:
    name = path.name
    if re.search(r"(?:perf|budget|slo|stress|microbench|hot_path|headroom)", name):
        return "benchmark"
    if compilation:
        return "build-runtime"
    if check_calls:
        return "aggregate"
    return "static"


def scan_script(path: Path) -> Mapping[str, Any]:
    source = path.read_text(encoding="utf-8")
    lines = active_lines(source)
    command_lines = executable_command_lines(active_shell_lines(source))
    compilation = (
        bool(COMPILE_RE.search("\n".join(command_lines)))
        or bool(PYTHON_COMPILE_RE.search(source))
        or bool(DYNAMIC_COMPILE_SUBJECT_RE.search(source))
    )
    network = any(NETWORK_RE.search(line) for line in command_lines) or bool(
        PYTHON_NETWORK_RE.search(source)
    )
    update_calls = unique_matches(UPDATE_CALL_RE, command_lines)
    check_calls = unique_matches(CHECK_CALL_RE, command_lines)
    local_script_calls = sorted(
        set(unique_matches(LOCAL_SCRIPT_CALL_RE, command_lines))
        | set(unique_matches(SOURCE_SCRIPT_CALL_RE, command_lines))
    )
    execution_helpers = sorted(
        {
            f"scripts/{name}"
            for name in local_script_calls
            if not Path(name).name.startswith(("check_", "update_"))
        }
    )
    persistent_candidates = sorted(
        {
            line.strip()
            for line in lines
            if PERSISTENT_OUTPUT_RE.search(line)
        }
    )
    refresh_controls = sorted(
        {
            token
            for line in lines
            for token in REFRESH_CONTROL_RE.findall(line)
        }
    )
    write_candidates = sorted(
        {line.strip() for line in lines if WRITE_CANDIDATE_RE.search(line)}
    )
    persistent_writer_signal = bool(write_candidates) or any(
        marker in source
        for marker in (
            "genesis_profile_gate_emit_runtime_report",
            '--report "$',
            '--history "$',
        )
    )
    persistent_refs = persistent_candidates if persistent_writer_signal else []
    read_only_persistent_inputs = (
        [] if persistent_writer_signal else persistent_candidates
    )
    rel = path.relative_to(ROOT).as_posix()
    return {
        "path": rel,
        "sha256": sha256(path.read_bytes()).hexdigest(),
        "class": classify(path, compilation, check_calls),
        "compilation_detected": compilation,
        "network_detected": network,
        "default_refresh_true": bool(DEFAULT_REFRESH_TRUE_RE.search(source)),
        "refresh_controls": refresh_controls,
        "update_invocations": update_calls,
        "check_invocations": check_calls,
        "execution_helper_invocations": execution_helpers,
        "persistent_output_references": persistent_refs,
        "read_only_persistent_inputs": read_only_persistent_inputs,
        "write_candidates": write_candidates,
        "r0_2_compliant": not persistent_refs
        and not update_calls
        and not DEFAULT_REFRESH_TRUE_RE.search(source),
    }


def scan_all() -> List[Mapping[str, Any]]:
    rows = [dict(scan_script(path)) for path in sorted((ROOT / "scripts").glob(CHECK_GLOB))]
    helper_cache: Dict[str, Mapping[str, Any]] = {}

    def helper_row(rel: str) -> Mapping[str, Any]:
        cached = helper_cache.get(rel)
        if cached is not None:
            return cached
        path = ROOT / rel
        if not path.is_file():
            raise BoundaryError(f"check invokes missing local helper: {rel}")
        scanned = scan_script(path)
        helper_cache[rel] = scanned
        return scanned

    for row in rows:
        direct_compilation = bool(row["compilation_detected"])
        direct_network = bool(row["network_detected"])
        direct_default_refresh = bool(row["default_refresh_true"])
        direct_update_calls = list(row["update_invocations"])
        direct_check_calls = list(row["check_invocations"])
        direct_persistent_refs = list(row["persistent_output_references"])
        direct_read_only_inputs = list(row["read_only_persistent_inputs"])
        direct_refresh_controls = list(row["refresh_controls"])
        direct_write_candidates = list(row["write_candidates"])

        closure: Dict[str, Mapping[str, Any]] = {}
        pending = list(row["execution_helper_invocations"])
        while pending:
            helper_path = pending.pop()
            if helper_path in closure:
                continue
            scanned = helper_row(helper_path)
            closure[helper_path] = scanned
            pending.extend(
                str(item)
                for item in scanned["execution_helper_invocations"]
                if str(item) not in closure
            )

        closure_rows = [closure[path] for path in sorted(closure)]
        row["direct_compilation_detected"] = direct_compilation
        row["direct_network_detected"] = direct_network
        row["direct_default_refresh_true"] = direct_default_refresh
        row["direct_update_invocations"] = direct_update_calls
        row["direct_check_invocations"] = direct_check_calls
        row["direct_persistent_output_references"] = direct_persistent_refs
        row["direct_read_only_persistent_inputs"] = direct_read_only_inputs
        row["direct_refresh_controls"] = direct_refresh_controls
        row["direct_write_candidates"] = direct_write_candidates
        row["execution_helper_closure"] = [
            {"path": helper["path"], "sha256": helper["sha256"]}
            for helper in closure_rows
        ]
        identity_rows = [{"path": row["path"], "sha256": row["sha256"]}]
        identity_rows.extend(row["execution_helper_closure"])
        row["execution_identity_sha256"] = sha256(
            json.dumps(identity_rows, sort_keys=True, separators=(",", ":")).encode(
                "utf-8"
            )
        ).hexdigest()
        row["compilation_detected"] = direct_compilation or any(
            bool(helper["compilation_detected"]) for helper in closure_rows
        )
        row["network_detected"] = direct_network or any(
            bool(helper["network_detected"]) for helper in closure_rows
        )
        row["default_refresh_true"] = direct_default_refresh or any(
            bool(helper["default_refresh_true"]) for helper in closure_rows
        )
        row["update_invocations"] = sorted(
            set(direct_update_calls).union(
                *(set(helper["update_invocations"]) for helper in closure_rows)
            )
        )
        row["check_invocations"] = sorted(
            set(direct_check_calls).union(
                *(set(helper["check_invocations"]) for helper in closure_rows)
            )
        )
        row["persistent_output_references"] = sorted(
            direct_persistent_refs
            + [
                f"{helper['path']}: {reference}"
                for helper in closure_rows
                for reference in helper["persistent_output_references"]
            ]
        )
        row["read_only_persistent_inputs"] = sorted(
            direct_read_only_inputs
            + [
                f"{helper['path']}: {reference}"
                for helper in closure_rows
                for reference in helper["read_only_persistent_inputs"]
            ]
        )
        row["refresh_controls"] = sorted(
            set(direct_refresh_controls).union(
                *(set(helper["refresh_controls"]) for helper in closure_rows)
            )
        )
        row["write_candidates"] = sorted(
            direct_write_candidates
            + [
                f"{helper['path']}: {candidate}"
                for helper in closure_rows
                for candidate in helper["write_candidates"]
            ]
        )
        row["mutation_helper_invocations"] = sorted(
            helper["path"]
            for helper in closure_rows
            if Path(str(helper["path"])).name == "reclaim_build_space.sh"
        )
        row["class"] = classify(
            ROOT / str(row["path"]),
            bool(row["compilation_detected"]),
            row["check_invocations"],
        )
        row["r0_2_compliant"] = (
            not row["persistent_output_references"]
            and not row["update_invocations"]
            and not row["default_refresh_true"]
            and not row["mutation_helper_invocations"]
        )

    by_path = {str(row["path"]): row for row in rows}
    changed = True
    while changed:
        changed = False
        for row in rows:
            transitive = sorted(
                {
                    f"scripts/{name}"
                    for name in row["check_invocations"]
                    if f"scripts/{name}" in by_path
                    and not bool(by_path[f"scripts/{name}"]["r0_2_compliant"])
                }
            )
            if transitive != row.get("transitive_noncompliant_checks", []):
                row["transitive_noncompliant_checks"] = transitive
                changed = True
            compliant = (
                not row["persistent_output_references"]
                and not row["update_invocations"]
                and not row["default_refresh_true"]
                and not row["mutation_helper_invocations"]
                and not transitive
            )
            if compliant != row["r0_2_compliant"]:
                row["r0_2_compliant"] = compliant
                changed = True
    for row in rows:
        row.setdefault("transitive_noncompliant_checks", [])
    return rows


def require_string_map(value: Any, label: str) -> Mapping[str, str]:
    if not isinstance(value, dict):
        raise BoundaryError(f"{label} must be an object")
    result: Dict[str, str] = {}
    for key, reason in value.items():
        if not isinstance(key, str) or not key:
            raise BoundaryError(f"{label} contains an invalid path key")
        if not isinstance(reason, str) or not reason.strip():
            raise BoundaryError(f"{label}[{key}] must contain a non-empty reason")
        result[key] = reason
    return result


def validate_policy(policy: Any, observations: Sequence[Mapping[str, Any]]) -> None:
    if not isinstance(policy, dict):
        raise BoundaryError("policy must be an object")
    if policy.get("kind") != "genesis/check-update-boundary-policy-v0.1":
        raise BoundaryError("invalid check/update boundary policy kind")
    if policy.get("version") != "0.1":
        raise BoundaryError("invalid check/update boundary policy version")
    expected_scripts = policy.get("expected_scripts")
    if not isinstance(expected_scripts, list) or not all(
        isinstance(item, str) and item for item in expected_scripts
    ):
        raise BoundaryError("policy.expected_scripts must be a string array")
    if len(expected_scripts) != len(set(expected_scripts)):
        raise BoundaryError("policy.expected_scripts contains duplicates")
    actual_scripts = [str(row["path"]) for row in observations]
    if sorted(expected_scripts) != sorted(actual_scripts):
        missing = sorted(set(expected_scripts) - set(actual_scripts))
        extra = sorted(set(actual_scripts) - set(expected_scripts))
        raise BoundaryError(
            f"check inventory drift: missing={missing} unreviewed={extra}; "
            "run scripts/update_check_update_boundary_audit.sh after reviewing policy"
        )

    compile_allowlist = require_string_map(
        policy.get("compile_allowlist"), "policy.compile_allowlist"
    )
    persistent_allowlist = require_string_map(
        policy.get("legacy_persistent_output_allowlist"),
        "policy.legacy_persistent_output_allowlist",
    )
    network_allowlist = require_string_map(
        policy.get("network_allowlist"), "policy.network_allowlist"
    )
    mutation_allowlist = require_string_map(
        policy.get("legacy_mutation_helper_allowlist"),
        "policy.legacy_mutation_helper_allowlist",
    )
    actual_compile = {
        str(row["path"]) for row in observations if row["compilation_detected"]
    }
    actual_persistent = {
        str(row["path"])
        for row in observations
        if row["persistent_output_references"]
    }
    actual_network = {
        str(row["path"]) for row in observations if row["network_detected"]
    }
    actual_mutation = {
        str(row["path"])
        for row in observations
        if row["mutation_helper_invocations"]
    }
    for label, actual, declared in (
        ("compile", actual_compile, set(compile_allowlist)),
        ("legacy persistent output", actual_persistent, set(persistent_allowlist)),
        ("network", actual_network, set(network_allowlist)),
        ("mutation helper", actual_mutation, set(mutation_allowlist)),
    ):
        if actual != declared:
            raise BoundaryError(
                f"{label} policy drift: removed={sorted(declared-actual)} "
                f"unreviewed={sorted(actual-declared)}"
            )

    ratchets = policy.get("ratchets")
    if not isinstance(ratchets, dict):
        raise BoundaryError("policy.ratchets must be an object")
    metrics = {
        "max_check_scripts": len(observations),
        "max_compile_scripts": len(actual_compile),
        "max_network_scripts": len(actual_network),
        "max_mutation_helper_scripts": len(actual_mutation),
        "max_legacy_persistent_output_scripts": len(actual_persistent),
        "max_default_refresh_true_scripts": sum(
            1 for row in observations if row["default_refresh_true"]
        ),
        "max_update_invocation_scripts": sum(
            1 for row in observations if row["update_invocations"]
        ),
    }
    for metric, actual in metrics.items():
        limit = ratchets.get(metric)
        if not isinstance(limit, int) or limit < 0:
            raise BoundaryError(f"policy.ratchets.{metric} must be a non-negative integer")
        if actual > limit:
            raise BoundaryError(f"boundary ratchet exceeded: {metric}={actual} limit={limit}")
    if ratchets.get("target_legacy_persistent_output_scripts") != 0:
        raise BoundaryError("persistent-output target must remain zero")


def render_report(
    policy: Mapping[str, Any], observations: Sequence[Mapping[str, Any]]
) -> str:
    compile_allowlist = policy["compile_allowlist"]
    persistent_allowlist = policy["legacy_persistent_output_allowlist"]
    network_allowlist = policy["network_allowlist"]
    mutation_allowlist = policy["legacy_mutation_helper_allowlist"]
    entries = []
    for row in observations:
        item = dict(row)
        path = str(item["path"])
        item["compilation_declaration"] = compile_allowlist.get(path)
        item["legacy_persistent_output_declaration"] = persistent_allowlist.get(path)
        item["network_declaration"] = network_allowlist.get(path)
        item["mutation_helper_declaration"] = mutation_allowlist.get(path)
        entries.append(item)
    summary = {
        "check_scripts": len(entries),
        "compile_scripts": sum(1 for row in entries if row["compilation_detected"]),
        "network_scripts": sum(1 for row in entries if row["network_detected"]),
        "mutation_helper_scripts": sum(
            1 for row in entries if row["mutation_helper_invocations"]
        ),
        "legacy_persistent_output_scripts": sum(
            1 for row in entries if row["persistent_output_references"]
        ),
        "default_refresh_true_scripts": sum(
            1 for row in entries if row["default_refresh_true"]
        ),
        "update_invocation_scripts": sum(
            1 for row in entries if row["update_invocations"]
        ),
        "r0_2_compliant_scripts": sum(1 for row in entries if row["r0_2_compliant"]),
    }
    summary["r0_2_a_complete"] = (
        summary["legacy_persistent_output_scripts"] == 0
        and summary["default_refresh_true_scripts"] == 0
        and summary["update_invocation_scripts"] == 0
        and summary["mutation_helper_scripts"] == 0
    )
    report = {
        "kind": "genesis/check-update-boundary-audit-v0.1",
        "version": "0.1",
        "policy": "policies/check_update_boundary_v0.1.json",
        "target": {
            "legacy_persistent_output_scripts": 0,
            "default_refresh_true_scripts": 0,
            "update_invocation_scripts": 0,
            "mutation_helper_scripts": 0,
        },
        "summary": summary,
        "entries": entries,
    }
    return json.dumps(report, indent=2, sort_keys=True) + "\n"


def atomic_write(path: Path, content: str) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    fd, temp_name = tempfile.mkstemp(prefix=f".{path.name}.", dir=path.parent)
    try:
        with os.fdopen(fd, "w", encoding="utf-8", newline="\n") as handle:
            handle.write(content)
        os.replace(temp_name, path)
    except BaseException:
        try:
            os.unlink(temp_name)
        except FileNotFoundError:
            pass
        raise


def run_self_tests(
    policy: Mapping[str, Any], observations: Sequence[Mapping[str, Any]]
) -> None:
    controls = 0

    def expect_rejected(label: str, candidate_policy: Any, candidate_rows: Any) -> None:
        nonlocal controls
        try:
            validate_policy(candidate_policy, candidate_rows)
        except BoundaryError:
            controls += 1
            return
        raise BoundaryError(f"self-test expected rejection: {label}")

    extra_script = copy.deepcopy(list(observations))
    extra_script.append({**extra_script[0], "path": "scripts/check_unreviewed_fixture.sh"})
    expect_rejected("unreviewed script", policy, extra_script)

    undeclared_compile = copy.deepcopy(list(observations))
    static_row = next(row for row in undeclared_compile if not row["compilation_detected"])
    static_row["compilation_detected"] = True
    expect_rejected("undeclared compilation", policy, undeclared_compile)

    persistent_growth = copy.deepcopy(list(observations))
    clean_row = next(
        row for row in persistent_growth if not row["persistent_output_references"]
    )
    clean_row["persistent_output_references"] = ["REPORT=.genesis/perf/fixture.json"]
    expect_rejected("persistent output growth", policy, persistent_growth)

    default_refresh = copy.deepcopy(list(observations))
    default_refresh[0]["default_refresh_true"] = True
    expect_rejected("default refresh", policy, default_refresh)

    update_invocation = copy.deepcopy(list(observations))
    update_invocation[0]["update_invocations"] = ["scripts/update_fixture.sh"]
    expect_rejected("check invokes update", policy, update_invocation)

    network_growth = copy.deepcopy(list(observations))
    network_row = next(row for row in network_growth if not row["network_detected"])
    network_row["network_detected"] = True
    expect_rejected("undeclared network", policy, network_growth)

    mutation_growth = copy.deepcopy(list(observations))
    mutation_row = next(
        row for row in mutation_growth if not row["mutation_helper_invocations"]
    )
    mutation_row["mutation_helper_invocations"] = [
        "scripts/reclaim_build_space.sh"
    ]
    expect_rejected("undeclared mutation helper", policy, mutation_growth)

    diagnostic_lines = active_shell_lines(
        'echo "rerun: bash scripts/check_fixture.sh"\n'
        "python3 - <<'PY'\n"
        'command = "bash scripts/check_heredoc_fixture.sh"\n'
        "PY\n"
        'bash scripts/check_actual_fixture.sh\n'
    )
    diagnostic_calls = unique_matches(
        CHECK_CALL_RE, executable_command_lines(diagnostic_lines)
    )
    if diagnostic_calls != ["check_actual_fixture.sh"]:
        raise BoundaryError(
            "self-test expected quoted diagnostic check command to be ignored"
        )
    controls += 1

    invalid_target = copy.deepcopy(policy)
    invalid_target["ratchets"]["target_legacy_persistent_output_scripts"] = 1
    expect_rejected("nonzero target", invalid_target, observations)

    print(f"check-update-boundary-contract: ok (negative_controls={controls})")


def main(argv: Sequence[str]) -> int:
    parser = argparse.ArgumentParser()
    mode = parser.add_mutually_exclusive_group(required=True)
    mode.add_argument("--check", action="store_true")
    mode.add_argument("--update", action="store_true")
    mode.add_argument("--print", dest="print_report", action="store_true")
    mode.add_argument("--self-test", action="store_true")
    args = parser.parse_args(argv)
    try:
        observations = scan_all()
        policy = load_json(POLICY_PATH)
        validate_policy(policy, observations)
        if args.self_test:
            run_self_tests(policy, observations)
            return 0
        rendered = render_report(policy, observations)
        if args.check:
            if not REPORT_PATH.is_file() or REPORT_PATH.read_text(encoding="utf-8") != rendered:
                raise BoundaryError(
                    "check/update boundary audit is stale; review changes, ratchet policy, then run "
                    "scripts/update_check_update_boundary_audit.sh"
                )
            summary = json.loads(rendered)["summary"]
            print(
                "check-update-boundary: ok "
                f"(checks={summary['check_scripts']} compliant={summary['r0_2_compliant_scripts']} "
                f"legacy_outputs={summary['legacy_persistent_output_scripts']} "
                f"default_refresh={summary['default_refresh_true_scripts']} "
                f"update_invocations={summary['update_invocation_scripts']} "
                f"mutation_helpers={summary['mutation_helper_scripts']})"
            )
        elif args.update:
            atomic_write(REPORT_PATH, rendered)
            print(f"update-check-update-boundary-audit: wrote {REPORT_PATH.relative_to(ROOT)}")
        else:
            print(rendered, end="")
    except BoundaryError as exc:
        print(f"check-update-boundary: {exc}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
