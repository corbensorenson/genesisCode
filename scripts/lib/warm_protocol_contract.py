#!/usr/bin/env python3
"""Verify the closed warm v0.2 schema and its production authority."""

from __future__ import annotations

import argparse
import copy
import json
from pathlib import Path
import re
import sys
from typing import Any


ROOT = Path(__file__).resolve().parents[2]
SCHEMA_PATH = ROOT / "docs/spec/WARM_PROTOCOL_v0.2.schema.json"
RESOURCE_SCHEMA_PATH = ROOT / "docs/spec/AGENT_SESSION_RESOURCES_v0.1.schema.json"
TRANSACTION_SCHEMA_PATH = ROOT / "docs/spec/AGENT_TRANSACTION_v0.1.schema.json"
DOC_PATH = ROOT / "docs/spec/CLI_JSON_SCHEMAS_v0.1.md"
SOURCE_PATHS = (
    ROOT / "crates/gc_cli_driver/src/warm_protocol.rs",
    ROOT / "crates/gc_cli_driver/src/warm_request.rs",
    ROOT / "crates/gc_cli_driver/src/session_resources.rs",
    ROOT / "crates/gc_cli_driver/src/warm_session.rs",
    ROOT / "crates/gc_cli_driver/src/warm_session/admission.rs",
    ROOT / "crates/gc_cli_driver/src/warm_session_config.rs",
    ROOT / "crates/gc_cli_driver/src/warm_state.rs",
    ROOT / "crates/gc_cli_driver/src/warm_worker.rs",
    ROOT / "crates/gc_cli_driver/src/warm_worker_process.rs",
    ROOT / "crates/gc_cli_driver/src/warm_worker_process/platform.rs",
    ROOT / "crates/gc_cli_driver/src/warm_workspace.rs",
)
TEST_PATH = ROOT / "crates/gc_cli/tests/cli_warm.rs"
MCP_SOURCE_PATHS = (
    ROOT / "crates/gc_cli_driver/src/mcp/catalog.rs",
    ROOT / "crates/gc_cli_driver/src/mcp/resources.rs",
    ROOT / "crates/gc_cli_driver/src/mcp/session.rs",
    ROOT / "crates/gc_cli_driver/src/mcp/session/cancellation.rs",
    ROOT / "crates/gc_cli_driver/src/mcp/session/roots.rs",
    ROOT / "crates/gc_cli_driver/src/mcp/session/transport.rs",
    ROOT / "crates/gc_cli_driver/src/mcp/session/wire.rs",
    ROOT / "crates/gc_cli_driver/src/cli_schema.rs",
)
MCP_TEST_PATH = ROOT / "crates/gc_cli/tests/cli_mcp.rs"
TRANSACTION_SOURCE_PATHS = (
    ROOT / "crates/gc_cli_driver/src/agent_session.rs",
    ROOT / "crates/gc_cli_driver/src/agent_session/model.rs",
    ROOT / "crates/gc_cli_driver/src/agent_session/storage.rs",
    ROOT / "crates/gc_cli_driver/src/cli_args.rs",
    ROOT / "crates/gc_cli_driver/src/cli_args/command_groups.rs",
    ROOT / "crates/gc_cli_driver/src/mcp/catalog.rs",
)
TRANSACTION_TEST_PATH = ROOT / "crates/gc_cli/tests/cli_agent_session.rs"

