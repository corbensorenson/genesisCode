#!/usr/bin/env python3
"""Validate, stage, and transactionally publish generated authorities."""

from __future__ import annotations

import argparse
import copy
import fnmatch
from hashlib import sha256
import json
import os
from pathlib import Path, PurePosixPath
import shutil
import signal
import stat
import subprocess
import sys
import tempfile
import time
from typing import Any, Dict, Iterable, Mapping, Optional, Sequence


ROOT = Path(__file__).resolve().parents[2]
POLICY_REL = "policies/check_update_boundary_v0.1.json"
SCHEMA_REL = "docs/spec/GENERATED_AUTHORITY_GRAPH_v0.1.schema.json"
GATES_REL = "genesis.gates.json"
AUDIT_REL = "docs/spec/CHECK_UPDATE_BOUNDARY_AUDIT_v0.1.json"
LOCK_NAME = "genesis-generated-authority.lock"
STAGE_SCOPED_ENVIRONMENT = (
    "CARGO_TARGET_DIR",
    "GENESIS_CARGO_CACHE_ROOT",
    "GENESIS_CARGO_CACHE_RESOLVED",
    "GENESIS_CARGO_CACHE_SCOPE",
    "GENESIS_CARGO_CACHE_KEY_SHA256",
    "GENESIS_CARGO_CACHE_HIT",
    "GENESIS_CARGO_CACHE_RUSTC_IDENTITY_JSON",
    "GENESIS_GENERATED_STATE_ROOT",
    "GENESIS_GENERATED_STATE_LEASE_PID",
    "GENESIS_GENERATED_STATE_LEASE_TOKEN",
    "GENESIS_GATE_BUDGET_ENFORCE",
    "GENESIS_GATE_AGGREGATE_OWNER_FD",
    "GENESIS_GATE_TELEMETRY_EVENT_FILE",
    "GENESIS_SELFHOST_TOOLCHAIN_ARTIFACT",
    "GENESIS_SELFHOST_TOOLCHAIN_FRESHNESS",
    "GENESIS_SELFHOST_TOOLCHAIN_MANIFEST",
)
STAGE_BUILD_ENVIRONMENT = {
    "CARGO_INCREMENTAL": "0",
    "CARGO_PROFILE_DEV_DEBUG": "0",
    "CARGO_PROFILE_TEST_DEBUG": "0",
}
CHECK_WORKERS = 4
# Compilation checks share one content-addressed Cargo target. Keep them
# single-writer so independent Cargo processes cannot race generated outputs.
COMPILATION_CHECK_WORKERS = 1
SHA_RE_LENGTH = 64
NODE_FIELDS = {
    "id", "command", "dependencies", "inputs", "outputs", "checks", "mode",
    "timeoutSeconds", "diskMiB",
}
GRAPH_FIELDS = {
    "kind", "version", "schema", "orchestratorEntrypoint", "limits",
    "identityExclusions", "protectedOutputs", "stagingTemporaryWrites",
    "excludedEntrypoints", "nodes", "mutationControls",
}
LIMIT_FIELDS = {"maxNodes", "maxOutputs", "maxTimeoutSeconds", "maxDiskMiB"}
MUTATION_FIELDS = {"path", "expectedNodes", "expectedOutputs"}
MODES = {"automatic", "operator-gated"}
FORBIDDEN_COMMAND_PARTS = ("sign", "attest", "keygen", "release-assets/evidence")
REQUIRED_IDENTITY_EXCLUSIONS = {
    "CHANGELOG.md",
    "ROADMAP.md",
    "docs/program/ROADMAP_EXECUTION_MANIFEST_v0.1.json",
    "genesis.gates.json",
    "llms.txt",
}


class AuthorityError(ValueError):
    pass


def reject_duplicate_keys(pairs: Sequence[tuple[str, Any]]) -> Dict[str, Any]:
    result: Dict[str, Any] = {}
    for key, value in pairs:
        if key in result:
            raise AuthorityError(f"duplicate JSON key: {key}")
        result[key] = value
    return result


def load_json(path: Path) -> Any:
    try:
        return json.loads(
            path.read_text(encoding="utf-8"), object_pairs_hook=reject_duplicate_keys
        )
    except FileNotFoundError as exc:
        raise AuthorityError(f"missing file: {display(path)}") from exc
    except json.JSONDecodeError as exc:
        raise AuthorityError(
            f"invalid JSON in {display(path)}:{exc.lineno}:{exc.colno}: {exc.msg}"
        ) from exc


def display(path: Path) -> str:
    try:
        return path.resolve().relative_to(ROOT.resolve()).as_posix()
    except ValueError:
        return path.as_posix()


def require(value: bool, message: str) -> None:
    if not value:
        raise AuthorityError(message)


def require_closed(value: Any, fields: set[str], label: str) -> Mapping[str, Any]:
    require(isinstance(value, dict), f"{label} must be an object")
    actual = set(value)
    require(actual == fields, f"{label} fields drift: missing={sorted(fields-actual)} extra={sorted(actual-fields)}")
    return value


def canonical_path(value: Any, label: str, *, allow_glob: bool = False) -> str:
    require(isinstance(value, str) and value, f"{label} must be a non-empty string")
    require("\\" not in value and not value.startswith("/"), f"{label} must be repository-relative")
    require(not (len(value) > 1 and value[1] == ":"), f"{label} must not be a host path")
    parts = PurePosixPath(value).parts
    require(parts and all(part not in ("", ".", "..") for part in parts), f"{label} is not canonical")
    if not allow_glob:
        require(not any(character in value for character in "*?["), f"{label} must be exact")
    require(not value.startswith(".git/") and value != ".git", f"{label} enters Git control state")
    return value


def string_list(value: Any, label: str, *, nonempty: bool = True) -> list[str]:
    require(isinstance(value, list), f"{label} must be an array")
    require(not nonempty or bool(value), f"{label} must not be empty")
    require(all(isinstance(item, str) and item for item in value), f"{label} must contain strings")
    require(len(value) == len(set(value)), f"{label} contains duplicates")
    return list(value)


def graph_from_policy(policy: Mapping[str, Any]) -> Mapping[str, Any]:
    graph = policy.get("generated_authority")
    return require_closed(graph, GRAPH_FIELDS, "policy.generated_authority")


def gate_inputs_by_check(root: Path) -> dict[str, tuple[str, ...]]:
    manifest = load_json(root / GATES_REL)
    require(isinstance(manifest.get("gates"), list), "gate manifest has no gate inventory")
    result: dict[str, tuple[str, ...]] = {}
    for index, gate in enumerate(manifest["gates"]):
        require(isinstance(gate, dict), f"gate manifest entry {index} is not an object")
        entrypoint = canonical_path(gate.get("entrypoint"), f"gate manifest entry {index}.entrypoint")
        inputs = gate.get("inputs")
        require(isinstance(inputs, dict), f"{entrypoint} gate inputs are missing")
        paths = string_list(inputs.get("paths"), f"{entrypoint}.inputs.paths")
        for path_index, path in enumerate(paths):
            canonical_path(path, f"{entrypoint}.inputs.paths[{path_index}]")
        require(entrypoint not in result, f"duplicate gate manifest entrypoint: {entrypoint}")
        result[entrypoint] = tuple(paths)
    return result


def effective_inputs(
    node: Mapping[str, Any], gate_inputs: Mapping[str, Sequence[str]]
) -> tuple[str, ...]:
    discovered = set(node["inputs"])
    if node["mode"] == "automatic":
        for check in node["checks"]:
            discovered.update(gate_inputs.get(check, ()))
    return tuple(sorted(discovered))


