#!/usr/bin/env python3
"""Validate the closed production dependency and source boundary for stage0."""

from __future__ import annotations

import argparse
import copy
import hashlib
import json
import re
import subprocess
from pathlib import Path
from typing import Any, Callable

from toml_compat import tomllib


class BoundaryError(ValueError):
    pass


ROOT_FIELDS = {
    "auditDate", "canonicalSpec", "canonicalSpecSha256", "contentIdentitySha256",
    "forbiddenAmbientPackages", "forbiddenWorkspacePackages", "kind", "nonclaims",
    "schema", "schemaSha256", "sourceEscapes", "stage0Packages",
    "stage0TrustContract", "stage0TrustContractIdentitySha256", "version",
}
PACKAGE_FIELDS = {
    "allowedBuildExternalDependencies", "allowedBuildWorkspaceDependencies",
    "allowedDevExternalDependencies", "allowedDevWorkspaceDependencies",
    "allowedFeatures", "allowedProductionExternalDependencies",
    "allowedProductionWorkspaceDependencies", "buildScriptPath", "domains",
    "manifestPath", "manifestSha256", "name", "productionResolvedClosureSha256",
    "productionResolvedPackageCount", "productionWorkspaceClosure", "sourceRoot",
}
ESCAPE_FIELDS = {"line", "path", "reason"}
STAGE0_PACKAGES = ("gc_coreform", "gc_kernel", "gc_prelude")
EXPECTED_DOMAINS = {
    "gc_coreform": ("S0-R",),
    "gc_kernel": ("S0-K", "S0-X"),
    "gc_prelude": ("S0-A", "S0-P"),
}
REQUIRED_AMBIENT_DENYLIST = {
    "async-std", "clap", "crossterm", "getrandom", "libloading", "mio", "notify",
    "rand", "rand_core", "reqwest", "rusqlite", "socket2", "tiny_http", "tokio",
    "wasm-bindgen", "wgpu",
}
EXPECTED_STAGE0_IDENTITY = "a44f3030762c4fe6cc10404ed737bf3c2a9d459e12012c375b4e968adab2c8b8"
SPEC_MARKERS = (
    "permitted workspace graph is exact and acyclic",
    "Dev dependencies are separately enumerated",
    "resolved non-dev graph",
    "Every stage0 manifest, direct external dependency, and feature definition is bound",
    "root-independent digest binds every resolved non-dev",
    "`include_bytes!` line is denied",
    "does not become runtime language semantics",
)
NONCLAIMS = (
    "changes-no-semantic-decision-h-level-production-authority-or-fallback",
    "proves-no-H2-H3-H4-or-release-readiness",
    "grants-no-authority-to-dev-only-parity-dependencies",
    "does-not-broaden-the-bound-stage0-contract",
)
INCLUDE_RE = re.compile(r"\binclude(?:_str|_bytes)?!\s*\(")
PATH_RE = re.compile(r"#\s*\[\s*path\s*=\s*\"([^\"]+)\"\s*\]")


def fail(message: str) -> None:
    raise BoundaryError(message)


def no_duplicate_pairs(pairs: list[tuple[str, Any]]) -> dict[str, Any]:
    result: dict[str, Any] = {}
    for key, value in pairs:
        if key in result:
            fail(f"duplicate JSON key: {key}")
        result[key] = value
    return result


def load_json(path: Path) -> Any:
    try:
        return json.loads(path.read_text(encoding="utf-8"), object_pairs_hook=no_duplicate_pairs)
    except (OSError, json.JSONDecodeError) as exc:
        fail(f"cannot read {path}: {exc}")


def sha256(raw: bytes) -> str:
    return hashlib.sha256(raw).hexdigest()


def identity(value: dict[str, Any]) -> str:
    payload = {key: item for key, item in value.items() if key != "contentIdentitySha256"}
    raw = json.dumps(payload, sort_keys=True, separators=(",", ":"), ensure_ascii=True)
    return sha256(raw.encode("utf-8"))


def reseal(value: dict[str, Any], spec: bytes, schema: bytes) -> None:
    value["canonicalSpecSha256"] = sha256(spec)
    value["schemaSha256"] = sha256(schema)
    value["contentIdentitySha256"] = identity(value)


def closed(value: Any, fields: set[str], label: str) -> dict[str, Any]:
    if not isinstance(value, dict):
        fail(f"{label} must be an object")
    missing = sorted(fields - set(value))
    unknown = sorted(set(value) - fields)
    if missing or unknown:
        fail(f"{label} field drift: missing={missing}, unknown={unknown}")
    return value


