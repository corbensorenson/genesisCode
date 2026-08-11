#!/usr/bin/env python3
"""Validate and reconcile retained tier-1 host-handle lifecycle evidence."""

from __future__ import annotations

import argparse
import copy
import hashlib
import json
from pathlib import Path
import sys
import tempfile
from typing import Any, Mapping


LINUX_REPORT_KIND = "genesis/host-bridge-fault-injection-v0.1"
MACOS_REPORT_KIND = "genesis/host-bridge-daemon-lifecycle-v0.1"
LINUX_ARTIFACT = "host-handle-lifecycle/linux/host_bridge_fault_injection_report.json"
LINUX_PRODUCER_ARTIFACT = "host-handle-lifecycle/linux/producer-worker.json"
MACOS_ARTIFACT = "host-handle-lifecycle/macos/host_bridge_daemon_lifecycle_report.json"
MACOS_PRODUCER_ARTIFACT = "host-handle-lifecycle/macos/producer-target.json"
LIFECYCLE_PATHS = [
    "success",
    "error",
    "cancellation",
    "timeout",
    "runtime-drop",
    "restart",
    "repeated-load",
]
RESOURCE_FAMILIES = [
    "filesystem",
    "network",
    "process",
    "plugin",
    "model-provider-process",
    "warm-daemon-provider-process",
]
VERIFIED_CONTROLS = [
    "bridge-success-error-cancellation-timeout-drop-restart",
    "browser-repeated-close-rejected",
    "editor-repeated-unsubscribe-rejected",
    "graphics-runtime-drop-dispatches-desktop-destroy",
    "gpu-device-explicit-destroy-rejected-after-close",
    "gpu-device-restart-rejects-stale-handles",
    "model-provider-session-success-error-timeout-drop-restart",
    "network-close-failures-cross-public-boundary-after-handle-removal",
    "spawn-pumps-cancel-before-fallback-reap",
    "warm-daemon-provider-success-error-timeout-restart-shutdown-eof",
    "warm-worker-cleanup-failures-cross-warm-and-mcp-boundaries",
    "warm-worker-pumps-cancel-before-fallback-reap",
    "xr-repeated-close-rejected",
]
DAEMON_SCENARIOS = [
    "active-eof-bounded-drain",
    "active-shutdown-bounded-drain",
    "daemon-process-restart-isolation",
    "generation-restart-renegotiation",
    "request-hard-timeout",
    "request-malformed-response",
    "request-success-owner-drop",
]
NEGATIVE_CONTROLS = [
    "accept-reaped-process-group",
    "detect-live-process-group",
    "reject-duplicate-process-identity",
    "reject-malformed-process-record",
    "reject-non-persistent-transport",
]
NONCLAIMS = [
    "does-not-standardize-the-R5.4.e-model-api",
    "does-not-qualify-unsupported-products-or-platform-packs",
    "does-not-convert-worker-observations-into-release-authority",
]


class LifecycleEvidenceError(ValueError):
    pass


def fail(message: str) -> None:
    raise LifecycleEvidenceError(message)


def exact_keys(value: Any, expected: set[str], label: str) -> Mapping[str, Any]:
    if not isinstance(value, dict) or set(value) != expected:
        observed = sorted(value) if isinstance(value, dict) else type(value).__name__
        fail(f"{label} fields mismatch: expected={sorted(expected)!r} observed={observed!r}")
    return value


def is_sha256(value: Any) -> bool:
    return (
        isinstance(value, str)
        and len(value) == 64
        and all(character in "0123456789abcdef" for character in value)
    )


def sha256_file(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for block in iter(lambda: handle.read(64 * 1024), b""):
            digest.update(block)
    return digest.hexdigest()


def load_json(path: Path) -> dict[str, Any]:
    try:
        value = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, UnicodeError, json.JSONDecodeError) as exc:
        fail(f"cannot read lifecycle evidence {path}: {exc}")
    if not isinstance(value, dict):
        fail(f"lifecycle evidence root must be an object: {path}")
    return value