def validate_schema(root: Path, graph: Mapping[str, Any]) -> None:
    schema_path = root / canonical_path(graph["schema"], "generated_authority.schema")
    schema = load_json(schema_path)
    require(schema.get("$schema") == "https://json-schema.org/draft/2020-12/schema", "generated-authority schema draft drift")
    require(schema.get("$id") == "https://genesiscode.dev/schemas/generated-authority-graph-v0.1.json", "generated-authority schema identity drift")
    require(schema.get("additionalProperties") is False, "generated-authority schema must be closed")
    require(set(schema.get("required", [])) == GRAPH_FIELDS, "generated-authority schema required fields drift")


def update_inventory(root: Path) -> set[str]:
    return {
        path.relative_to(root).as_posix()
        for path in (root / "scripts").glob("update_*.sh")
        if path.is_file()
    }


def validate_graph(root: Path, graph: Mapping[str, Any]) -> list[Mapping[str, Any]]:
    require(graph["kind"] == "genesis/generated-authority-graph-v0.1", "generated-authority kind drift")
    require(graph["version"] == "0.1", "generated-authority version drift")
    validate_schema(root, graph)
    orchestrator = canonical_path(graph["orchestratorEntrypoint"], "generated_authority.orchestratorEntrypoint")
    require((root / orchestrator).is_file(), "generated-authority orchestrator is missing")

    limits = require_closed(graph["limits"], LIMIT_FIELDS, "generated_authority.limits")
    for field in LIMIT_FIELDS:
        require(isinstance(limits[field], int) and limits[field] > 0, f"generated_authority.limits.{field} must be positive")

    exclusions = graph["identityExclusions"]
    require(isinstance(exclusions, dict) and exclusions, "identityExclusions must be a non-empty object")
    for path, reason in exclusions.items():
        canonical_path(path, f"identityExclusions[{path}]")
        require(isinstance(reason, str) and reason.strip(), f"identityExclusions[{path}] requires a reason")
    require(
        set(exclusions) == REQUIRED_IDENTITY_EXCLUSIONS,
        "generated-authority fixed-point exclusion inventory drift",
    )

    protected = string_list(graph["protectedOutputs"], "protectedOutputs")
    for index, path in enumerate(protected):
        canonical_path(path, f"protectedOutputs[{index}]", allow_glob=True)
        require("E3" in path or "E4" in path or "evidence" in path.lower(), f"protected output lacks evidence class: {path}")

    temporary_writes = string_list(
        graph["stagingTemporaryWrites"], "stagingTemporaryWrites"
    )
    for index, path in enumerate(temporary_writes):
        canonical_path(path, f"stagingTemporaryWrites[{index}]", allow_glob=True)
    require(
        temporary_writes == [".genesis/**"],
        "staging temporary writes must remain confined to .genesis/**",
    )

    excluded = graph["excludedEntrypoints"]
    require(isinstance(excluded, dict), "excludedEntrypoints must be an object")
    for path, reason in excluded.items():
        canonical_path(path, f"excludedEntrypoints[{path}]")
        require(isinstance(reason, str) and reason.strip(), f"excludedEntrypoints[{path}] requires a reason")

    nodes_value = graph["nodes"]
    require(isinstance(nodes_value, list) and nodes_value, "generated_authority.nodes must be non-empty")
    require(len(nodes_value) <= limits["maxNodes"], "generated-authority node limit exceeded")
    nodes: list[Mapping[str, Any]] = []
    by_id: dict[str, Mapping[str, Any]] = {}
    owner: dict[str, str] = {}
    update_commands: set[str] = set()
    total_outputs = 0
    gate_inputs = gate_inputs_by_check(root)
    gate_checks = set(gate_inputs)
    audit = load_json(root / AUDIT_REL)
    audited_checks = {entry["path"] for entry in audit["entries"]}
    require(gate_checks == audited_checks, "gate manifest and check/update audit inventories diverge")

    for index, raw in enumerate(nodes_value):
        node = require_closed(raw, NODE_FIELDS, f"nodes[{index}]")
        node_id = node["id"]
        require(isinstance(node_id, str) and node_id.startswith("generate/") and node_id[9:], f"nodes[{index}].id is invalid")
        require(node_id not in by_id, f"duplicate generated-authority node: {node_id}")
        command = string_list(node["command"], f"{node_id}.command")
        require(all("\n" not in part and "\x00" not in part for part in command), f"{node_id}.command contains control bytes")
        joined = " ".join(command).lower()
        require(not any(part in joined for part in FORBIDDEN_COMMAND_PARTS), f"{node_id} invokes signing, attestation, or retained-evidence publication")
        if len(command) >= 2 and command[0] == "bash" and command[1].startswith("scripts/update_"):
            entrypoint = canonical_path(command[1], f"{node_id}.command[1]")
            require((root / entrypoint).is_file(), f"{node_id} updater is missing: {entrypoint}")
            update_commands.add(entrypoint)
        elif command[0].startswith("internal:"):
            require(command == ["internal:roadmap-evidence"], f"{node_id} has unknown internal action")
        else:
            executable = command[0]
            require(executable in {"python3", "bash"}, f"{node_id} command executable is not admitted")
            if len(command) >= 2 and command[1].startswith("scripts/"):
                canonical_path(command[1], f"{node_id}.command[1]")
                require((root / command[1]).is_file(), f"{node_id} command source is missing")
        dependencies = string_list(node["dependencies"], f"{node_id}.dependencies", nonempty=False)
        inputs = string_list(node["inputs"], f"{node_id}.inputs")
        for input_index, path in enumerate(inputs):
            canonical_path(path, f"{node_id}.inputs[{input_index}]", allow_glob=True)
        if len(command) >= 2 and command[1].startswith("scripts/"):
            require(
                matches(command[1], inputs),
                f"{node_id} command source is not in its freshness read set: {command[1]}",
            )
        outputs = string_list(node["outputs"], f"{node_id}.outputs")
        total_outputs += len(outputs)
        for output_index, path in enumerate(outputs):
            exact = canonical_path(path, f"{node_id}.outputs[{output_index}]")
            require(
                matches(exact, inputs),
                f"{node_id} output is not in its own freshness read set: {exact}",
            )
            require(not any(fnmatch.fnmatchcase(exact, pattern) for pattern in protected), f"{node_id} owns protected evidence output {exact}")
            previous = owner.setdefault(exact, node_id)
            require(previous == node_id, f"generated output has multiple owners: {exact}: {previous}, {node_id}")
        checks = string_list(node["checks"], f"{node_id}.checks")
        for check_index, check in enumerate(checks):
            check = canonical_path(check, f"{node_id}.checks[{check_index}]")
            require(check in gate_checks and check in audited_checks, f"{node_id} check is not in both discovery authorities: {check}")
        require(node["mode"] in MODES, f"{node_id}.mode is invalid")
        require(isinstance(node["timeoutSeconds"], int) and 1 <= node["timeoutSeconds"] <= limits["maxTimeoutSeconds"], f"{node_id}.timeoutSeconds exceeds policy")
        require(isinstance(node["diskMiB"], int) and 0 <= node["diskMiB"] <= limits["maxDiskMiB"], f"{node_id}.diskMiB exceeds policy")
        by_id[node_id] = node
        nodes.append(node)
    require(total_outputs <= limits["maxOutputs"], "generated-authority output limit exceeded")

    for node in nodes:
        for dependency in node["dependencies"]:
            require(dependency in by_id, f"{node['id']} has unknown dependency: {dependency}")
            require(dependency != node["id"], f"{node['id']} depends on itself")
    topological(nodes)

    ancestor_cache: dict[str, set[str]] = {}

    def ancestors(node_id: str) -> set[str]:
        cached = ancestor_cache.get(node_id)
        if cached is not None:
            return cached
        result: set[str] = set()
        pending = list(by_id[node_id]["dependencies"])
        while pending:
            dependency = pending.pop()
            if dependency in result:
                continue
            result.add(dependency)
            pending.extend(by_id[dependency]["dependencies"])
        ancestor_cache[node_id] = result
        return result

    for consumer in nodes:
        ordered_before = ancestors(consumer["id"])
        for producer in nodes:
            if producer["id"] == consumer["id"]:
                continue
            reads_producer = any(
                matches(output, consumer["inputs"])
                for output in producer["outputs"]
            )
            require(
                not reads_producer or producer["id"] in ordered_before,
                f"{consumer['id']} reads {producer['id']} outputs without depending on it",
            )

    inventory = update_inventory(root)
    classified = update_commands | set(excluded) | {orchestrator}
    require(inventory == classified, f"updater inventory classification drift: missing={sorted(inventory-classified)} stale={sorted(classified-inventory)}")
    require(not (update_commands & set(excluded)), "an updater is both graph-owned and excluded")

    controls = graph["mutationControls"]
    require(isinstance(controls, list) and controls, "mutationControls must be non-empty")
    seen_paths: set[str] = set()
    for index, raw in enumerate(controls):
        control = require_closed(raw, MUTATION_FIELDS, f"mutationControls[{index}]")
        path = canonical_path(control["path"], f"mutationControls[{index}].path")
        require(path not in seen_paths, f"duplicate mutation control path: {path}")
        seen_paths.add(path)
        expected_nodes = string_list(control["expectedNodes"], f"mutationControls[{index}].expectedNodes")
        expected_outputs = string_list(control["expectedOutputs"], f"mutationControls[{index}].expectedOutputs")
        selected = closure_for_paths(
            nodes, [path], include_operator=True, gate_inputs=gate_inputs
        )
        require(expected_nodes == [node["id"] for node in selected], f"mutation route drift for {path}")
        actual_outputs = sorted({output for node in selected for output in node["outputs"]})
        require(expected_outputs == actual_outputs, f"mutation output route drift for {path}")
    required_controls = {
        "Cargo.lock", "docs/spec/CLI_JSON_SCHEMAS_v0.1.md", "ROADMAP.md",
        "policies/gc_agent_profile_v0.3.json",
        "policies/gc_diagnostic_catalog_v0.1.json",
        "benchmarks/agent_tasks/v0.1/suite.json",
        "crates/gc_cli/tests/cli_genesisbench_front_door.rs",
        "scripts/lib/genesisbench_mlx_responses.py",
        "scripts/check_agent_authoring_bundle.sh",
        "docs/spec/GENESISBENCH_MLX_CUSTODY_v0.1.schema.json",
    }
    require(required_controls <= seen_paths, f"required mutation controls missing: {sorted(required_controls-seen_paths)}")
    return nodes