def strings(value: Any, label: str) -> list[str]:
    if not isinstance(value, list) or not all(isinstance(item, str) and item for item in value):
        fail(f"{label} must be a string array")
    if value != sorted(set(value)):
        fail(f"{label} must be sorted and unique")
    return value


def relative_file(root: Path, value: Any, label: str) -> Path:
    if not isinstance(value, str) or not value:
        fail(f"{label} must be a repository-relative path")
    path = Path(value)
    if path.is_absolute() or ".." in path.parts:
        fail(f"{label} escapes repository: {value}")
    resolved = root / path
    if not resolved.is_file():
        fail(f"{label} is not a file: {value}")
    return resolved


def validate_schema(schema: Any) -> None:
    value = closed(
        schema,
        {"$schema", "$id", "title", "type", "additionalProperties", "required", "properties", "$defs"},
        "schema",
    )
    if value.get("type") != "object" or value.get("additionalProperties") is not False:
        fail("schema root must be a closed object")
    if set(value.get("required", [])) != ROOT_FIELDS or set(value.get("properties", {})) != ROOT_FIELDS:
        fail("schema root fields drift")
    package = value["properties"]["stage0Packages"]["items"]
    if package.get("additionalProperties") is not False:
        fail("schema stage0 package must be closed")
    if set(package.get("required", [])) != PACKAGE_FIELDS or set(package.get("properties", {})) != PACKAGE_FIELDS:
        fail("schema stage0 package fields drift")
    escape = value["properties"]["sourceEscapes"]["items"]
    if escape.get("additionalProperties") is not False:
        fail("schema source escape must be closed")
    if set(escape.get("required", [])) != ESCAPE_FIELDS or set(escape.get("properties", {})) != ESCAPE_FIELDS:
        fail("schema source escape fields drift")


def workspace_manifests(root: Path) -> dict[str, Path]:
    workspace = tomllib.loads((root / "Cargo.toml").read_text(encoding="utf-8"))
    members = workspace.get("workspace", {}).get("members", [])
    if not isinstance(members, list) or not members:
        fail("workspace member inventory missing")
    result: dict[str, Path] = {}
    for member in members:
        manifest = root / str(member) / "Cargo.toml"
        data = tomllib.loads(manifest.read_text(encoding="utf-8"))
        name = data.get("package", {}).get("name")
        if not isinstance(name, str) or name in result:
            fail(f"invalid or duplicate workspace package: {name}")
        result[name] = manifest
    return result


def dependency_sections(manifest: dict[str, Any], kind: str) -> dict[str, Any]:
    section_name = {
        "production": "dependencies",
        "build": "build-dependencies",
        "dev": "dev-dependencies",
    }[kind]
    result = dict(manifest.get(section_name, {}))
    for target in manifest.get("target", {}).values():
        if isinstance(target, dict):
            for key, spec in target.get(section_name, {}).items():
                if key in result and result[key] != spec:
                    fail(f"dependency alias {key} has class/target drift")
                result[key] = spec
    return result


def canonical_dependency_name(alias: str, spec: Any) -> str:
    if isinstance(spec, dict) and isinstance(spec.get("package"), str):
        return spec["package"]
    return alias


def partition_dependencies(entries: dict[str, Any], workspace_names: set[str]) -> tuple[list[str], list[str]]:
    local: list[str] = []
    external: list[str] = []
    for alias, spec in entries.items():
        name = canonical_dependency_name(alias, spec)
        (local if name in workspace_names else external).append(name)
    return sorted(set(local)), sorted(set(external))


def load_metadata(root: Path) -> dict[str, Any]:
    proc = subprocess.run(
        ["cargo", "metadata", "--locked", "--offline", "--format-version", "1"],
        cwd=root,
        check=False,
        capture_output=True,
        text=True,
    )
    if proc.returncode != 0:
        fail("cargo metadata failed: " + proc.stderr.strip())
    try:
        return json.loads(proc.stdout)
    except json.JSONDecodeError as exc:
        fail(f"cargo metadata emitted invalid JSON: {exc}")


def canonical_package_id(package: dict[str, Any], workspace_ids: set[str]) -> str:
    package_id = package["id"]
    if package_id in workspace_ids:
        return f"workspace:{package['name']}@{package['version']}"
    return package_id