def validate_daemon_facts(value: Any, *, platform: str, architecture: str) -> dict[str, Any]:
    daemon = exact_keys(
        value,
        {
            "fresh_daemon_process_isolation", "maximum_cleanup_ms",
            "no_live_provider_or_descendant", "profile", "runs", "scenarios",
            "source_identities", "verified",
        },
        "Linux daemon lifecycle facts",
    )
    if (
        daemon["profile"] != "genesis/warm-protocol-v0.2"
        or daemon["verified"] is not True
        or daemon["fresh_daemon_process_isolation"] is not True
        or daemon["no_live_provider_or_descendant"] is not True
        or daemon["runs"] != 3
        or daemon["scenarios"] != DAEMON_SCENARIOS
        or not isinstance(daemon["maximum_cleanup_ms"], int)
        or isinstance(daemon["maximum_cleanup_ms"], bool)
        or not 0 <= daemon["maximum_cleanup_ms"] <= 8_000
    ):
        fail("Linux daemon lifecycle facts are incomplete or outside the cleanup bound")
    identities = daemon["source_identities"]
    if not isinstance(identities, list) or len(identities) != 1:
        fail("Linux daemon lifecycle must bind one stable source identity")
    identity = exact_keys(
        identities[0],
        {
            "architecture", "genesis_executable_sha256", "probe_source_sha256",
            "selfhost_artifact_sha256",
        },
        "Linux daemon source identity",
    )
    if (
        identity["architecture"] != architecture
        or platform != "linux"
        or any(
            not is_sha256(identity[name])
            for name in (
                "genesis_executable_sha256", "probe_source_sha256",
                "selfhost_artifact_sha256",
            )
        )
    ):
        fail("Linux daemon source identity is invalid")
    return dict(daemon)


def validate_linux_report(doc: Any) -> dict[str, Any]:
    report = exact_keys(
        doc,
        {
            "budget_ms", "deterministic_replay_verified", "elapsed_ms", "failed_runs",
            "families", "hard_cancellation", "host_handle_lifecycle", "kind",
            "max_failure_rate_pct", "native_host", "observed_failure_rate_pct", "ok",
            "passed_runs", "runs", "runs_detail", "timestamp_unix_s",
        },
        "Linux host-fault report",
    )
    if (
        report["kind"] != LINUX_REPORT_KIND
        or report["ok"] is not True
        or report["runs"] != 3
        or report["passed_runs"] != 3
        or report["failed_runs"] != 0
        or report["max_failure_rate_pct"] != 0
        or report["observed_failure_rate_pct"] != 0
        or report["deterministic_replay_verified"] is not True
        or report["families"] != ["fs", "net", "process", "plugin"]
        or not isinstance(report["elapsed_ms"], int)
        or not isinstance(report["budget_ms"], int)
        or not 0 < report["elapsed_ms"] <= report["budget_ms"]
        or not isinstance(report["timestamp_unix_s"], int)
        or report["timestamp_unix_s"] <= 0
    ):
        fail("Linux host-fault report is unsuccessful, relabeled, or under-sampled")
    runs = report["runs_detail"]
    if not isinstance(runs, list) or len(runs) != 3:
        fail("Linux host-fault run detail is incomplete")
    for expected, raw in enumerate(runs, 1):
        row = exact_keys(raw, {"elapsed_ms", "ok", "run"}, "Linux host-fault run")
        if row["run"] != expected or row["ok"] is not True or not isinstance(row["elapsed_ms"], int) or row["elapsed_ms"] <= 0:
            fail("Linux host-fault run detail is invalid")

    native = exact_keys(
        report["native_host"],
        {
            "platform", "process_group_probe", "signal_reap_failure_control",
            "zombie_only_group_control",
        },
        "Linux native host",
    )
    if (
        native["platform"] != "linux"
        or not isinstance(native["process_group_probe"], str)
        or not native["process_group_probe"]
        or native["signal_reap_failure_control"] is not True
        or native["zombie_only_group_control"] is not True
    ):
        fail("Linux native process-group controls are incomplete")

    lifecycle = exact_keys(
        report["host_handle_lifecycle"],
        {
            "coverage_complete", "daemon_service_lifecycle",
            "independent_cross_host_evidence", "model_sessions", "r2_2_f_closeable",
            "runtime_owner_scope", "verified_controls",
        },
        "Linux host-handle lifecycle",
    )
    if (
        lifecycle["coverage_complete"] is not False
        or lifecycle["independent_cross_host_evidence"] is not False
        or lifecycle["r2_2_f_closeable"] is not False
        or lifecycle["runtime_owner_scope"] != "per-run"
        or lifecycle["verified_controls"] != VERIFIED_CONTROLS
    ):
        fail("Linux producer attempted closure or omitted a lifecycle control")
    model = exact_keys(
        lifecycle["model_sessions"],
        {"profile", "standard_model_api_owner", "status", "transport", "verified"},
        "Linux model session lifecycle",
    )
    if model != {
        "profile": "genesis.agent-model-runner.v0.1",
        "standard_model_api_owner": "R5.4.e",
        "status": "bridge-profile-implemented",
        "transport": "host/plugin::command",
        "verified": True,
    }:
        fail("Linux model session lifecycle facts drifted")

    hard = exact_keys(
        report["hard_cancellation"],
        {
            "child_reap", "io_worker_quiescence", "lifecycle_paths", "owner_scope",
            "process_global_session_cache", "process_group_quiescence",
            "process_tree_termination", "repeated_hang_cases", "resource_families",
            "transports", "uncertain_request_retry",
        },
        "Linux hard-cancellation facts",
    )
    if (
        hard["child_reap"] is not True
        or hard["io_worker_quiescence"] is not True
        or hard["process_tree_termination"] is not True
        or hard["process_group_quiescence"] != "no-live-members"
        or hard["process_global_session_cache"] is not False
        or hard["uncertain_request_retry"] is not False
        or hard["owner_scope"] != "runner"
        or hard["repeated_hang_cases"] != 49
        or hard["lifecycle_paths"] != LIFECYCLE_PATHS
        or hard["resource_families"] != RESOURCE_FAMILIES
        or hard["transports"] != ["persistent-stdio", "spawn-per-op"]
    ):
        fail("Linux hard-cancellation matrix is incomplete")
    daemon = validate_daemon_facts(
        lifecycle["daemon_service_lifecycle"], platform="linux", architecture="x86_64"
    )
    return {"daemon": daemon, "lifecycle": dict(lifecycle), "native": dict(native)}