def topological(nodes: Sequence[Mapping[str, Any]]) -> list[Mapping[str, Any]]:
    by_id = {node["id"]: node for node in nodes}
    pending = {node["id"]: set(node["dependencies"]) for node in nodes}
    ordered: list[Mapping[str, Any]] = []
    while pending:
        ready = sorted(node_id for node_id, deps in pending.items() if not deps)
        require(bool(ready), f"generated-authority graph contains a cycle: {sorted(pending)}")
        for node_id in ready:
            ordered.append(by_id[node_id])
            del pending[node_id]
        for deps in pending.values():
            deps.difference_update(ready)
    return ordered


def matches(path: str, patterns: Iterable[str]) -> bool:
    return any(fnmatch.fnmatchcase(path, pattern) for pattern in patterns)


def closure_for_paths(
    nodes: Sequence[Mapping[str, Any]],
    paths: Sequence[str],
    *,
    include_operator: bool,
    gate_inputs: Optional[Mapping[str, Sequence[str]]] = None,
) -> list[Mapping[str, Any]]:
    discovered = gate_inputs or {}
    selected = {
        node["id"]
        for node in nodes
        if any(matches(path, effective_inputs(node, discovered)) for path in paths)
    }
    changed = True
    while changed:
        before = len(selected)
        for node in nodes:
            if set(node["dependencies"]) & selected:
                selected.add(node["id"])
        changed = len(selected) != before
    ordered = [node for node in topological(nodes) if node["id"] in selected]
    if not include_operator:
        blocked = [node["id"] for node in ordered if node["mode"] == "operator-gated"]
        require(not blocked, "generated closure reaches operator-gated nodes: " + ", ".join(blocked))
    return ordered


def git(root: Path, *args: str, capture: bool = True) -> str:
    result = subprocess.run(["git", *args], cwd=root, check=True, text=True, stdout=subprocess.PIPE if capture else None, stderr=subprocess.PIPE if capture else None)
    return result.stdout if capture else ""


def changed_paths(root: Path, base: Optional[str] = None) -> list[str]:
    values: set[str] = set()
    if base:
        subprocess.run(
            ["git", "rev-parse", "--verify", base], cwd=root, check=True,
            stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL,
        )
        values.update(
            line for line in git(root, "diff", "--name-only", f"{base}...HEAD").splitlines()
            if line
        )
    for args in (("diff", "--name-only", "HEAD"), ("ls-files", "--others", "--exclude-standard")):
        values.update(line for line in git(root, *args).splitlines() if line)
    return sorted(values)


def copy_overlay(root: Path, stage: Path) -> None:
    patch = subprocess.run(["git", "diff", "--binary", "HEAD"], cwd=root, check=True, stdout=subprocess.PIPE).stdout
    if patch:
        subprocess.run(["git", "apply", "--binary", "-"], cwd=stage, input=patch, check=True)
    for rel in git(root, "ls-files", "--others", "--exclude-standard").splitlines():
        source = root / rel
        destination = stage / rel
        destination.parent.mkdir(parents=True, exist_ok=True)
        if source.is_symlink():
            destination.symlink_to(os.readlink(source))
        elif source.is_file():
            shutil.copy2(source, destination)


def worktree_changes(stage: Path) -> set[str]:
    tracked = set(git(stage, "diff", "--name-only", "HEAD").splitlines())
    untracked = set(git(stage, "ls-files", "--others", "--exclude-standard").splitlines())
    return {path for path in tracked | untracked if path}


def repository_mode(path: Path) -> int:
    """Return the mode bits that Git can reproduce in a fresh checkout."""
    metadata = path.lstat()
    if stat.S_ISLNK(metadata.st_mode):
        return 0o120000
    require(stat.S_ISREG(metadata.st_mode), f"repository identity requires a file: {path}")
    return 0o100755 if metadata.st_mode & stat.S_IXUSR else 0o100644


def file_identity(path: Path) -> str:
    mode = repository_mode(path)
    if path.is_symlink():
        payload = f"link:{mode:o}\0".encode("ascii") + os.readlink(path).encode("utf-8")
    else:
        payload = f"file:{mode:o}\0".encode("ascii") + path.read_bytes()
    return sha256(payload).hexdigest()


def changed_content_snapshot(root: Path) -> Mapping[str, str]:
    # Git identifies every observable repository write. Hashing only that
    # frontier preserves detection of clean, dirty, untracked, and deleted-path
    # mutations without rereading the complete repository around every node.
    result: dict[str, str] = {}
    for rel in sorted(
        path for path in worktree_changes(root)
        if path and not path.startswith(".genesis/")
    ):
        path = root / rel
        if not path.is_symlink() and not path.is_file():
            result[rel] = "missing"
        else:
            result[rel] = file_identity(path)
    return result