def production_closure(
    metadata: dict[str, Any], root_name: str
) -> tuple[set[str], set[str], int, str]:
    workspace_ids = set(metadata.get("workspace_members", []))
    packages = {row["id"]: row for row in metadata.get("packages", [])}
    nodes = {row["id"]: row for row in metadata.get("resolve", {}).get("nodes", [])}
    roots = [pid for pid in workspace_ids if packages.get(pid, {}).get("name") == root_name]
    if len(roots) != 1:
        fail(f"metadata does not identify exactly one workspace package {root_name}")
    seen: set[str] = set()
    pending = roots[:]
    while pending:
        package_id = pending.pop()
        if package_id in seen:
            continue
        seen.add(package_id)
        node = nodes.get(package_id)
        if node is None:
            fail(f"metadata resolve node missing: {package_id}")
        for dep in node.get("deps", []):
            kinds = dep.get("dep_kinds", [])
            if any(item.get("kind") != "dev" for item in kinds):
                pending.append(dep["pkg"])
    workspace = {packages[pid]["name"] for pid in seen if pid in workspace_ids}
    all_names = {packages[pid]["name"] for pid in seen}
    canonical: list[dict[str, Any]] = []
    for package_id in sorted(seen, key=lambda pid: canonical_package_id(packages[pid], workspace_ids)):
        package = packages[package_id]
        node = nodes[package_id]
        deps: list[dict[str, Any]] = []
        for dep in node.get("deps", []):
            kinds = [
                {"kind": item.get("kind"), "target": item.get("target")}
                for item in dep.get("dep_kinds", [])
                if item.get("kind") != "dev"
            ]
            if kinds:
                deps.append(
                    {
                        "kinds": sorted(kinds, key=lambda item: (str(item["kind"]), str(item["target"]))),
                        "name": dep["name"],
                        "package": canonical_package_id(packages[dep["pkg"]], workspace_ids),
                    }
                )
        canonical.append(
            {
                "dependencies": sorted(deps, key=lambda item: (item["name"], item["package"])),
                "edition": package.get("edition"),
                "features": sorted(node.get("features", [])),
                "id": canonical_package_id(package, workspace_ids),
                "links": package.get("links"),
                "rustVersion": package.get("rust_version"),
            }
        )
    raw = json.dumps(canonical, sort_keys=True, separators=(",", ":"), ensure_ascii=True)
    return workspace, all_names, len(canonical), sha256(raw.encode("utf-8"))


def refresh_dynamic_bindings(
    root: Path, contract: dict[str, Any], metadata: dict[str, Any]
) -> None:
    """Refresh exact Cargo-derived facts without changing reviewed boundary policy."""
    manifests = workspace_manifests(root)
    workspace_names = set(manifests)
    rows = contract.get("stage0Packages")
    if not isinstance(rows, list) or [row.get("name") for row in rows if isinstance(row, dict)] != list(STAGE0_PACKAGES):
        fail("cannot refresh: stage0 package order or inventory drift")

    contract["forbiddenWorkspacePackages"] = sorted(workspace_names - set(STAGE0_PACKAGES))
    for row in rows:
        name = row["name"]
        manifest_path = manifests[name]
        manifest = tomllib.loads(manifest_path.read_text(encoding="utf-8"))
        row["manifestPath"] = manifest_path.relative_to(root).as_posix()
        row["manifestSha256"] = sha256(manifest_path.read_bytes())
        for kind, local_field, external_field in (
            ("production", "allowedProductionWorkspaceDependencies", "allowedProductionExternalDependencies"),
            ("build", "allowedBuildWorkspaceDependencies", "allowedBuildExternalDependencies"),
            ("dev", "allowedDevWorkspaceDependencies", "allowedDevExternalDependencies"),
        ):
            local, external = partition_dependencies(dependency_sections(manifest, kind), workspace_names)
            row[local_field] = local
            row[external_field] = external

        features = manifest.get("features", {})
        if not isinstance(features, dict):
            fail(f"cannot refresh: {name} features must be an object")
        normalized_features: dict[str, list[str]] = {}
        for key, items in sorted(features.items()):
            if not isinstance(key, str) or not isinstance(items, list) or not all(
                isinstance(item, str) and item for item in items
            ):
                fail(f"cannot refresh: {name}.features.{key} must be a string array")
            normalized_features[key] = sorted(set(items))
        row["allowedFeatures"] = normalized_features

        workspace_closure, _package_closure, package_count, closure_sha = production_closure(metadata, name)
        row["productionWorkspaceClosure"] = sorted(workspace_closure)
        row["productionResolvedPackageCount"] = package_count
        row["productionResolvedClosureSha256"] = closure_sha