def validate_macos_report(doc: Any) -> dict[str, Any]:
    report = exact_keys(
        doc,
        {
            "architecture", "cleanup", "daemon_processes", "descendant_processes",
            "elapsed_ms", "genesis_executable_sha256", "kind", "negative_controls",
            "ok", "platform", "probe_source_sha256", "provider_processes", "scenarios",
            "selfhost_artifact_sha256", "transport", "unique_provider_processes",
            "version", "warm_protocol",
        },
        "macOS daemon lifecycle report",
    )
    if (
        report["kind"] != MACOS_REPORT_KIND
        or report["version"] != "0.1"
        or report["ok"] is not True
        or report["platform"] != "darwin"
        or report["architecture"] != "arm64"
        or report["daemon_processes"] != 3
        or report["provider_processes"] != 5
        or report["unique_provider_processes"] != 5
        or report["descendant_processes"] != 5
        or report["scenarios"] != DAEMON_SCENARIOS
        or sorted(report["negative_controls"]) != NEGATIVE_CONTROLS
        or report["transport"] != "persistent-stdio"
        or report["warm_protocol"] != "genesis/warm-protocol-v0.2"
        or not isinstance(report["elapsed_ms"], int)
        or report["elapsed_ms"] <= 0
        or any(
            not is_sha256(report[name])
            for name in (
                "genesis_executable_sha256", "probe_source_sha256",
                "selfhost_artifact_sha256",
            )
        )
    ):
        fail("macOS daemon lifecycle report is incomplete or relabeled")
    cleanup = exact_keys(
        report["cleanup"],
        {"bound_ms", "maximum_ms", "no_live_provider_or_descendant", "samples"},
        "macOS daemon cleanup",
    )
    if (
        cleanup["bound_ms"] != 8_000
        or cleanup["no_live_provider_or_descendant"] is not True
        or not isinstance(cleanup["samples"], int)
        or cleanup["samples"] < 5
        or not isinstance(cleanup["maximum_ms"], int)
        or isinstance(cleanup["maximum_ms"], bool)
        or not 0 <= cleanup["maximum_ms"] <= cleanup["bound_ms"]
    ):
        fail("macOS daemon cleanup is incomplete or outside its bound")
    return dict(report)