def refresh_roadmap_evidence(stage: Path) -> None:
    output = subprocess.check_output(["python3", "scripts/lib/roadmap_evidence.py", "--print"], cwd=stage, text=True)
    identities = dict(line.strip().rsplit("-sha256:", 1) for line in output.splitlines() if line.strip())
    roadmap = stage / "ROADMAP.md"
    text = roadmap.read_text(encoding="utf-8")
    import re
    for name, digest in identities.items():
        text, count = re.subn(rf"{re.escape(name)}-sha256:[0-9a-f]{{64}}", f"{name}-sha256:{digest}", text)
        require(count > 0, f"roadmap evidence identity has no citation: {name}")
    roadmap.write_text(text, encoding="utf-8")


def filesystem_free_bytes(path: Path) -> int:
    if hasattr(os, "statvfs"):
        values = os.statvfs(path)
        return int(values.f_bavail) * int(values.f_frsize)
    return int(shutil.disk_usage(path).free)


def allocated_tree_bytes(path: Path) -> int:
    total = 0
    seen: set[tuple[int, int]] = set()
    stack = [path]
    while stack:
        current = stack.pop()
        try:
            metadata = current.lstat()
        except OSError:
            continue
        identity = (metadata.st_dev, metadata.st_ino)
        if identity in seen:
            continue
        seen.add(identity)
        total += max(0, int(getattr(metadata, "st_blocks", 0))) * 512
        if stat.S_ISDIR(metadata.st_mode) and not stat.S_ISLNK(metadata.st_mode):
            try:
                with os.scandir(current) as entries:
                    stack.extend(Path(entry.path) for entry in entries)
            except OSError:
                continue
    return total


def kill_and_reap(process: subprocess.Popen[Any]) -> None:
    if process.poll() is None:
        try:
            if os.name != "nt":
                os.killpg(process.pid, signal.SIGKILL)
            else:
                process.kill()
        except ProcessLookupError:
            pass
    process.wait()


class AggregateResourceOwner:
    """Own complete-transaction wall, disk, event, and child cancellation limits."""

    def __init__(self, root: Path, event_root: Path, limits: Mapping[str, Any]):
        self.root = root
        self.started = time.monotonic()
        self.timeout_seconds = int(limits["maxTimeoutSeconds"])
        self.disk_limit_bytes = int(limits["maxDiskMiB"]) * 1024 * 1024
        self.free_baseline = filesystem_free_bytes(root)
        self.event_path = event_root / (
            f"aggregate-resource-events-{os.getpid()}-{time.monotonic_ns()}.jsonl"
        )
        self.event_fd: Optional[int] = None
        self.event_offset = 0
        self.event_fragment = b""
        self.event_count = 0
        self.network_attempts = 0
        if os.name != "nt":
            self.event_fd = os.open(
                self.event_path,
                os.O_CREAT | os.O_EXCL | os.O_WRONLY | os.O_APPEND,
                0o600,
            )
            os.set_inheritable(self.event_fd, True)

    def child_environment(
        self, environment: Mapping[str, str]
    ) -> tuple[dict[str, str], tuple[int, ...]]:
        result = dict(environment)
        result.pop("GENESIS_GATE_BUDGET_ENFORCE", None)
        if self.event_fd is None:
            result.pop("GENESIS_GATE_AGGREGATE_OWNER_FD", None)
            result.pop("GENESIS_GATE_TELEMETRY_EVENT_FILE", None)
            return result, ()
        result["GENESIS_GATE_AGGREGATE_OWNER_FD"] = str(self.event_fd)
        result["GENESIS_GATE_TELEMETRY_EVENT_FILE"] = str(self.event_path)
        return result, (self.event_fd,)

    def consume_events(self, *, final: bool = False) -> None:
        if self.event_fd is None:
            return
        with self.event_path.open("rb") as handle:
            handle.seek(self.event_offset)
            chunk = handle.read()
        self.event_offset += len(chunk)
        data = self.event_fragment + chunk
        lines = data.split(b"\n")
        self.event_fragment = lines.pop()
        for raw in lines:
            require(bool(raw) and len(raw) <= 128, "aggregate resource event line is invalid")
            try:
                event = json.loads(
                    raw.decode("ascii"), object_pairs_hook=reject_duplicate_keys
                )
            except (UnicodeError, json.JSONDecodeError) as exc:
                raise AuthorityError("aggregate resource event is malformed") from exc
            require(
                isinstance(event, dict) and set(event) == {"count", "kind"},
                "aggregate resource event fields drift",
            )
            count = event["count"]
            kind = event["kind"]
            require(
                kind in {"cache-hit", "network-attempt"}
                and isinstance(count, int)
                and not isinstance(count, bool)
                and count > 0,
                "aggregate resource event value is invalid",
            )
            self.event_count += count
            require(self.event_count <= 1_000_000, "aggregate resource event count exceeded")
            if kind == "network-attempt":
                self.network_attempts += count
        if final:
            require(not self.event_fragment, "aggregate resource event channel ended mid-record")
        require(
            self.network_attempts == 0,
            f"aggregate resource owner rejected network attempts: {self.network_attempts}",
        )

    def check(
        self,
        label: str,
        *,
        scope_allocated_bytes: Optional[int] = None,
        scope_root: Optional[Path] = None,
        scope_disk_mib: Optional[int] = None,
    ) -> None:
        elapsed = time.monotonic() - self.started
        require(
            elapsed <= self.timeout_seconds,
            f"aggregate resource owner exceeded wall limit during {label}: "
            f"{elapsed:.3f}s>{self.timeout_seconds}s",
        )
        free_now = filesystem_free_bytes(self.root)
        aggregate_delta = max(0, self.free_baseline - free_now)
        require(
            aggregate_delta <= self.disk_limit_bytes,
            f"aggregate resource owner exceeded disk limit during {label}: "
            f"{aggregate_delta}B>{self.disk_limit_bytes}B",
        )
        if (
            scope_allocated_bytes is not None
            and scope_root is not None
            and scope_disk_mib is not None
        ):
            scope_delta = max(0, allocated_tree_bytes(scope_root) - scope_allocated_bytes)
            scope_limit = int(scope_disk_mib) * 1024 * 1024
            require(
                scope_delta <= scope_limit,
                f"generated node exceeded disk limit during {label}: "
                f"{scope_delta}B>{scope_limit}B",
            )
        self.consume_events()

    def close(self, *, validate: bool = True) -> None:
        try:
            if validate:
                self.check("transaction cleanup")
                self.consume_events(final=True)
        finally:
            if self.event_fd is not None:
                os.close(self.event_fd)
                self.event_fd = None


def run_bounded(
    command: Sequence[str], *, cwd: Path, timeout: int,
    environment: Optional[Mapping[str, str]] = None,
    owner: Optional[AggregateResourceOwner] = None,
    disk_mib: Optional[int] = None,
) -> None:
    scope_allocated = allocated_tree_bytes(cwd) if disk_mib is not None else None
    process_environment = dict(environment or os.environ)
    pass_fds: tuple[int, ...] = ()
    if owner is not None:
        process_environment, pass_fds = owner.child_environment(process_environment)
    process = subprocess.Popen(
        list(command), cwd=cwd, env=process_environment,
        start_new_session=(os.name != "nt"),
        **({"pass_fds": pass_fds} if pass_fds else {}),
    )
    try:
        started = time.monotonic()
        while process.poll() is None:
            if time.monotonic() - started > timeout:
                raise subprocess.TimeoutExpired(list(command), timeout)
            if owner is not None:
                owner.check("child execution")
            time.sleep(0.05)
        return_code = process.returncode
        if owner is not None:
            owner.check(
                "child completion",
                scope_allocated_bytes=scope_allocated,
                scope_root=cwd,
                scope_disk_mib=disk_mib,
            )
    except BaseException:
        kill_and_reap(process)
        raise
    if return_code != 0:
        raise subprocess.CalledProcessError(return_code, list(command))