def validate_sources(
    root: Path,
    packages: list[dict[str, Any]],
    forbidden_workspace: set[str],
    escapes: set[tuple[str, str]],
    overrides: dict[str, str] | None,
) -> None:
    observed_escapes: set[tuple[str, str]] = set()
    forbidden_re = re.compile(
        r"(?<![A-Za-z0-9_])(" + "|".join(re.escape(name) for name in sorted(forbidden_workspace)) + r")\s*::"
    )
    for package in packages:
        source_root_path = root / package["sourceRoot"]
        if source_root_path.is_symlink() or not source_root_path.is_dir():
            fail(f"source root missing: {package['sourceRoot']}")
        source_root = source_root_path.resolve()
        paths = list(source_root.rglob("*.rs"))
        build_script = package.get("buildScriptPath")
        if build_script is not None:
            build_path = root / build_script
            if build_path.is_symlink() or not build_path.is_file():
                fail(f"stage0 build script must be a regular non-symlink file: {build_script}")
            paths.append(build_path)
        for path in sorted(paths):
            if path.is_symlink() or not path.is_file():
                fail(f"stage0 source must be a regular non-symlink file: {path}")
            rel = path.relative_to(root).as_posix()
            text = overrides.get(rel, path.read_text(encoding="utf-8")) if overrides else path.read_text(encoding="utf-8")
            match = forbidden_re.search(text)
            if match:
                fail(f"stage0 production source imports forbidden crate {match.group(1)}: {rel}")
            for raw_line in text.splitlines():
                line = raw_line.strip()
                if INCLUDE_RE.search(line):
                    observed_escapes.add((rel, line))
            for target in PATH_RE.findall(text):
                resolved = (path.parent / target).resolve()
                try:
                    resolved.relative_to(source_root if path.is_relative_to(source_root) else path.parent.resolve())
                except ValueError:
                    fail(f"stage0 #[path] escapes source root: {rel}: {target}")
    if observed_escapes != escapes:
        fail(
            "stage0 source escape drift: missing="
            f"{sorted(escapes - observed_escapes)}, unknown={sorted(observed_escapes - escapes)}"
        )