PROTOCOL = "genesis/warm-protocol-v0.2"
RESPONSE = "genesis/warm-response-v0.2"
ERROR = "genesis/warm-protocol-error-v0.2"
SESSION = "genesis/warm-session-v0.2"
METHODS = {"initialize", "execute", "cancel", "ping", "restart", "shutdown"}
REQUIRED_ERROR_CODES = {
    "warm/cancel-target",
    "warm/cancelled",
    "warm/deadline-exceeded",
    "warm/drain-bounded",
    "warm/drain-timeout",
    "warm/duplicate-id",
    "warm/frame-fields",
    "warm/frame-json",
    "warm/frame-too-large",
    "warm/frame-utf8",
    "warm/nested",
    "warm/not-initialized",
    "warm/protocol-version",
    "warm/queue-full",
    "warm/resource-exceeded",
    "warm/resource-override",
    "warm/restart-busy",
    "warm/session-limit",
    "warm/worker-abort",
    "warm/worker-crash",
    "warm/worker-launch",
    "warm/worker-restarted",
    "warm/workspace-escape",
    "warm/workspace-path-escape",
    "warm/workspace-rebind",
}
REQUIRED_TESTS = {
    "warm_v02_matches_cold_json_and_isolates_workspaces",
    "warm_v02_rejects_uninitialized_duplicate_unknown_and_nested_frames",
    "warm_v02_recovers_after_an_oversized_frame",
    "warm_v02_counts_rejected_transport_frames_toward_the_session_limit",
    "warm_v02_bounds_the_queue_and_cancels_queued_work",
    "warm_v02_suppresses_running_results_after_cancel_or_deadline",
    "warm_v02_restart_requires_renegotiation",
    "warm_v02_rejects_request_resource_overrides_before_admission",
    "warm_v02_hard_limits_kill_and_reap_native_workers_with_audits",
    "warm_v02_contains_aborted_worker_and_runs_the_next_request",
    "warm_v02_eof_terminalizes_every_accepted_request_with_a_bounded_drain",
    "warm_v02_enforces_steps_effects_processes_and_disk_with_exact_evidence",
}
MCP_PROTOCOL = "2025-11-25"
MCP_TOOLS = {
    "parse",
    "format",
    "check",
    "run",
    "test",
    "explain",
    "search-symbol",
    "session-abort",
    "session-apply",
    "session-begin",
    "session-stage",
    "session-status",
    "session-test",
    "get-card",
    "diff",
    "apply-patch",
    "verify",
    "replay",
    "package",
    "build",
}
MCP_REQUIRED_TESTS = {
    "mcp_lists_generated_tools_and_resources_without_stdout_pollution",
    "mcp_negotiates_roots_and_executes_parse_with_strict_progress",
    "mcp_executes_transactional_session_through_generated_cli_routes",
    "mcp_rejects_batches_tasks_and_escaped_roots_without_panicking",
    "mcp_eof_terminalizes_every_accepted_call_with_audited_provenance",
}
TRANSACTION = "genesis/agent-transaction-v0.1"
WORKSPACE_SNAPSHOT = "genesis/workspace-snapshot-v0.1"
TRANSACTION_REQUIRED_TESTS = {
    "agent_session_stages_tests_and_explicitly_applies_a_verified_snapshot",
    "agent_session_rejects_stale_live_state_without_overwriting_it",
    "agent_session_rejects_unverified_and_tampered_transactions",
}


class ContractError(ValueError):
    pass