def stage_environment(marker: str) -> dict[str, str]:
    environment = os.environ.copy()
    for name in STAGE_SCOPED_ENVIRONMENT:
        environment.pop(name, None)
    # Staging worktrees are disposable, so incremental state and debug symbols
    # only add cold-build time and disk without contributing to verification.
    environment.update(STAGE_BUILD_ENVIRONMENT)
    environment[marker] = "1"
    return environment


def validation_environment() -> dict[str, str]:
    return stage_environment("GENESIS_GENERATED_AUTHORITY_VALIDATING")


def next_check_position(
    pending: Sequence[tuple[int, str, int, bool]],
    active_compilation_lanes: Sequence[bool],
) -> Optional[int]:
    if not pending:
        return None
    lanes = set(active_compilation_lanes)
    require(len(lanes) <= 1, "generated checks mixed static and compilation lanes")
    lane = next(iter(lanes)) if lanes else pending[0][3]
    limit = COMPILATION_CHECK_WORKERS if lane else CHECK_WORKERS
    if len(active_compilation_lanes) >= limit:
        return None
    return next(
        (position for position, item in enumerate(pending) if item[3] == lane),
        None,
    )


def run_node(
    stage: Path,
    node: Mapping[str, Any],
    owner: Optional[AggregateResourceOwner] = None,
) -> None:
    scope_allocated = allocated_tree_bytes(stage) if owner is not None else None
    before = changed_content_snapshot(stage)
    command = list(node["command"])
    if command == ["internal:roadmap-evidence"]:
        refresh_roadmap_evidence(stage)
    else:
        run_bounded(
            command, cwd=stage,
            environment=stage_environment("GENESIS_GENERATED_AUTHORITY_STAGE"),
            timeout=node["timeoutSeconds"],
            owner=owner,
            disk_mib=node["diskMiB"],
        )
    after = changed_content_snapshot(stage)
    writes = {
        path for path in set(before) | set(after) if before.get(path) != after.get(path)
    }
    undeclared = sorted(writes - set(node["outputs"]))
    require(not undeclared, f"{node['id']} wrote undeclared paths: {undeclared}")
    if owner is not None:
        owner.check(
            node["id"],
            scope_allocated_bytes=scope_allocated,
            scope_root=stage,
            scope_disk_mib=node["diskMiB"],
        )


def run_checks(
    stage: Path,
    nodes: Sequence[Mapping[str, Any]],
    owner: Optional[AggregateResourceOwner] = None,
) -> None:
    require(owner is not None, "generated validation requires an aggregate resource owner")
    checks: dict[str, int] = {}
    for node in nodes:
        for check in node["checks"]:
            checks[check] = max(checks.get(check, 0), node["timeoutSeconds"])
    gate_manifest = stage / "genesis.gates.json"
    compilation_by_check: dict[str, bool] = {}
    if gate_manifest.is_file():
        manifest = load_json(gate_manifest)
        for gate in manifest.get("gates", []):
            entrypoint = gate.get("entrypoint")
            compilation = gate.get("compilation")
            if isinstance(entrypoint, str) and isinstance(compilation, bool):
                compilation_by_check[entrypoint] = compilation
        unknown = sorted(set(checks) - set(compilation_by_check))
        require(not unknown, "generated checks missing gate compilation metadata: " + ", ".join(unknown))
    environment, pass_fds = owner.child_environment(validation_environment())
    check_records = [
        (index, check, timeout, compilation_by_check.get(check, False))
        for index, (check, timeout) in enumerate(checks.items())
    ]
    pending = list(check_records)
    active: dict[int, tuple[str, int, bool, subprocess.Popen[bytes], Any, float, Path]] = {}
    completed_logs: dict[int, Path] = {}
    durations_ms: dict[int, int] = {}
    failure: Optional[BaseException] = None

    print(
        "generated-authority: validating "
        f"checks={len(pending)} workers={CHECK_WORKERS} "
        f"compilation_workers={COMPILATION_CHECK_WORKERS}",
        flush=True,
    )
    with tempfile.TemporaryDirectory(prefix="generated-authority-checks-") as temporary:
        log_root = Path(temporary)
        while pending or active:
            try:
                owner.check("parallel validation")
            except BaseException as exc:
                failure = exc
            while failure is None and pending and len(active) < CHECK_WORKERS:
                next_pending = next_check_position(
                    pending,
                    [item[2] for item in active.values()],
                )
                if next_pending is None:
                    break
                index, check, timeout, compilation = pending.pop(next_pending)
                log_path = log_root / f"{index:04d}.log"
                handle = log_path.open("wb")
                process = subprocess.Popen(
                    ["bash", check],
                    cwd=stage,
                    env=environment,
                    stdout=handle,
                    stderr=subprocess.STDOUT,
                    start_new_session=(os.name != "nt"),
                    **({"pass_fds": pass_fds} if pass_fds else {}),
                )
                active[index] = (
                    check, timeout, compilation, process, handle, time.monotonic(), log_path
                )

            now = time.monotonic()
            for index, (check, timeout, _, process, handle, started, log_path) in list(active.items()):
                return_code = process.poll()
                if return_code is None and now - started <= timeout:
                    continue
                if return_code is None:
                    failure = subprocess.TimeoutExpired(["bash", check], timeout)
                elif return_code != 0:
                    failure = subprocess.CalledProcessError(return_code, ["bash", check])
                handle.close()
                completed_logs[index] = log_path
                durations_ms[index] = round((now - started) * 1000)
                del active[index]
                if failure is not None:
                    break

            if failure is not None:
                for index, (_, _, _, process, handle, started, log_path) in active.items():
                    kill_and_reap(process)
                    handle.close()
                    completed_logs[index] = log_path
                    durations_ms[index] = round((time.monotonic() - started) * 1000)
                active.clear()
                break
            if active:
                time.sleep(0.05)

        sys.stdout.flush()
        for index in sorted(completed_logs):
            with completed_logs[index].open("rb") as handle:
                shutil.copyfileobj(handle, sys.stdout.buffer)
        sys.stdout.buffer.flush()
        for index, check, _, compilation in check_records:
            if index not in durations_ms:
                continue
            lane = "compilation" if compilation else "static"
            print(
                f"generated-authority-check: {check} lane={lane} duration_ms={durations_ms[index]}"
            )
        if failure is not None:
            raise failure


def tree_snapshot(root: Path, excluded_outputs: set[str]) -> str:
    digest = sha256()
    paths = set(git(root, "ls-files").splitlines()) | set(git(root, "ls-files", "--others", "--exclude-standard").splitlines())
    for rel in sorted(paths - excluded_outputs):
        path = root / rel
        if not path.is_file() and not path.is_symlink():
            continue
        digest.update(rel.encode("utf-8") + b"\0")
        if path.is_symlink():
            digest.update(f"link:{repository_mode(path):o}\0".encode("ascii"))
            digest.update(os.readlink(path).encode("utf-8"))
        else:
            digest.update(f"file:{repository_mode(path):o}\0".encode("ascii"))
            digest.update(path.read_bytes())
        digest.update(b"\0")
    return digest.hexdigest()


def common_git_dir(root: Path) -> Path:
    value = git(root, "rev-parse", "--git-common-dir").strip()
    path = Path(value)
    return path if path.is_absolute() else (root / path).resolve()