def validate(
    root: Path,
    contract: Any,
    schema: Any,
    spec_bytes: bytes,
    schema_bytes: bytes,
    stage0: Any,
    metadata: dict[str, Any],
    source_overrides: dict[str, str] | None = None,
) -> None:
    value = closed(contract, ROOT_FIELDS, "contract")
    validate_schema(schema)
    expected = {
        "kind": "genesis/stage0-dependency-boundary-v0.1",
        "version": "0.1",
        "canonicalSpec": "docs/spec/STAGE0_DEPENDENCY_BOUNDARY_v0.1.md",
        "schema": "docs/spec/STAGE0_DEPENDENCY_BOUNDARY_v0.1.schema.json",
        "stage0TrustContract": "docs/spec/STAGE0_TRUST_CONTRACT_v0.1.json",
    }
    for field, expected_value in expected.items():
        if value.get(field) != expected_value:
            fail(f"{field} drift")
    if not re.fullmatch(r"\d{4}-\d{2}-\d{2}", str(value.get("auditDate", ""))):
        fail("auditDate must use YYYY-MM-DD")
    if value.get("canonicalSpecSha256") != sha256(spec_bytes):
        fail("canonical prose identity mismatch")
    if value.get("schemaSha256") != sha256(schema_bytes):
        fail("schema identity mismatch")
    if value.get("contentIdentitySha256") != identity(value):
        fail("content identity mismatch")
    if not isinstance(stage0, dict) or stage0.get("contentIdentitySha256") != EXPECTED_STAGE0_IDENTITY:
        fail("bound stage0 trust contract identity drift")
    if value.get("stage0TrustContractIdentitySha256") != EXPECTED_STAGE0_IDENTITY:
        fail("stage0 trust binding drift")
    if tuple(strings(value.get("nonclaims"), "nonclaims")) != tuple(sorted(NONCLAIMS)):
        fail("nonclaims drift")

    manifests = workspace_manifests(root)
    workspace_names = set(manifests)
    forbidden_workspace = set(strings(value.get("forbiddenWorkspacePackages"), "forbiddenWorkspacePackages"))
    if forbidden_workspace != workspace_names - set(STAGE0_PACKAGES):
        fail("forbidden workspace inventory must equal every non-stage0 workspace package")
    ambient = set(strings(value.get("forbiddenAmbientPackages"), "forbiddenAmbientPackages"))
    if not REQUIRED_AMBIENT_DENYLIST.issubset(ambient):
        fail("ambient dependency denylist was weakened")

    rows_raw = value.get("stage0Packages")
    if not isinstance(rows_raw, list) or len(rows_raw) != len(STAGE0_PACKAGES):
        fail("stage0Packages must contain exactly three rows")
    rows = [closed(row, PACKAGE_FIELDS, "stage0 package") for row in rows_raw]
    if [row.get("name") for row in rows] != list(STAGE0_PACKAGES):
        fail("stage0 package order or inventory drift")

    stage0_domains = {row.get("id") for row in stage0.get("stage0Domains", [])}
    for row in rows:
        name = row["name"]
        if tuple(strings(row.get("domains"), f"{name}.domains")) != EXPECTED_DOMAINS[name]:
            fail(f"{name} domain binding drift")
        if not set(row["domains"]).issubset(stage0_domains):
            fail(f"{name} references unknown stage0 domain")
        manifest_path = relative_file(root, row.get("manifestPath"), f"{name}.manifestPath")
        if manifest_path.resolve() != manifests[name].resolve():
            fail(f"{name} manifest binding drift")
        if row.get("manifestSha256") != sha256(manifest_path.read_bytes()):
            fail(f"{name} manifest identity drift")
        build_script = row.get("buildScriptPath")
        if build_script is not None:
            build_path = relative_file(root, build_script, f"{name}.buildScriptPath")
            if build_path.parent != manifest_path.parent or build_path.name != "build.rs":
                fail(f"{name} build script binding drift")
        manifest = tomllib.loads(manifest_path.read_text(encoding="utf-8"))
        if manifest.get("package", {}).get("name") != name:
            fail(f"{name} manifest package name drift")
        for kind, local_field, external_field in (
            ("production", "allowedProductionWorkspaceDependencies", "allowedProductionExternalDependencies"),
            ("build", "allowedBuildWorkspaceDependencies", "allowedBuildExternalDependencies"),
            ("dev", "allowedDevWorkspaceDependencies", "allowedDevExternalDependencies"),
        ):
            local, external = partition_dependencies(dependency_sections(manifest, kind), workspace_names)
            if local != strings(row.get(local_field), f"{name}.{local_field}"):
                fail(f"{name} {kind} workspace dependency drift: observed={local}")
            if external != strings(row.get(external_field), f"{name}.{external_field}"):
                fail(f"{name} {kind} external dependency drift: observed={external}")
        features = manifest.get("features", {})
        if not isinstance(features, dict):
            fail(f"{name} features must be an object")
        normalized_features = {
            key: strings(items, f"{name}.features.{key}") for key, items in sorted(features.items())
        }
        expected_features = row.get("allowedFeatures")
        if not isinstance(expected_features, dict):
            fail(f"{name}.allowedFeatures must be an object")
        normalized_expected = {
            key: strings(items, f"{name}.allowedFeatures.{key}")
            for key, items in sorted(expected_features.items())
        }
        if normalized_features != normalized_expected:
            fail(f"{name} feature definition drift")

        workspace_closure, package_closure, package_count, closure_sha = production_closure(metadata, name)
        expected_closure = set(strings(row.get("productionWorkspaceClosure"), f"{name}.productionWorkspaceClosure"))
        if workspace_closure != expected_closure:
            fail(f"{name} resolved workspace closure drift: observed={sorted(workspace_closure)}")
        if row.get("productionResolvedPackageCount") != package_count:
            fail(f"{name} resolved package count drift: observed={package_count}")
        if row.get("productionResolvedClosureSha256") != closure_sha:
            fail(f"{name} resolved production closure identity drift: observed={closure_sha}")
        forbidden_reachable = sorted(workspace_closure & forbidden_workspace)
        if forbidden_reachable:
            fail(f"{name} reaches forbidden workspace packages: {forbidden_reachable}")
        ambient_reachable = sorted(package_closure & ambient)
        if ambient_reachable:
            fail(f"{name} reaches ambient packages: {ambient_reachable}")

    escapes_raw = value.get("sourceEscapes")
    if not isinstance(escapes_raw, list):
        fail("sourceEscapes must be an array")
    escapes: list[tuple[str, str]] = []
    for item in escapes_raw:
        row = closed(item, ESCAPE_FIELDS, "source escape")
        path = relative_file(root, row.get("path"), "source escape path")
        if not isinstance(row.get("line"), str) or not row["line"].strip():
            fail("source escape line missing")
        if not isinstance(row.get("reason"), str) or not row["reason"].strip():
            fail("source escape reason missing")
        escapes.append((path.relative_to(root).as_posix(), row["line"]))
    if escapes != sorted(set(escapes)):
        fail("sourceEscapes must be sorted and unique by path/line")
    validate_sources(root, rows, forbidden_workspace, set(escapes), source_overrides)

    spec_text = spec_bytes.decode("utf-8")
    missing_markers = [marker for marker in SPEC_MARKERS if marker not in spec_text]
    if missing_markers:
        fail(f"normative prose markers missing: {missing_markers}")