def load_json(path: Path) -> Any:
    try:
        return json.loads(path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as exc:
        raise ContractError(f"cannot load {path.relative_to(ROOT)}: {exc}") from exc


def require(condition: bool, message: str) -> None:
    if not condition:
        raise ContractError(message)


def validate_schema(schema: Any) -> None:
    require(isinstance(schema, dict), "schema root must be an object")
    require(schema.get("$id") == PROTOCOL, "schema protocol identity drift")
    require(
        schema.get("$schema") == "https://json-schema.org/draft/2020-12/schema",
        "schema dialect drift",
    )
    defs = schema.get("$defs")
    require(isinstance(defs, dict), "schema must define $defs")
    expected_defs = {
        "id",
        "client",
        "workspace",
        "initialize",
        "execute",
        "cancel",
        "control",
        "error",
        "meta",
        "sessionAudit",
        "response",
    }
    require(set(defs) == expected_defs, "schema definition set drift")
    required_fields = {
        "client": {"name", "version"},
        "workspace": {"id", "root"},
        "initialize": {"protocol", "id", "method", "client"},
        "execute": {"protocol", "id", "method", "workspace", "argv"},
        "cancel": {"protocol", "id", "method", "target_id"},
        "control": {"protocol", "id", "method"},
        "error": {"schema", "code", "message", "retryable", "details"},
        "meta": {
            "generation",
            "sequence",
            "session_cache_key",
            "queue_depth",
            "workspace_count",
            "evicted_workspace_count",
            "crash_count",
        },
        "response": {"protocol", "id", "kind", "ok", "status", "data", "error", "meta"},
    }
    for name, expected_required in required_fields.items():
        require(
            defs[name].get("additionalProperties") is False,
            f"$defs.{name} must be closed",
        )
        properties = defs[name].get("properties")
        required = defs[name].get("required")
        require(isinstance(properties, dict), f"$defs.{name}.properties missing")
        require(
            isinstance(required, list) and set(required) == expected_required,
            f"$defs.{name} required/property closure drift",
        )
    require(
        defs["initialize"]["properties"]["protocol"].get("const") == PROTOCOL,
        "initialize protocol const drift",
    )
    require(
        defs["response"]["properties"]["kind"].get("const") == RESPONSE,
        "response kind drift",
    )
    require(
        defs["error"]["properties"]["schema"].get("const") == ERROR,
        "error schema drift",
    )
    methods = {
        defs["initialize"]["properties"]["method"]["const"],
        defs["execute"]["properties"]["method"]["const"],
        defs["cancel"]["properties"]["method"]["const"],
        *defs["control"]["properties"]["method"]["enum"],
    }
    require(methods == METHODS, "schema method set drift")
    argv = defs["execute"]["properties"]["argv"]
    require(argv.get("minItems") == 1 and argv.get("maxItems") == 256, "argv bound drift")
    require(argv.get("items", {}).get("maxLength") == 16384, "argv entry bound drift")
    deadline = defs["execute"]["properties"]["deadline_ms"]
    require(
        deadline.get("minimum") == 1 and deadline.get("maximum") == 86_400_000,
        "deadline bound drift",
    )
    key = defs["meta"]["properties"]["session_cache_key"]
    require(key.get("pattern") == "^[0-9a-f]{64}$", "cache-key shape drift")


def validate_resource_schema(schema: Any) -> None:
    require(isinstance(schema, dict), "resource schema root must be an object")
    require(
        schema.get("$id") == "genesis/agent-session-resources-v0.1",
        "resource schema identity drift",
    )
    require(
        schema.get("$schema") == "https://json-schema.org/draft/2020-12/schema",
        "resource schema dialect drift",
    )
    defs = schema.get("$defs")
    require(
        isinstance(defs, dict)
        and set(defs) == {"nonnegative", "nullableNonnegative", "limits", "observed", "enforcement", "audit"},
        "resource schema definition set drift",
    )
    expected = {
        "limits": {
            "max_wall_ms", "max_cpu_ms", "max_steps", "max_heap_bytes",
            "max_output_bytes", "max_effects", "max_processes", "max_disk_bytes",
            "max_drain_requests", "drain_timeout_ms",
        },
        "observed": {
            "wall_ms", "cpu_ms", "output_bytes", "disk_delta_bytes", "effect_ops",
            "peak_heap_bytes", "peak_processes",
        },
        "enforcement": {"wall", "cpu", "steps", "heap", "output", "effects", "processes", "disk"},
        "audit": {
            "kind", "worker_profile", "limits_identity", "observed", "enforcement",
            "termination", "exceeded",
        },
    }
    for name, fields in expected.items():
        definition = defs[name]
        require(definition.get("additionalProperties") is False, f"resource {name} must be closed")
        require(set(definition.get("required", [])) == fields, f"resource {name} required fields drift")
        require(set(definition.get("properties", {})) == fields, f"resource {name} property closure drift")
    require(
        defs["limits"]["properties"]["max_processes"].get("maximum") == 64,
        "process limit bound drift",
    )
    require(
        defs["limits"]["properties"]["max_drain_requests"].get("minimum") == 0,
        "zero-drain profile must remain representable",
    )
    require(
        set(defs["audit"]["properties"]["worker_profile"].get("enum", []))
        == {"native-isolated-v0.1", "wasi-inline-v0.1", "not-started-v0.1"},
        "worker profile set drift",
    )
    require(
        set(defs["audit"]["properties"]["exceeded"].get("enum", []))
        == {None, "wall", "cpu", "steps", "heap", "output", "effects", "processes", "disk"},
        "resource dimension set drift",
    )


def validate_transaction_schema(schema: Any) -> None:
    require(isinstance(schema, dict), "transaction schema root must be an object")
    require(schema.get("$id") == TRANSACTION, "transaction schema identity drift")
    require(
        schema.get("$schema") == "https://json-schema.org/draft/2020-12/schema",
        "transaction schema dialect drift",
    )
    defs = schema.get("$defs")
    expected_defs = {
        "hash",
        "sessionId",
        "relativePath",
        "patch",
        "verification",
        "session",
        "snapshotFile",
        "snapshot",
    }
    require(isinstance(defs, dict) and set(defs) == expected_defs, "transaction defs drift")
    required = {
        "patch": {
            "patch",
            "before_snapshot",
            "after_snapshot",
            "obligations_ok",
            "acceptance",
        },
        "verification": {"snapshot", "acceptance", "obligations_ok"},
        "session": {
            "schema",
            "session",
            "package_manifest",
            "base_snapshot",
            "current_snapshot",
            "status",
            "patches",
            "verification",
        },
        "snapshotFile": {"path", "blob", "bytes"},
        "snapshot": {"schema", "identity", "files"},
    }
    for name, fields in required.items():
        definition = defs[name]
        require(definition.get("additionalProperties") is False, f"{name} must be closed")
        require(set(definition.get("required", [])) == fields, f"{name} required fields drift")
        require(set(definition.get("properties", {})) == fields, f"{name} property closure drift")
    require(
        defs["session"]["properties"]["schema"].get("const") == TRANSACTION,
        "transaction state identity drift",
    )
    require(
        defs["snapshot"]["properties"]["schema"].get("const") == WORKSPACE_SNAPSHOT,
        "workspace snapshot identity drift",
    )
    require(
        set(defs["session"]["properties"]["status"].get("enum", []))
        == {"open", "applied", "aborted"},
        "transaction status set drift",
    )
    require(
        defs["session"]["properties"]["patches"].get("maxItems") == 4096,
        "transaction patch bound drift",
    )
    require(
        defs["snapshot"]["properties"]["files"].get("maxItems") == 4096,
        "snapshot file bound drift",
    )
    require(
        defs["snapshotFile"]["properties"]["bytes"].get("maximum") == 268_435_456,
        "snapshot byte bound drift",
    )


def validate_transaction_authorities() -> None:
    source = "\n".join(path.read_text(encoding="utf-8") for path in TRANSACTION_SOURCE_PATHS)
    docs = DOC_PATH.read_text(encoding="utf-8")
    tests = TRANSACTION_TEST_PATH.read_text(encoding="utf-8")
    for identity in (TRANSACTION, WORKSPACE_SNAPSHOT):
        require(identity in source, f"transaction production source missing {identity}")
        require(identity in docs, f"transaction docs missing {identity}")
    for token in (
        "session/stale-base",
        "session/unverified",
        "session/workspace-tampered",
        "session/snapshot-mismatch",
        "MAX_SNAPSHOT_FILES",
        "MAX_SNAPSHOT_BYTES",
        "agent-session-patch-v0.1",
    ):
        require(token in source, f"transaction source missing {token}")
    observed = set(re.findall(r"fn (agent_session_[a-z0-9_]+)\(", tests))
    missing = sorted(TRANSACTION_REQUIRED_TESTS - observed)
    require(not missing, f"transaction integration controls missing: {missing}")


def validate_authorities() -> None:
    source = "\n".join(path.read_text(encoding="utf-8") for path in SOURCE_PATHS)
    docs = DOC_PATH.read_text(encoding="utf-8")
    tests = TEST_PATH.read_text(encoding="utf-8")
    for identity in (PROTOCOL, RESPONSE, ERROR, SESSION):
        require(identity in source, f"production source missing {identity}")
        require(identity in docs, f"normative docs missing {identity}")
    for token in (
        "native-isolated-v0.1",
        "wasi-inline-v0.1",
        "process-tree-kill-and-reap",
        "resource-killed-and-reaped",
        "worker-signal-contained",
        "genesis/agent-session-audit-v0.1",
        "max_drain_requests",
        "drain_timeout",
    ):
        require(token in source, f"session resource source missing {token}")
    require(
        re.search(r"hard[-_ ]termination", docs.lower()) is not None,
        "docs must state hard-termination boundary",
    )
    require(
        "AGENT_SESSION_RESOURCES_v0.1.schema.json" in docs,
        "docs must bind the closed session resource schema",
    )
    observed_codes = set(re.findall(r'"(warm/[a-z0-9-]+)"', source))
    missing_codes = sorted(REQUIRED_ERROR_CODES - observed_codes)
    require(not missing_codes, f"production typed errors missing: {missing_codes}")
    observed_tests = set(re.findall(r"fn (warm_v02_[a-z0-9_]+)\(", tests))
    missing_tests = sorted(REQUIRED_TESTS - observed_tests)
    require(not missing_tests, f"integration controls missing: {missing_tests}")

    mcp_source = "\n".join(path.read_text(encoding="utf-8") for path in MCP_SOURCE_PATHS)
    mcp_tests = MCP_TEST_PATH.read_text(encoding="utf-8")
    require(MCP_PROTOCOL in mcp_source, "MCP source protocol version drift")
    require(MCP_PROTOCOL in docs, "normative docs missing pinned MCP version")
    require("MCP Tasks were not negotiated" in mcp_source, "MCP Tasks must fail closed")
    require('"taskSupport": "forbidden"' in mcp_source, "core MCP tools must forbid Tasks")
    require("additionalProperties\": false" in mcp_source, "MCP input schemas must be closed")
    observed_tools = set(re.findall(r'route\(\s*"([a-z-]+)"', mcp_source))
    require(observed_tools == MCP_TOOLS, f"MCP generated tool set drift: {sorted(observed_tools)}")
    observed_mcp_tests = set(re.findall(r"fn (mcp_[a-z0-9_]+)\(", mcp_tests))
    missing_mcp_tests = sorted(MCP_REQUIRED_TESTS - observed_mcp_tests)
    require(not missing_mcp_tests, f"MCP integration controls missing: {missing_mcp_tests}")


def validate() -> None:
    validate_schema(load_json(SCHEMA_PATH))
    validate_resource_schema(load_json(RESOURCE_SCHEMA_PATH))
    validate_transaction_schema(load_json(TRANSACTION_SCHEMA_PATH))
    validate_authorities()
    validate_transaction_authorities()


def self_test() -> None:
    schema = load_json(SCHEMA_PATH)
    controls = 0
    mutations = []
    version = copy.deepcopy(schema)
    version["$id"] = "genesis/warm-protocol-v9"
    mutations.append(version)
    open_response = copy.deepcopy(schema)
    open_response["$defs"]["response"]["additionalProperties"] = True
    mutations.append(open_response)
    missing_required = copy.deepcopy(schema)
    missing_required["$defs"]["meta"]["required"].remove("crash_count")
    mutations.append(missing_required)
    unbounded = copy.deepcopy(schema)
    del unbounded["$defs"]["execute"]["properties"]["argv"]["maxItems"]
    mutations.append(unbounded)
    capability_drift = copy.deepcopy(schema)
    capability_drift["$defs"]["control"]["properties"]["method"]["enum"].append("kill")
    mutations.append(capability_drift)
    for mutation in mutations:
        try:
            validate_schema(mutation)
        except ContractError:
            controls += 1
        else:
            raise ContractError("negative schema mutation was accepted")
    transaction_schema = load_json(TRANSACTION_SCHEMA_PATH)
    transaction_mutations = []
    open_state = copy.deepcopy(transaction_schema)
    open_state["$defs"]["session"]["additionalProperties"] = True
    transaction_mutations.append(open_state)
    stale_status = copy.deepcopy(transaction_schema)
    stale_status["$defs"]["session"]["properties"]["status"]["enum"].append("committed")
    transaction_mutations.append(stale_status)
    unbounded_files = copy.deepcopy(transaction_schema)
    del unbounded_files["$defs"]["snapshot"]["properties"]["files"]["maxItems"]
    transaction_mutations.append(unbounded_files)
    missing_verification = copy.deepcopy(transaction_schema)
    missing_verification["$defs"]["session"]["required"].remove("verification")
    transaction_mutations.append(missing_verification)
    for mutation in transaction_mutations:
        try:
            validate_transaction_schema(mutation)
        except ContractError:
            controls += 1
        else:
            raise ContractError("negative transaction schema mutation was accepted")
    resource_schema = load_json(RESOURCE_SCHEMA_PATH)
    resource_mutations = []
    open_audit = copy.deepcopy(resource_schema)
    open_audit["$defs"]["audit"]["additionalProperties"] = True
    resource_mutations.append(open_audit)
    missing_cpu = copy.deepcopy(resource_schema)
    missing_cpu["$defs"]["limits"]["required"].remove("max_cpu_ms")
    resource_mutations.append(missing_cpu)
    unbounded_processes = copy.deepcopy(resource_schema)
    del unbounded_processes["$defs"]["limits"]["properties"]["max_processes"]["maximum"]
    resource_mutations.append(unbounded_processes)
    missing_heap_observation = copy.deepcopy(resource_schema)
    missing_heap_observation["$defs"]["observed"]["required"].remove("peak_heap_bytes")
    resource_mutations.append(missing_heap_observation)
    for mutation in resource_mutations:
        try:
            validate_resource_schema(mutation)
        except ContractError:
            controls += 1
        else:
            raise ContractError("negative resource schema mutation was accepted")
    print(f"warm-protocol-contract: self-test ok (negative_controls={controls})")


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--check", action="store_true")
    parser.add_argument("--self-test", action="store_true")
    args = parser.parse_args()
    if args.check == args.self_test:
        parser.error("select exactly one of --check or --self-test")
    try:
        if args.check:
            validate()
            print(
                "warm-protocol-contract: ok "
                f"(methods={len(METHODS)} required_errors={len(REQUIRED_ERROR_CODES)})"
            )
        else:
            self_test()
    except (ContractError, OSError) as exc:
        print(f"warm-protocol-contract: {exc}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