def promote(
    root: Path,
    stage: Path,
    outputs: Sequence[str],
    *,
    expected_input_snapshot: Optional[str] = None,
    expected_output_identities: Optional[Mapping[str, str]] = None,
) -> list[str]:
    for rel in outputs:
        require(
            (stage / rel).is_file() and not (stage / rel).is_symlink()
            and (root / rel).is_file() and not (root / rel).is_symlink(),
            f"generated output must remain a regular file: {rel}",
        )
    changed = [rel for rel in outputs if file_identity(stage / rel) != file_identity(root / rel)]
    if not changed:
        return []
    lock = common_git_dir(root) / LOCK_NAME
    try:
        descriptor = os.open(lock, os.O_CREAT | os.O_EXCL | os.O_WRONLY, 0o600)
    except FileExistsError as exc:
        raise AuthorityError(f"generated-authority publication lock exists: {lock}") from exc
    try:
        os.write(descriptor, f"pid={os.getpid()}\n".encode("ascii"))
        os.close(descriptor)
        descriptor = -1
        transaction = Path(
            tempfile.mkdtemp(
                prefix="generated-authority-transaction-", dir=common_git_dir(root)
            )
        )
    except BaseException:
        if descriptor >= 0:
            os.close(descriptor)
        lock.unlink(missing_ok=True)
        raise
    backups = transaction / "backups"
    backups.mkdir()
    promoted: list[str] = []
    old_mask = None
    try:
        if hasattr(signal, "pthread_sigmask"):
            old_mask = signal.pthread_sigmask(signal.SIG_BLOCK, {signal.SIGINT, signal.SIGTERM, signal.SIGHUP})
        if expected_input_snapshot is not None:
            require(
                tree_snapshot(root, set(outputs)) == expected_input_snapshot,
                "canonical inputs changed before generated publication lock",
            )
        if expected_output_identities is not None:
            observed = {rel: file_identity(root / rel) for rel in outputs}
            require(
                observed == expected_output_identities,
                "canonical outputs changed before generated publication lock",
            )
        for rel in changed:
            source = stage / rel
            destination = root / rel
            require(
                source.is_file() and not source.is_symlink()
                and destination.is_file() and not destination.is_symlink(),
                f"generated output must be a regular file before promotion: {rel}",
            )
            backup = backups / rel
            backup.parent.mkdir(parents=True, exist_ok=True)
            shutil.copy2(destination, backup)
            temporary = destination.with_name(f".{destination.name}.generated-authority-{os.getpid()}")
            shutil.copy2(source, temporary)
            os.replace(temporary, destination)
            promoted.append(rel)
            fail_after = os.environ.get("GENESIS_GENERATED_AUTHORITY_FAIL_AFTER_PROMOTIONS")
            if fail_after and len(promoted) >= int(fail_after):
                raise AuthorityError("injected promotion failure")
        if expected_input_snapshot is not None:
            require(
                tree_snapshot(root, set(outputs)) == expected_input_snapshot,
                "canonical inputs changed during generated publication",
            )
        observed_published = {rel: file_identity(root / rel) for rel in outputs}
        expected_published = {rel: file_identity(stage / rel) for rel in outputs}
        require(
            observed_published == expected_published,
            "canonical outputs changed during generated publication",
        )
    except BaseException:
        for rel in reversed(promoted):
            os.replace(backups / rel, root / rel)
        raise
    finally:
        if old_mask is not None:
            signal.pthread_sigmask(signal.SIG_SETMASK, old_mask)
        shutil.rmtree(transaction, ignore_errors=True)
        lock.unlink(missing_ok=True)
    return changed


def stage_closure(
    root: Path,
    nodes: Sequence[Mapping[str, Any]],
    *,
    update: bool,
    limits: Mapping[str, Any],
) -> list[str]:
    outputs = sorted({output for node in nodes for output in node["outputs"]})
    require(
        all((root / output).is_file() and not (root / output).is_symlink() for output in outputs),
        "all generated outputs must exist as regular files before staging",
    )
    baseline = tree_snapshot(root, set(outputs))
    baseline_outputs = {output: file_identity(root / output) for output in outputs}
    temporary_root = Path(tempfile.mkdtemp(prefix="generated-authority-stage-"))
    stage = temporary_root / "worktree"
    owner = AggregateResourceOwner(root, temporary_root, limits)
    try:
        owner.check("worktree setup")
        run_bounded(
            ["git", "worktree", "add", "--detach", str(stage), "HEAD"],
            cwd=root,
            timeout=min(300, owner.timeout_seconds),
            owner=owner,
        )
        owner.check("worktree checkout")
        copy_overlay(root, stage)
        owner.check("worktree overlay")
        for node in nodes:
            run_node(stage, node, owner)
        run_checks(stage, nodes, owner)
        owner.check("staged closure validation")
        require(tree_snapshot(root, set(outputs)) == baseline, "canonical inputs changed while generated closure was staged")
        stale = [
            output for output in outputs
            if file_identity(stage / output) != file_identity(root / output)
        ]
        if not update:
            require(not stale, "generated-authority closure is stale: " + ", ".join(stale))
            return []
        promoted = promote(
            root, stage, outputs,
            expected_input_snapshot=baseline,
            expected_output_identities=baseline_outputs,
        )
        owner.check("generated publication")
        return promoted
    finally:
        if stage.exists():
            subprocess.run(
                ["git", "worktree", "remove", "--force", str(stage)],
                cwd=root,
                stdout=subprocess.DEVNULL,
                stderr=subprocess.DEVNULL,
                timeout=60,
            )
        try:
            owner.close()
        finally:
            shutil.rmtree(temporary_root, ignore_errors=True)


def synthetic_graph(root: Path, graph: Mapping[str, Any], mutation: callable) -> None:
    candidate = copy.deepcopy(graph)
    mutation(candidate)
    validate_graph(root, candidate)