def remove_prose_marker(
    _doc: dict[str, Any],
    _schema: dict[str, Any],
    prose: bytearray,
    _trust: dict[str, Any],
    _metadata: dict[str, Any],
) -> None:
    marker = b"permitted workspace graph is exact and acyclic"
    if prose.count(marker) != 1:
        fail("self-test prose marker drift")
    prose[:] = prose.replace(marker, b"workspace graph wording removed", 1)


def run_self_tests(
    root: Path,
    contract: dict[str, Any],
    schema: dict[str, Any],
    spec: bytes,
    schema_bytes: bytes,
    stage0: dict[str, Any],
    metadata: dict[str, Any],
) -> int:
    mutations: list[tuple[str, Callable[[dict[str, Any], dict[str, Any], bytearray, dict[str, Any], dict[str, Any]], None]]] = [
        ("unknown-root", lambda d, s, p, t, m: d.__setitem__("unknown", True)),
        ("package-removal", lambda d, s, p, t, m: d["stage0Packages"].pop()),
        ("workspace-denylist-loss", lambda d, s, p, t, m: d["forbiddenWorkspacePackages"].pop()),
        ("ambient-denylist-loss", lambda d, s, p, t, m: d["forbiddenAmbientPackages"].remove("tokio")),
        ("production-edge-broadening", lambda d, s, p, t, m: d["stage0Packages"][1]["allowedProductionWorkspaceDependencies"].append("gc_effects")),
        ("dev-edge-promotion", lambda d, s, p, t, m: d["stage0Packages"][2]["allowedProductionWorkspaceDependencies"].append("gc_opt")),
        ("external-edge-broadening", lambda d, s, p, t, m: d["stage0Packages"][0]["allowedProductionExternalDependencies"].append("tokio")),
        ("feature-broadening", lambda d, s, p, t, m: d["stage0Packages"][2]["allowedFeatures"]["embedded-bootstrap"].append("dep:gc_effects")),
        ("manifest-identity", lambda d, s, p, t, m: d["stage0Packages"][0].__setitem__("manifestSha256", "f" * 64)),
        ("resolved-closure-identity", lambda d, s, p, t, m: d["stage0Packages"][0].__setitem__("productionResolvedClosureSha256", "f" * 64)),
        ("resolved-package-count", lambda d, s, p, t, m: d["stage0Packages"][0].__setitem__("productionResolvedPackageCount", 1)),
        ("source-escape-loss", lambda d, s, p, t, m: d["sourceEscapes"].pop()),
        ("stage0-binding", lambda d, s, p, t, m: d.__setitem__("stage0TrustContractIdentitySha256", "f" * 64)),
        ("prose-drift", remove_prose_marker),
        ("schema-field", lambda d, s, p, t, m: s.__setitem__("unknown", True)),
        ("resolved-forbidden-edge", add_forbidden_metadata_edge),
    ]
    passed = 0
    for name, mutate in mutations:
        doc = copy.deepcopy(contract)
        schema_doc = copy.deepcopy(schema)
        prose = bytearray(spec)
        trust = copy.deepcopy(stage0)
        meta = copy.deepcopy(metadata)
        mutate(doc, schema_doc, prose, trust, meta)
        schema_raw = json.dumps(schema_doc, indent=2, sort_keys=True).encode("utf-8") + b"\n"
        reseal(doc, bytes(prose), schema_raw)
        try:
            validate(root, doc, schema_doc, bytes(prose), schema_raw, trust, meta)
        except BoundaryError:
            passed += 1
        else:
            fail(f"self-test accepted mutation: {name}")

    source_cases = {
        "forbidden-source-import": "\nuse gc_effects::run;\n",
        "undeclared-include": "\nconst X: &str = include_str!(\"missing\");\n",
        "path-escape": "\n#[path = \"../../../gc_effects/src/lib.rs\"]\nmod escaped;\n",
    }
    target = "crates/gc_coreform/src/lib.rs"
    original = (root / target).read_text(encoding="utf-8")
    for name, suffix in source_cases.items():
        try:
            validate(root, contract, schema, spec, schema_bytes, stage0, metadata, {target: original + suffix})
        except BoundaryError:
            passed += 1
        else:
            fail(f"self-test accepted mutation: {name}")

    duplicate = json.dumps(contract).replace('{"auditDate"', '{"auditDate":"2000-01-01","auditDate"', 1)
    try:
        json.loads(duplicate, object_pairs_hook=no_duplicate_pairs)
    except BoundaryError:
        passed += 1
    else:
        fail("self-test accepted duplicate JSON key")

    bad_identity = copy.deepcopy(contract)
    bad_identity["contentIdentitySha256"] = "0" * 64
    try:
        validate(root, bad_identity, schema, spec, schema_bytes, stage0, metadata)
    except BoundaryError:
        passed += 1
    else:
        fail("self-test accepted content identity drift")
    return passed