def reconcile(
    linux_path: Path,
    macos_path: Path,
    *,
    linux_producer_sha256: str,
    macos_producer_sha256: str,
) -> dict[str, Any]:
    if not is_sha256(linux_producer_sha256) or not is_sha256(macos_producer_sha256):
        fail("lifecycle producer identities must be SHA-256 values")
    linux_doc = load_json(linux_path)
    macos_doc = load_json(macos_path)
    linux = validate_linux_report(linux_doc)
    macos = validate_macos_report(macos_doc)
    linux_source = linux["daemon"]["source_identities"][0]
    if (
        linux_source["probe_source_sha256"] != macos["probe_source_sha256"]
        or linux_source["selfhost_artifact_sha256"] != macos["selfhost_artifact_sha256"]
    ):
        fail("tier-1 lifecycle reports do not share probe and self-host identities")
    return {
        "coverageComplete": True,
        "custody": {
            "linuxProducerArtifact": LINUX_PRODUCER_ARTIFACT,
            "linuxProducerSha256": linux_producer_sha256,
            "macosProducerArtifact": MACOS_PRODUCER_ARTIFACT,
            "macosProducerSha256": macos_producer_sha256,
        },
        "independentCrossHostEvidence": True,
        "lifecyclePaths": LIFECYCLE_PATHS,
        "negativeControls": NEGATIVE_CONTROLS,
        "nonclaims": NONCLAIMS,
        "r2_2_f_closeable": True,
        "resourceFamilies": RESOURCE_FAMILIES,
        "sharedIdentities": {
            "probeSourceSha256": macos["probe_source_sha256"],
            "selfhostArtifactSha256": macos["selfhost_artifact_sha256"],
        },
        "status": "pass",
        "tier1Hosts": [
            {
                "architecture": "x86_64",
                "artifact": LINUX_ARTIFACT,
                "artifactSha256": sha256_file(linux_path),
                "maximumCleanupMs": linux["daemon"]["maximum_cleanup_ms"],
                "platform": "linux",
                "scope": "complete-host-fault-and-daemon-matrix",
            },
            {
                "architecture": "arm64",
                "artifact": MACOS_ARTIFACT,
                "artifactSha256": sha256_file(macos_path),
                "maximumCleanupMs": macos["cleanup"]["maximum_ms"],
                "platform": "darwin",
                "scope": "public-warm-daemon-lifecycle",
            },
        ],
        "verifiedControls": VERIFIED_CONTROLS,
    }


def validate_summary(summary: Any, artifact_root: Path) -> None:
    value = exact_keys(
        summary,
        {
            "coverageComplete", "custody", "independentCrossHostEvidence",
            "lifecyclePaths", "negativeControls", "nonclaims", "r2_2_f_closeable",
            "resourceFamilies", "sharedIdentities", "status", "tier1Hosts",
            "verifiedControls",
        },
        "host-handle lifecycle summary",
    )
    custody = exact_keys(
        value["custody"],
        {
            "linuxProducerArtifact", "linuxProducerSha256", "macosProducerArtifact",
            "macosProducerSha256",
        },
        "host-handle lifecycle custody",
    )
    expected = reconcile(
        artifact_root / LINUX_ARTIFACT,
        artifact_root / MACOS_ARTIFACT,
        linux_producer_sha256=custody["linuxProducerSha256"],
        macos_producer_sha256=custody["macosProducerSha256"],
    )
    if value != expected:
        fail("host-handle lifecycle summary is not derived from retained evidence")


def fixture_reports() -> tuple[dict[str, Any], dict[str, Any]]:
    source = {
        "architecture": "x86_64",
        "genesis_executable_sha256": "a" * 64,
        "probe_source_sha256": "b" * 64,
        "selfhost_artifact_sha256": "c" * 64,
    }
    daemon = {
        "fresh_daemon_process_isolation": True,
        "maximum_cleanup_ms": 40,
        "no_live_provider_or_descendant": True,
        "profile": "genesis/warm-protocol-v0.2",
        "runs": 3,
        "scenarios": DAEMON_SCENARIOS,
        "source_identities": [source],
        "verified": True,
    }
    linux = {
        "budget_ms": 300_000,
        "deterministic_replay_verified": True,
        "elapsed_ms": 100_000,
        "failed_runs": 0,
        "families": ["fs", "net", "process", "plugin"],
        "hard_cancellation": {
            "child_reap": True,
            "io_worker_quiescence": True,
            "lifecycle_paths": LIFECYCLE_PATHS,
            "owner_scope": "runner",
            "process_global_session_cache": False,
            "process_group_quiescence": "no-live-members",
            "process_tree_termination": True,
            "repeated_hang_cases": 49,
            "resource_families": RESOURCE_FAMILIES,
            "transports": ["persistent-stdio", "spawn-per-op"],
            "uncertain_request_retry": False,
        },
        "host_handle_lifecycle": {
            "coverage_complete": False,
            "daemon_service_lifecycle": daemon,
            "independent_cross_host_evidence": False,
            "model_sessions": {
                "profile": "genesis.agent-model-runner.v0.1",
                "standard_model_api_owner": "R5.4.e",
                "status": "bridge-profile-implemented",
                "transport": "host/plugin::command",
                "verified": True,
            },
            "r2_2_f_closeable": False,
            "runtime_owner_scope": "per-run",
            "verified_controls": VERIFIED_CONTROLS,
        },
        "kind": LINUX_REPORT_KIND,
        "max_failure_rate_pct": 0,
        "native_host": {
            "platform": "linux",
            "process_group_probe": "proc-pgrp-status",
            "signal_reap_failure_control": True,
            "zombie_only_group_control": True,
        },
        "observed_failure_rate_pct": 0,
        "ok": True,
        "passed_runs": 3,
        "runs": 3,
        "runs_detail": [
            {"elapsed_ms": 30_000 + index, "ok": True, "run": index}
            for index in range(1, 4)
        ],
        "timestamp_unix_s": 1,
    }
    macos = {
        "architecture": "arm64",
        "cleanup": {
            "bound_ms": 8_000,
            "maximum_ms": 35,
            "no_live_provider_or_descendant": True,
            "samples": 5,
        },
        "daemon_processes": 3,
        "descendant_processes": 5,
        "elapsed_ms": 2_000,
        "genesis_executable_sha256": "d" * 64,
        "kind": MACOS_REPORT_KIND,
        "negative_controls": NEGATIVE_CONTROLS,
        "ok": True,
        "platform": "darwin",
        "probe_source_sha256": source["probe_source_sha256"],
        "provider_processes": 5,
        "scenarios": DAEMON_SCENARIOS,
        "selfhost_artifact_sha256": source["selfhost_artifact_sha256"],
        "transport": "persistent-stdio",
        "unique_provider_processes": 5,
        "version": "0.1",
        "warm_protocol": "genesis/warm-protocol-v0.2",
    }
    return linux, macos