def self_test(root: Path, graph: Mapping[str, Any]) -> None:
    controls = 0

    static = (0, "scripts/static.sh", 1, False)
    compilation = (1, "scripts/compilation.sh", 1, True)
    require(
        next_check_position([static, compilation], []) == 0
        and next_check_position([compilation], [False]) is None
        and next_check_position([static], [True]) is None
        and next_check_position(
            [compilation], [True] * (COMPILATION_CHECK_WORKERS - 1)
        )
        == 0
        and next_check_position(
            [compilation], [True] * COMPILATION_CHECK_WORKERS
        )
        is None,
        "generated check scheduler mixed lanes or exceeded its compiler bound",
    )
    controls += 1

    staged_names = (*STAGE_SCOPED_ENVIRONMENT, *STAGE_BUILD_ENVIRONMENT)
    saved_stage_values = {name: os.environ.get(name) for name in staged_names}
    try:
        for name in staged_names:
            os.environ[name] = "inherited-fixture"
        isolated = stage_environment("GENESIS_GENERATED_AUTHORITY_STAGE")
        require(
            not any(name in isolated for name in STAGE_SCOPED_ENVIRONMENT),
            "staging environment retained repository-scoped Cargo provenance",
        )
        require(
            isolated.get("GENESIS_GENERATED_AUTHORITY_STAGE") == "1",
            "staging environment omitted its execution marker",
        )
        require(
            all(isolated.get(name) == value for name, value in STAGE_BUILD_ENVIRONMENT.items()),
            "staging environment omitted its deterministic slim Cargo profile",
        )
        validating = validation_environment()
        require(
            validating.get("GENESIS_GENERATED_AUTHORITY_VALIDATING") == "1"
            and "GENESIS_GATE_BUDGET_ENFORCE" not in validating
            and "GENESIS_GATE_AGGREGATE_OWNER_FD" not in validating,
            "validation environment retained a caller-selected resource owner",
        )
    finally:
        for name, value in saved_stage_values.items():
            if value is None:
                os.environ.pop(name, None)
            else:
                os.environ[name] = value

    with tempfile.TemporaryDirectory(prefix="generated-authority-check-cancel-") as temporary:
        stage = Path(temporary)
        scripts = stage / "scripts"
        scripts.mkdir()
        marker = stage / "escaped"
        (scripts / "fail.sh").write_text("#!/usr/bin/env bash\nexit 7\n", encoding="utf-8")
        (scripts / "linger.sh").write_text(
            "#!/usr/bin/env bash\nsleep 2\nprintf escaped > escaped\n",
            encoding="utf-8",
        )
        fixture = [{
            "checks": ["scripts/fail.sh", "scripts/linger.sh"],
            "timeoutSeconds": 5,
        }]
        try:
            run_checks(stage, fixture)
        except AuthorityError:
            controls += 1
        else:
            raise AuthorityError("self-test accepted validation without an aggregate owner")
        owner = AggregateResourceOwner(
            stage, stage, {"maxTimeoutSeconds": 30, "maxDiskMiB": 64}
        )
        try:
            try:
                run_checks(stage, fixture, owner)
            except subprocess.CalledProcessError:
                time.sleep(0.1)
                require(not marker.exists(), "failed check did not cancel its concurrent process group")
                controls += 1
            else:
                raise AuthorityError("self-test accepted a failed concurrent check")

            (scripts / "timeout.sh").write_text(
                "#!/usr/bin/env bash\nsleep 2\nprintf escaped > escaped\n",
                encoding="utf-8",
            )
            try:
                run_checks(
                    stage,
                    [{"checks": ["scripts/timeout.sh"], "timeoutSeconds": 0}],
                    owner,
                )
            except subprocess.TimeoutExpired:
                time.sleep(0.1)
                require(not marker.exists(), "timed-out check escaped hard cancellation")
                controls += 1
            else:
                raise AuthorityError("self-test accepted a timed-out check")
        finally:
            owner.close(validate=False)

    with tempfile.TemporaryDirectory(prefix="generated-authority-resource-") as temporary:
        resource_root = Path(temporary)
        wall_owner = AggregateResourceOwner(
            resource_root,
            resource_root,
            {"maxTimeoutSeconds": 0, "maxDiskMiB": 64},
        )
        wall_marker = resource_root / "wall-escaped"
        try:
            run_bounded(
                ["bash", "-c", "sleep 2; printf escaped > wall-escaped"],
                cwd=resource_root,
                timeout=5,
                owner=wall_owner,
            )
        except AuthorityError:
            time.sleep(0.1)
            require(not wall_marker.exists(), "aggregate wall failure did not kill its child group")
            controls += 1
        else:
            raise AuthorityError("self-test accepted an aggregate wall overrun")
        finally:
            wall_owner.close(validate=False)

        disk_owner = AggregateResourceOwner(
            resource_root,
            resource_root,
            {"maxTimeoutSeconds": 30, "maxDiskMiB": 64},
        )
        try:
            run_bounded(
                [
                    sys.executable,
                    "-c",
                    "import os; f=open('disk-payload','wb'); f.write(b'x'*(2*1024*1024)); f.flush(); os.fsync(f.fileno()); f.close()",
                ],
                cwd=resource_root,
                timeout=5,
                owner=disk_owner,
                disk_mib=0,
            )
        except AuthorityError:
            controls += 1
        else:
            raise AuthorityError("self-test accepted a generated-node disk overrun")
        finally:
            disk_owner.close(validate=False)
            (resource_root / "disk-payload").unlink(missing_ok=True)

        network_owner = AggregateResourceOwner(
            resource_root,
            resource_root,
            {"maxTimeoutSeconds": 30, "maxDiskMiB": 64},
        )
        try:
            if network_owner.event_fd is None:
                require(os.name == "nt", "aggregate event owner is unavailable")
            else:
                environment, pass_fds = network_owner.child_environment(os.environ)
                subprocess.run(
                    [
                        "bash",
                        "-c",
                        "printf '{\"count\":1,\"kind\":\"network-attempt\"}\\n' >>\"$GENESIS_GATE_TELEMETRY_EVENT_FILE\"",
                    ],
                    cwd=resource_root,
                    env=environment,
                    pass_fds=pass_fds,
                    check=True,
                )
                try:
                    network_owner.check("network control")
                except AuthorityError:
                    pass
                else:
                    raise AuthorityError("self-test accepted an aggregate network attempt")
            controls += 1
        finally:
            network_owner.close(validate=False)

    def rejected(label: str, mutation: callable) -> None:
        nonlocal controls
        try:
            synthetic_graph(root, graph, mutation)
        except AuthorityError:
            controls += 1
            return
        raise AuthorityError(f"self-test expected rejection: {label}")

    rejected("duplicate-owner", lambda g: g["nodes"][1]["outputs"].append(g["nodes"][0]["outputs"][0]))
    rejected("cycle", lambda g: g["nodes"][0]["dependencies"].append(g["nodes"][1]["id"]))
    rejected("unknown-check", lambda g: g["nodes"][0]["checks"].append("scripts/check_missing_fixture.sh"))
    rejected("resource-limit", lambda g: g["nodes"][0].__setitem__("diskMiB", g["limits"]["maxDiskMiB"] + 1))
    rejected("protected-output", lambda g: g["nodes"][0]["outputs"].append("docs/program/evidence/E3/fixture.json"))
    rejected("signing-command", lambda g: g["nodes"][0].__setitem__("command", ["genesis", "attest"]))
    rejected("unknown-updater", lambda g: g["excludedEntrypoints"].pop(next(iter(g["excludedEntrypoints"]))))
    rejected("mutation-route-drift", lambda g: g["mutationControls"][0]["expectedNodes"].pop())
    rejected(
        "fixed-point-exclusion",
        lambda g: g["identityExclusions"].pop("genesis.gates.json"),
    )
    gate_node_index = next(
        index for index, node in enumerate(graph["nodes"])
        if node["id"] == "generate/gate-manifest"
    )
    rejected(
        "output-read-order",
        lambda g: g["nodes"][gate_node_index].__setitem__("dependencies", []),
    )
    command_node_index = next(
        index for index, node in enumerate(graph["nodes"])
        if len(node["command"]) >= 2 and node["command"][1].startswith("scripts/")
    )
    command_source = graph["nodes"][command_node_index]["command"][1]
    rejected(
        "undeclared-command-source",
        lambda g: g["nodes"][command_node_index]["inputs"].remove(command_source),
    )
    operator_output = next(
        node["outputs"][0] for node in graph["nodes"]
        if node["mode"] == "operator-gated"
    )
    try:
        closure_for_paths(
            graph["nodes"],
            [operator_output],
            include_operator=False,
            gate_inputs=gate_inputs_by_check(root),
        )
    except AuthorityError:
        controls += 1
    else:
        raise AuthorityError("self-test allowed automatic operator-gated publication")

    discovered_route = closure_for_paths(
        graph["nodes"],
        ["crates/gc_cli/tests/cli_genesisbench_front_door.rs"],
        include_operator=True,
        gate_inputs=gate_inputs_by_check(root),
    )
    discovered_ids = {node["id"] for node in discovered_route}
    require(
        {"generate/benchmark-protocol", "generate/agent-corpus", "generate/gate-manifest"}
        <= discovered_ids,
        "recursively discovered Rust gate input did not reach its generated closure",
    )
    controls += 1

    with tempfile.TemporaryDirectory(prefix="generated-authority-write-set-") as temporary:
        repository = Path(temporary)
        subprocess.run(["git", "init", "-q"], cwd=repository, check=True)
        subprocess.run(["git", "config", "user.email", "authority@example.invalid"], cwd=repository, check=True)
        subprocess.run(["git", "config", "user.name", "Authority Self Test"], cwd=repository, check=True)
        (repository / "declared").write_text("old\n", encoding="utf-8")
        (repository / "undeclared").write_text("old\n", encoding="utf-8")
        subprocess.run(["git", "add", "."], cwd=repository, check=True)
        subprocess.run(["git", "commit", "-qm", "fixture"], cwd=repository, check=True)
        fixture_node = {
            "id": "generate/write-set-fixture",
            "command": [
                "python3", "-c",
                "from pathlib import Path; Path('declared').write_text('new\\n'); Path('undeclared').write_text('new\\n')",
            ],
            "outputs": ["declared"],
            "timeoutSeconds": 10,
            "diskMiB": 16,
        }
        try:
            run_node(repository, fixture_node)
        except AuthorityError:
            controls += 1
        else:
            raise AuthorityError("self-test accepted an undeclared write")

        (repository / "declared").write_text("old\n", encoding="utf-8")
        (repository / "undeclared").write_text("dirty-before\n", encoding="utf-8")
        try:
            run_node(repository, fixture_node)
        except AuthorityError:
            controls += 1
        else:
            raise AuthorityError(
                "self-test accepted mutation of an already-dirty undeclared path"
            )

    with tempfile.TemporaryDirectory(prefix="generated-authority-self-test-") as temporary:
        base = Path(temporary)
        live = base / "live"
        staged = base / "stage"
        live.mkdir()
        staged.mkdir()
        for directory in (live, staged):
            (directory / "a").write_bytes(b"old\n")
            (directory / "b").write_bytes(b"old\n")
        restrictive = live / "restrictive"
        checkout = staged / "checkout"
        restrictive.write_bytes(b"same\n")
        checkout.write_bytes(b"same\n")
        restrictive.chmod(0o600)
        checkout.chmod(0o644)
        require(
            file_identity(restrictive) == file_identity(checkout),
            "repository identity included permission bits Git cannot reproduce",
        )
        (staged / "a").write_bytes(b"new-a\n")
        (staged / "b").chmod(0o755)
        original_common = common_git_dir
        globals()["common_git_dir"] = lambda _root: base
        try:
            os.environ["GENESIS_GENERATED_AUTHORITY_FAIL_AFTER_PROMOTIONS"] = "1"
            try:
                promote(live, staged, ["a", "b"])
            except AuthorityError:
                require((live / "a").read_bytes() == b"old\n" and (live / "b").read_bytes() == b"old\n", "promotion rollback was not byte-identical")
                controls += 1
            else:
                raise AuthorityError("self-test expected injected promotion failure")
            os.environ.pop("GENESIS_GENERATED_AUTHORITY_FAIL_AFTER_PROMOTIONS", None)
            require(promote(live, staged, ["a", "b"]) == ["a", "b"], "promotion did not publish both outputs")
            require((live / "b").stat().st_mode & 0o777 == 0o755, "promotion lost a Git-representable executable-bit change")
            require(promote(live, staged, ["a", "b"]) == [], "second promotion was not a no-op")
            controls += 1
        finally:
            os.environ.pop("GENESIS_GENERATED_AUTHORITY_FAIL_AFTER_PROMOTIONS", None)
            globals()["common_git_dir"] = original_common

    with tempfile.TemporaryDirectory(prefix="generated-authority-race-test-") as temporary:
        base = Path(temporary)
        live = base / "live"
        staged = base / "stage"
        live.mkdir()
        staged.mkdir()
        subprocess.run(["git", "init", "-q"], cwd=live, check=True)
        subprocess.run(["git", "config", "user.email", "authority@example.invalid"], cwd=live, check=True)
        subprocess.run(["git", "config", "user.name", "Authority Self Test"], cwd=live, check=True)
        for name in ("a", "b", "input"):
            (live / name).write_text("old\n", encoding="utf-8")
        subprocess.run(["git", "add", "."], cwd=live, check=True)
        subprocess.run(["git", "commit", "-qm", "fixture"], cwd=live, check=True)
        (staged / "a").write_text("new-a\n", encoding="utf-8")
        (staged / "b").write_text("new-b\n", encoding="utf-8")
        outputs = ["a", "b"]
        expected_inputs = tree_snapshot(live, set(outputs))
        expected_outputs = {name: file_identity(live / name) for name in outputs}
        (live / "input").write_text("concurrent\n", encoding="utf-8")
        try:
            promote(
                live, staged, outputs,
                expected_input_snapshot=expected_inputs,
                expected_output_identities=expected_outputs,
            )
        except AuthorityError:
            require((live / "a").read_text() == "old\n", "input-race rejection changed an output")
            controls += 1
        else:
            raise AuthorityError("self-test accepted concurrent input drift")
        (live / "input").write_text("old\n", encoding="utf-8")
        expected_inputs = tree_snapshot(live, set(outputs))
        expected_outputs = {name: file_identity(live / name) for name in outputs}
        (live / "a").write_text("concurrent-output\n", encoding="utf-8")
        try:
            promote(
                live, staged, outputs,
                expected_input_snapshot=expected_inputs,
                expected_output_identities=expected_outputs,
            )
        except AuthorityError:
            require(
                (live / "a").read_text() == "concurrent-output\n",
                "output-race rejection overwrote the concurrent output",
            )
            controls += 1
        else:
            raise AuthorityError("self-test accepted concurrent output drift")
    require(controls == 26, "generated-authority self-test inventory drift")
    print(f"generated-authority-self-test: ok (negative_controls={controls})")