def add_forbidden_metadata_edge(
    _doc: dict[str, Any],
    _schema: dict[str, Any],
    _prose: bytearray,
    _trust: dict[str, Any],
    metadata: dict[str, Any],
) -> None:
    workspace = set(metadata["workspace_members"])
    packages = {row["id"]: row for row in metadata["packages"]}
    ids = {packages[pid]["name"]: pid for pid in workspace}
    nodes = {row["id"]: row for row in metadata["resolve"]["nodes"]}
    nodes[ids["gc_kernel"]]["deps"].append(
        {
            "name": "gc_effects",
            "pkg": ids["gc_effects"],
            "dep_kinds": [{"kind": None, "target": None}],
        }
    )


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--root", type=Path, default=Path.cwd())
    parser.add_argument("--contract", default="docs/spec/STAGE0_DEPENDENCY_BOUNDARY_v0.1.json")
    parser.add_argument("--schema", default="docs/spec/STAGE0_DEPENDENCY_BOUNDARY_v0.1.schema.json")
    parser.add_argument("--spec", default="docs/spec/STAGE0_DEPENDENCY_BOUNDARY_v0.1.md")
    parser.add_argument("--stage0", default="docs/spec/STAGE0_TRUST_CONTRACT_v0.1.json")
    parser.add_argument("--self-test", action="store_true")
    parser.add_argument("--refresh-identities", action="store_true")
    args = parser.parse_args()
    root = args.root.resolve()
    try:
        contract = load_json(root / args.contract)
        schema = load_json(root / args.schema)
        stage0 = load_json(root / args.stage0)
        spec = (root / args.spec).read_bytes()
        schema_bytes = (root / args.schema).read_bytes()
        metadata = load_metadata(root)
        if args.refresh_identities:
            refresh_dynamic_bindings(root, contract, metadata)
            contract["stage0TrustContractIdentitySha256"] = stage0[
                "contentIdentitySha256"
            ]
            reseal(contract, spec, schema_bytes)
            validate(root, contract, schema, spec, schema_bytes, stage0, metadata)
            (root / args.contract).write_text(
                json.dumps(contract, indent=2, ensure_ascii=True) + "\n",
                encoding="utf-8",
            )
            print(f"stage0-dependency-boundary: refreshed {args.contract}")
            return 0
        validate(root, contract, schema, spec, schema_bytes, stage0, metadata)
        controls = run_self_tests(root, contract, schema, spec, schema_bytes, stage0, metadata) if args.self_test else 0
        print(
            "stage0-dependency-boundary: ok "
            f"identity={contract['contentIdentitySha256']} packages={len(contract['stage0Packages'])} "
            f"source_escapes={len(contract['sourceEscapes'])} controls={controls}"
        )
    except (BoundaryError, OSError) as exc:
        print(f"stage0-dependency-boundary: {exc}")
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