def self_test() -> int:
    controls = 0
    linux, macos = fixture_reports()
    with tempfile.TemporaryDirectory(prefix="genesis-host-lifecycle-evidence-") as raw:
        root = Path(raw)
        linux_path = root / LINUX_ARTIFACT
        macos_path = root / MACOS_ARTIFACT

        def write_reports(left: Mapping[str, Any], right: Mapping[str, Any]) -> None:
            linux_path.parent.mkdir(parents=True, exist_ok=True)
            macos_path.parent.mkdir(parents=True, exist_ok=True)
            linux_path.write_text(json.dumps(left, sort_keys=True) + "\n", encoding="utf-8")
            macos_path.write_text(json.dumps(right, sort_keys=True) + "\n", encoding="utf-8")

        write_reports(linux, macos)
        baseline = reconcile(
            linux_path,
            macos_path,
            linux_producer_sha256="e" * 64,
            macos_producer_sha256="f" * 64,
        )
        validate_summary(baseline, root)

        mutations = [
            lambda left, right: left["host_handle_lifecycle"].__setitem__("coverage_complete", True),
            lambda left, right: left["hard_cancellation"].__setitem__("child_reap", False),
            lambda left, right: left["hard_cancellation"]["lifecycle_paths"].pop(),
            lambda left, right: left["hard_cancellation"]["resource_families"].pop(),
            lambda left, right: left["host_handle_lifecycle"]["verified_controls"].pop(),
            lambda left, right: left["native_host"].__setitem__("platform", "darwin"),
            lambda left, right: left["host_handle_lifecycle"]["daemon_service_lifecycle"].__setitem__("runs", 2),
            lambda left, right: right.__setitem__("architecture", "x86_64"),
            lambda left, right: right["cleanup"].__setitem__("no_live_provider_or_descendant", False),
            lambda left, right: right["scenarios"].pop(),
            lambda left, right: right["negative_controls"].pop(),
            lambda left, right: right.__setitem__("probe_source_sha256", "0" * 64),
        ]
        for mutate in mutations:
            candidate_linux = copy.deepcopy(linux)
            candidate_macos = copy.deepcopy(macos)
            mutate(candidate_linux, candidate_macos)
            write_reports(candidate_linux, candidate_macos)
            try:
                reconcile(
                    linux_path,
                    macos_path,
                    linux_producer_sha256="e" * 64,
                    macos_producer_sha256="f" * 64,
                )
            except LifecycleEvidenceError:
                controls += 1
            else:
                fail(f"lifecycle evidence self-test accepted mutation {controls + 1}")

        write_reports(linux, macos)
        tampered = copy.deepcopy(baseline)
        tampered["tier1Hosts"][1]["maximumCleanupMs"] += 1
        try:
            validate_summary(tampered, root)
        except LifecycleEvidenceError:
            controls += 1
        else:
            fail("lifecycle evidence self-test accepted a forged aggregate summary")
    if controls != 13:
        fail(f"lifecycle evidence negative-control inventory drifted: {controls}")
    return controls


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--self-test", action="store_true")
    args = parser.parse_args()
    if not args.self_test:
        parser.error("--self-test is required")
    try:
        controls = self_test()
    except LifecycleEvidenceError as exc:
        print(f"host-handle-lifecycle-evidence: {exc}", file=sys.stderr)
        return 1
    print(f"host-handle-lifecycle-evidence: self-test ok (negative_controls={controls})")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