def main(argv: Sequence[str]) -> int:
    parser = argparse.ArgumentParser()
    mode = parser.add_mutually_exclusive_group(required=True)
    mode.add_argument("--check", action="store_true", help="validate graph and discovery closure")
    mode.add_argument("--freshness", action="store_true", help="stage affected generators and require byte freshness")
    mode.add_argument("--update", action="store_true", help="stage, validate, and transactionally promote")
    mode.add_argument("--plan", action="store_true", help="print selected node IDs")
    mode.add_argument("--self-test", action="store_true")
    parser.add_argument("--all", action="store_true", help="select every automatic node")
    parser.add_argument(
        "--path",
        action="append",
        default=[],
        help="select closure for one path in addition to every dirty overlay path",
    )
    parser.add_argument("--git-base", help="include committed changes since this Git revision")
    parser.add_argument("--root", type=Path, default=ROOT)
    args = parser.parse_args(argv)
    root = args.root.resolve()
    try:
        policy = load_json(root / POLICY_REL)
        graph = graph_from_policy(policy)
        nodes = validate_graph(root, graph)
        if args.self_test:
            self_test(root, graph)
            return 0
        if args.check:
            lock = common_git_dir(root) / LOCK_NAME
            require(not lock.exists(), f"generated-authority publication is in progress: {lock}")
            subprocess.run(
                ["python3", "scripts/lib/gate_manifest.py", "--check"],
                cwd=root,
                check=True,
                stdout=subprocess.DEVNULL,
            )
            print(f"generated-authority: ok (nodes={len(nodes)} outputs={sum(len(node['outputs']) for node in nodes)} updaters={len(update_inventory(root))})")
            return 0
        paths = [canonical_path(path, "--path") for path in args.path]
        if args.all:
            selected = [node for node in topological(nodes) if node["mode"] == "automatic"]
        else:
            # Staging always overlays the complete worktree patch. Selection must
            # therefore cover every dirty input even when callers name one path.
            paths = sorted(set(paths) | set(changed_paths(root, args.git_base)))
            selected = closure_for_paths(
                nodes,
                paths,
                include_operator=False,
                gate_inputs=gate_inputs_by_check(root),
            ) if paths else (
                [node for node in topological(nodes) if node["mode"] == "automatic"]
                if args.update else []
            )
        if not selected:
            print("generated-authority: no affected nodes")
            return 0
        if args.plan:
            print("\n".join(node["id"] for node in selected))
            return 0
        changed = stage_closure(
            root, selected, update=args.update, limits=graph["limits"]
        )
        action = "updated" if args.update else "fresh"
        print(f"generated-authority: {action} (nodes={len(selected)} changed={len(changed)})")
        if changed:
            print("generated-authority: promoted " + ", ".join(changed))
    except (AuthorityError, subprocess.CalledProcessError, subprocess.TimeoutExpired, OSError) as exc:
        print(f"generated-authority: {exc}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
