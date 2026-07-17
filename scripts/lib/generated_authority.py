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
)
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
    gate_manifest = load_json(root / GATES_REL)
    gate_checks = {gate["entrypoint"] for gate in gate_manifest["gates"]}
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
        selected = closure_for_paths(nodes, [path], include_operator=True)
        require(expected_nodes == [node["id"] for node in selected], f"mutation route drift for {path}")
        actual_outputs = sorted({output for node in selected for output in node["outputs"]})
        require(expected_outputs == actual_outputs, f"mutation output route drift for {path}")
    required_controls = {
        "Cargo.lock", "docs/spec/CLI_JSON_SCHEMAS_v0.1.md", "ROADMAP.md",
        "policies/gc_agent_profile_v0.3.json",
        "policies/gc_diagnostic_catalog_v0.1.json",
        "benchmarks/agent_tasks/v0.1/suite.json",
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


def closure_for_paths(nodes: Sequence[Mapping[str, Any]], paths: Sequence[str], *, include_operator: bool) -> list[Mapping[str, Any]]:
    selected = {node["id"] for node in nodes if any(matches(path, node["inputs"]) for path in paths)}
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


def content_snapshot(root: Path) -> Mapping[str, str]:
    paths = set(git(root, "ls-files").splitlines()) | set(
        git(root, "ls-files", "--others", "--exclude-standard").splitlines()
    )
    result: dict[str, str] = {}
    for rel in sorted(path for path in paths if path and not path.startswith(".genesis/")):
        path = root / rel
        if not path.is_symlink() and not path.is_file():
            continue
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


def run_bounded(
    command: Sequence[str], *, cwd: Path, timeout: int,
    environment: Optional[Mapping[str, str]] = None,
) -> None:
    process = subprocess.Popen(
        list(command), cwd=cwd, env=environment,
        start_new_session=(os.name != "nt"),
    )
    try:
        return_code = process.wait(timeout=timeout)
    except subprocess.TimeoutExpired:
        if os.name != "nt":
            os.killpg(process.pid, signal.SIGKILL)
        else:
            process.kill()
        process.wait()
        raise
    if return_code != 0:
        raise subprocess.CalledProcessError(return_code, list(command))


def stage_environment(marker: str) -> dict[str, str]:
    environment = os.environ.copy()
    for name in STAGE_SCOPED_ENVIRONMENT:
        environment.pop(name, None)
    environment[marker] = "1"
    return environment


def run_node(stage: Path, node: Mapping[str, Any]) -> None:
    before = content_snapshot(stage)
    command = list(node["command"])
    if command == ["internal:roadmap-evidence"]:
        refresh_roadmap_evidence(stage)
    else:
        run_bounded(
            command, cwd=stage,
            environment=stage_environment("GENESIS_GENERATED_AUTHORITY_STAGE"),
            timeout=node["timeoutSeconds"],
        )
    after = content_snapshot(stage)
    writes = {
        path for path in set(before) | set(after) if before.get(path) != after.get(path)
    }
    undeclared = sorted(writes - set(node["outputs"]))
    require(not undeclared, f"{node['id']} wrote undeclared paths: {undeclared}")


def run_checks(stage: Path, nodes: Sequence[Mapping[str, Any]]) -> None:
    checks: dict[str, int] = {}
    for node in nodes:
        for check in node["checks"]:
            checks[check] = max(checks.get(check, 0), node["timeoutSeconds"])
    environment = stage_environment("GENESIS_GENERATED_AUTHORITY_VALIDATING")
    for check, timeout in checks.items():
        run_bounded(
            ["bash", check], cwd=stage, environment=environment,
            timeout=timeout,
        )


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


def stage_closure(root: Path, nodes: Sequence[Mapping[str, Any]], *, update: bool) -> list[str]:
    outputs = sorted({output for node in nodes for output in node["outputs"]})
    require(
        all((root / output).is_file() and not (root / output).is_symlink() for output in outputs),
        "all generated outputs must exist as regular files before staging",
    )
    baseline = tree_snapshot(root, set(outputs))
    baseline_outputs = {output: file_identity(root / output) for output in outputs}
    temporary_root = Path(tempfile.mkdtemp(prefix="generated-authority-stage-"))
    stage = temporary_root / "worktree"
    try:
        git(root, "worktree", "add", "--detach", str(stage), "HEAD", capture=False)
        copy_overlay(root, stage)
        for node in nodes:
            run_node(stage, node)
        run_checks(stage, nodes)
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
        return promoted
    finally:
        if stage.exists():
            subprocess.run(["git", "worktree", "remove", "--force", str(stage)], cwd=root, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        shutil.rmtree(temporary_root, ignore_errors=True)


def synthetic_graph(root: Path, graph: Mapping[str, Any], mutation: callable) -> None:
    candidate = copy.deepcopy(graph)
    mutation(candidate)
    validate_graph(root, candidate)


def self_test(root: Path, graph: Mapping[str, Any]) -> None:
    controls = 0

    saved_stage_values = {name: os.environ.get(name) for name in STAGE_SCOPED_ENVIRONMENT}
    try:
        for name in STAGE_SCOPED_ENVIRONMENT:
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
    finally:
        for name, value in saved_stage_values.items():
            if value is None:
                os.environ.pop(name, None)
            else:
                os.environ[name] = value

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
        closure_for_paths(graph["nodes"], [operator_output], include_operator=False)
    except AuthorityError:
        controls += 1
    else:
        raise AuthorityError("self-test allowed automatic operator-gated publication")

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
        }
        try:
            run_node(repository, fixture_node)
        except AuthorityError:
            controls += 1
        else:
            raise AuthorityError("self-test accepted an undeclared write")

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
    require(controls == 16, "generated-authority self-test inventory drift")
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
    parser.add_argument("--path", action="append", default=[], help="select closure for one changed path")
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
            print(f"generated-authority: ok (nodes={len(nodes)} outputs={sum(len(node['outputs']) for node in nodes)} updaters={len(update_inventory(root))})")
            return 0
        paths = [canonical_path(path, "--path") for path in args.path]
        if args.all:
            selected = [node for node in topological(nodes) if node["mode"] == "automatic"]
        else:
            paths = paths or changed_paths(root, args.git_base)
            selected = closure_for_paths(nodes, paths, include_operator=False) if paths else (
                [node for node in topological(nodes) if node["mode"] == "automatic"]
                if args.update else []
            )
        if not selected:
            print("generated-authority: no affected nodes")
            return 0
        if args.plan:
            print("\n".join(node["id"] for node in selected))
            return 0
        changed = stage_closure(root, selected, update=args.update)
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
