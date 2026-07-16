#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

REPORT_OUT="${1:?usage: scripts/render_cargo_target_dir_policy_report.sh <report.json> <history.jsonl>}"
HISTORY_OUT="${2:?usage: scripts/render_cargo_target_dir_policy_report.sh <report.json> <history.jsonl>}"

python3 - "$ROOT_DIR" "$REPORT_OUT" "$HISTORY_OUT" <<'PY'
import copy
import json
import os
import pathlib
import re
import shutil
import subprocess
import sys
import tempfile
import time

root = pathlib.Path(sys.argv[1]).resolve()
report_path = pathlib.Path(sys.argv[2])
history_path = pathlib.Path(sys.argv[3])
sys.path.insert(0, str(root / "scripts/lib"))
import cargo_cache as cache  # noqa: E402
import generated_state as state  # noqa: E402


class ControlFailure(AssertionError):
    pass


def require(condition, message):
    if not condition:
        raise ControlFailure(message)


def key(result):
    return result["metadata"]["cacheKeySha256"]


def mock_env(cache_root):
    env = dict(os.environ)
    env.update(
        {
            "GENESIS_CARGO_CACHE_ROOT": str(cache_root),
            "GENESIS_CARGO_CACHE_RUSTC_IDENTITY_JSON": json.dumps(
                {
                    "release": "1.90.0",
                    "commit-hash": "0" * 40,
                    "host": "test-host-triple",
                },
                sort_keys=True,
            ),
        }
    )
    for name in cache.load_policy(root)["buildEnvironment"]:
        env.pop(name, None)
    return env


policy = cache.load_policy(root)
schema = cache.load_json(root / cache.SCHEMA_REL)
require(
    isinstance(schema, dict)
    and schema.get("$schema") == "https://json-schema.org/draft/2020-12/schema"
    and schema.get("$id") == "https://genesiscode.dev/schemas/cargo-cache-policy-v0.1.json"
    and schema.get("additionalProperties") is False,
    "schema identity or closure mismatch",
)
require(set(schema.get("required", [])) == cache.EXPECTED_POLICY_FIELDS, "schema required fields drift")

controls = []


def passed(control_id):
    controls.append(control_id)


storage_gate = (root / "scripts/check_evidence_storage_classes.sh").read_text(encoding="utf-8")
release_renderer = (root / "scripts/render_evidence_release_asset.sh").read_text(encoding="utf-8")
require(
    "genesis_configure_cargo_target_dir" not in storage_gate
    and '"evidence-release-asset"' in release_renderer
    and "evidence-verifier-host" in release_renderer,
    "Python-only parent gate must not reserve root-host around nested verifier builds",
)
passed("nested-verifier-scope-reservation")


configuration_paths = {
    root / cache.POLICY_REL,
    root / cache.SCHEMA_REL,
    root / "rust-toolchain.toml",
    root / "policies/deterministic_cleanup_v0.1.json",
    root / "policies/generated_state_v0.1.json",
    root / ".cargo/config.toml",
    root / "Cargo.lock",
    root / "tools/genesis-evidence-verifier/Cargo.lock",
    root / "scripts/lib/cargo_cache.py",
    root / "scripts/lib/cargo_target_dir.sh",
    root / "scripts/lib/deterministic_cleanup.py",
    root / "scripts/lib/generated_state.py",
    root / "scripts/check_evidence_storage_classes.sh",
    root / "scripts/render_evidence_release_asset.sh",
}
for scope in policy["scopes"]:
    for pattern in scope["manifestGlobs"]:
        configuration_paths.update(root.glob(pattern))
for path in configuration_paths:
    require(path.is_file(), f"missing configuration authority: {path}")

with tempfile.TemporaryDirectory(prefix="genesis-cargo-cache-policy.") as temp_raw:
    temp = pathlib.Path(temp_raw)
    fixture = temp / "repo"
    for source in sorted(configuration_paths):
        destination = fixture / source.relative_to(root)
        destination.parent.mkdir(parents=True, exist_ok=True)
        shutil.copy2(source, destination)
    source_fixture = fixture / "crates/gc_coreform/src/lib.rs"
    source_fixture.parent.mkdir(parents=True, exist_ok=True)
    source_fixture.write_text("pub fn source_only_fixture() {}\n", encoding="utf-8")
    subprocess.run(["git", "init", "-q"], cwd=fixture, check=True)

    fresh_env = mock_env(fixture / ".genesis/build/cargo-cache/v1")
    fresh = subprocess.run(
        [
            sys.executable,
            str(fixture / "scripts/lib/cargo_cache.py"),
            "--root",
            str(fixture),
            "--scope",
            "root-host",
            "--format",
            "json",
        ],
        cwd=fixture,
        env=fresh_env,
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
    )
    require(fresh.returncode == 0, f"fresh-checkout cache admission failed: {fresh.stderr}")
    fresh_result = json.loads(fresh.stdout)
    cleanup_marker = cache.load_json(
        fixture / "policies/deterministic_cleanup_v0.1.json"
    )["markerFile"]
    require(
        pathlib.Path(fresh_result["targetDir"]).is_dir()
        and (fixture / ".genesis/build" / cleanup_marker).is_file(),
        "fresh-checkout cache admission omitted materialization or provenance",
    )
    require(
        state.status(fixture)["activeLeases"] == 0,
        "non-shell cache resolution leaked a process lease",
    )
    passed("fresh-checkout-materialization")

    slim_env = dict(fresh_env)
    slim_env["CARGO_INCREMENTAL"] = "0"
    slim_env["CARGO_PROFILE_DEV_DEBUG"] = "0"
    slim = subprocess.run(
        [
            sys.executable,
            str(fixture / "scripts/lib/cargo_cache.py"),
            "--root",
            str(fixture),
            "--scope",
            "root-host",
            "--format",
            "json",
        ],
        cwd=fixture,
        env=slim_env,
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
    )
    require(slim.returncode == 0, f"slim host cache admission failed: {slim.stderr}")
    slim_result = json.loads(slim.stdout)
    registry = cache.load_json(
        fixture / ".genesis/build/.generated-state-v0.1/registry.json"
    )
    cache_entries = [
        entry for entry in registry["entries"] if entry["owner"] == "cargo-cache"
    ]
    require(
        pathlib.Path(slim_result["targetDir"]).is_dir()
        and {entry["sizeClass"] for entry in cache_entries}
        == {"cargo-host", "cargo-host-slim"}
        and len(cache_entries) == 2,
        "normal and slim host cache profiles did not coexist under the soft quota",
    )
    passed("host-profile-co-residency")

    env = mock_env(temp / "cache-a")
    baseline = cache.resolve(fixture, "root-host", env)
    repeated_env = dict(env)
    repeated_env["GENESIS_TEST_CALLER_CONTEXT"] = "different-script-name"
    repeated = cache.resolve(fixture, "root-host", repeated_env)
    require(key(baseline) == key(repeated), "caller context changed cache key")
    passed("context-convergence")

    relocated_env = dict(env)
    relocated_env["GENESIS_CARGO_CACHE_ROOT"] = str(temp / "cache-b")
    relocated = cache.resolve(fixture, "root-host", relocated_env)
    require(key(baseline) == key(relocated), "cache root relocation changed key")
    require(baseline["target_dir"] != relocated["target_dir"], "cache root relocation did not relocate output")
    passed("root-relocation-key-stability")

    source_fixture.write_text("pub fn source_only_fixture() { let _ = 1; }\n", encoding="utf-8")
    source_changed = cache.resolve(fixture, "root-host", env)
    require(key(source_changed) == key(baseline), "source-only edit rotated target directory")
    passed("source-edit-incremental-stability")

    lock_path = fixture / "Cargo.lock"
    original_lock = lock_path.read_text(encoding="utf-8")
    lock_path.write_text(original_lock + "\n# cache-key-negative-control\n", encoding="utf-8")
    lock_changed = cache.resolve(fixture, "root-host", env)
    require(key(lock_changed) != key(baseline), "lockfile change did not rotate key")
    lock_path.write_text(original_lock, encoding="utf-8")
    passed("lockfile-sensitivity")

    toolchain_path = fixture / "rust-toolchain.toml"
    original_toolchain = toolchain_path.read_text(encoding="utf-8")
    toolchain_path.write_text(original_toolchain + "\n# cache-key-negative-control\n", encoding="utf-8")
    toolchain_changed = cache.resolve(fixture, "root-host", env)
    require(key(toolchain_changed) != key(baseline), "toolchain file change did not rotate key")
    toolchain_path.write_text(original_toolchain, encoding="utf-8")
    passed("toolchain-file-sensitivity")

    rustc_env = dict(env)
    rustc_env["GENESIS_CARGO_CACHE_RUSTC_IDENTITY_JSON"] = json.dumps(
        {"release": "1.90.1", "commit-hash": "1" * 40, "host": "test-host-triple"},
        sort_keys=True,
    )
    rustc_changed = cache.resolve(fixture, "root-host", rustc_env)
    require(key(rustc_changed) != key(baseline), "observed rustc change did not rotate key")
    passed("observed-toolchain-sensitivity")

    feature_manifest = fixture / "crates/gc_cli/Cargo.toml"
    original_manifest = feature_manifest.read_text(encoding="utf-8")
    require("[features]\n" in original_manifest, "feature mutation fixture missing [features]")
    feature_manifest.write_text(
        original_manifest.replace("[features]\n", "[features]\ncache-policy-negative-control = []\n", 1),
        encoding="utf-8",
    )
    feature_changed = cache.resolve(fixture, "root-host", env)
    require(key(feature_changed) != key(baseline), "feature definition change did not rotate key")
    feature_manifest.write_text(original_manifest, encoding="utf-8")
    passed("feature-definition-sensitivity")

    flags_env = dict(env)
    flags_env["RUSTFLAGS"] = "-Cdebuginfo=1"
    flags_changed = cache.resolve(fixture, "root-host", flags_env)
    require(key(flags_changed) != key(baseline), "RUSTFLAGS change did not rotate key")
    passed("build-environment-sensitivity")

    wasi = cache.resolve(fixture, "root-wasi", env)
    wasm = cache.resolve(fixture, "root-wasm", env)
    verifier = cache.resolve(fixture, "evidence-verifier-host", env)
    require(
        len({key(baseline), key(wasi), key(wasm), key(verifier)}) == 4,
        "declared scopes collided",
    )
    passed("workspace-target-separation")

    metadata_text = json.dumps(baseline["metadata"], sort_keys=True)
    require(str(fixture) not in metadata_text and str(temp) not in metadata_text, "cache metadata leaked a host path")
    require(re.search(r"(?:/Users/|/home/|/private/|[A-Za-z]:\\\\)", metadata_text) is None, "cache metadata contains an absolute host path")
    passed("host-path-exclusion")

    cache.materialize(baseline)
    metadata_path = pathlib.Path(baseline["target_dir"]) / baseline["metadata_file"]
    require(metadata_path.is_file(), "materialization metadata was not written")
    cache.materialize(baseline)
    metadata_path.write_text("{}\n", encoding="utf-8")
    try:
        cache.materialize(baseline)
    except cache.CachePolicyError:
        pass
    else:
        raise ControlFailure("tampered materialization metadata was accepted")
    passed("metadata-tamper-rejection")

    unknown_policy = copy.deepcopy(policy)
    unknown_policy["unknown"] = True
    unknown_path = temp / "unknown-policy.json"
    unknown_path.write_text(json.dumps(unknown_policy), encoding="utf-8")
    try:
        cache.load_policy(fixture, unknown_path)
    except cache.CachePolicyError:
        pass
    else:
        raise ControlFailure("unknown policy field was accepted")
    passed("unknown-policy-field-rejection")

    duplicate_path = temp / "duplicate-policy.json"
    duplicate_path.write_text('{"kind":"a","kind":"b"}\n', encoding="utf-8")
    try:
        cache.load_policy(fixture, duplicate_path)
    except cache.CachePolicyError:
        pass
    else:
        raise ControlFailure("duplicate policy key was accepted")
    passed("duplicate-policy-key-rejection")

    try:
        cache.resolve(fixture, "unknown-scope", env)
    except cache.CachePolicyError:
        pass
    else:
        raise ControlFailure("undeclared scope was accepted")
    passed("undeclared-scope-rejection")

    helper = fixture / "scripts/lib/cargo_target_dir.sh"
    shell_base = ["bash", "-c", 'source "$1"; genesis_configure_cargo_target_dir "$2" policy-test root-host', "bash", str(helper), str(fixture)]
    legacy_env = dict(env)
    legacy_env["GENESIS_CARGO_TARGET_DIR"] = str(temp / "legacy")
    proc = subprocess.run(shell_base, env=legacy_env, text=True, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
    require(proc.returncode != 0 and "is retired" in proc.stderr, "legacy target override was accepted")
    passed("legacy-override-rejection")

    inherited_env = dict(env)
    # The enclosing health profile may itself use a resolved shared cache. Strip
    # that provenance so this control actually models an arbitrary caller.
    for key in tuple(inherited_env):
        if key.startswith("GENESIS_CARGO_CACHE_"):
            inherited_env.pop(key)
    inherited_env["CARGO_TARGET_DIR"] = str(temp / "arbitrary")
    proc = subprocess.run(shell_base, env=inherited_env, text=True, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
    require(proc.returncode != 0 and "arbitrary inherited" in proc.stderr, "arbitrary inherited target was accepted")
    passed("arbitrary-inherited-target-rejection")

    clear_base = ["bash", "-c", 'source "$1"; genesis_clear_resolved_cargo_target_dir policy-test', "bash", str(helper)]
    proc = subprocess.run(clear_base, env=inherited_env, text=True, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
    require(proc.returncode != 0 and "refusing to clear arbitrary inherited" in proc.stderr, "arbitrary target was cleared")
    passed("arbitrary-clear-rejection")

    transition = [
        "bash",
        "-c",
        'source "$1"; genesis_configure_cargo_target_dir "$2" first root-host >/dev/null; first="$CARGO_TARGET_DIR"; '
        'export CARGO_INCREMENTAL=0; genesis_clear_resolved_cargo_target_dir transition; '
        'genesis_configure_cargo_target_dir "$2" second root-host >/dev/null; test "$first" != "$CARGO_TARGET_DIR"',
        "bash",
        str(helper),
        str(fixture),
    ]
    transition_env = dict(env)
    for key in (
        "CARGO_TARGET_DIR",
        "GENESIS_CARGO_CACHE_RESOLVED",
        "GENESIS_CARGO_CACHE_SCOPE",
        "GENESIS_CARGO_CACHE_KEY_SHA256",
        "GENESIS_CARGO_CACHE_HIT",
    ):
        transition_env.pop(key, None)
    proc = subprocess.run(transition, env=transition_env, text=True, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
    require(proc.returncode == 0, f"declared environment transition failed: {proc.stderr}")
    passed("resolved-environment-transition")

    # Keep the lease control independent from reservation entries created by
    # earlier key-rotation controls in this synthetic repository.
    shutil.rmtree(fixture / ".genesis/build", ignore_errors=True)
    github_env_proc = subprocess.run(
        [
            sys.executable,
            str(fixture / "scripts/lib/cargo_cache.py"),
            "--root", str(fixture),
            "--scope", "root-host",
            "--format", "github-env",
        ],
        cwd=fixture,
        env=fresh_env,
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
    )
    require(
        github_env_proc.returncode == 0,
        f"github-env fixture failed: {github_env_proc.stderr}",
    )
    github_env = dict(fresh_env)
    for line in github_env_proc.stdout.splitlines():
        name, value = line.split("=", 1)
        github_env[name] = value
    github_resolved = cache.resolve(fixture, "root-host", github_env)
    require(
        str(github_resolved["target_dir"]) == github_env["CARGO_TARGET_DIR"],
        "GitHub environment provenance changed the resolved cache target",
    )
    lease_contract_script = r'''
set -euo pipefail
source "$1"
genesis_configure_cargo_target_dir "$2" outer root-host >/dev/null
outer_token="$GENESIS_GENERATED_STATE_LEASE_TOKEN"
outer_target="$CARGO_TARGET_DIR"
canonical_root="$(cd "$2" && pwd -P)"
outer_relative="${outer_target#"$canonical_root"/}"
python3 "$2/scripts/lib/generated_state.py" \
  --root "$2" validate-lease \
  --token "$outer_token" \
  --path "$outer_relative" >/dev/null
if python3 "$2/scripts/lib/generated_state.py" \
  --root "$2" validate-lease \
  --token "$outer_token" \
  --path ".genesis/build/not-the-leased-cache" >/dev/null 2>&1; then
  exit 41
fi
if [[ "${outer_token:0:1}" == "0" ]]; then
  unknown_token="1${outer_token:1}"
else
  unknown_token="0${outer_token:1}"
fi
if python3 "$2/scripts/lib/generated_state.py" \
  --root "$2" validate-lease \
  --token "$unknown_token" \
  --path "$outer_relative" >/dev/null 2>&1; then
  exit 42
fi
bash -c '
  set -euo pipefail
  source "$1"
  genesis_configure_cargo_target_dir "$2" nested root-host >/dev/null
  test "$GENESIS_GENERATED_STATE_LEASE_TOKEN" = "$3"
  test "$CARGO_TARGET_DIR" = "$4"
' bash "$1" "$2" "$outer_token" "$outer_target"
python3 "$2/scripts/lib/generated_state.py" \
  --root "$2" status --format json >"$2/lease-status.json"
genesis_clear_resolved_cargo_target_dir outer-exit
'''
    proc = subprocess.run(
        ["bash", "-c", lease_contract_script, "bash", str(helper), str(fixture)],
        env=github_env,
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
    )
    registry_path = fixture / ".genesis/build/.generated-state-v0.1/registry.json"
    registry_debug = registry_path.read_text(encoding="utf-8") if registry_path.is_file() else "<absent>"
    require(
        proc.returncode == 0,
        f"nested lease reuse failed: {proc.stderr}\nregistry:\n{registry_debug}",
    )
    lease_status = json.loads((fixture / "lease-status.json").read_text(encoding="utf-8"))
    require(
        lease_status["activeLeases"] == 1,
        "nested resolver duplicated the active parent lease",
    )
    require(
        state.status(fixture)["activeLeases"] == 0,
        "owned test lease was not released",
    )
    passed("lease-validation-fail-closed")
    passed("nested-live-lease-reuse")

scripts_dir = root / "scripts"
cargo_re = re.compile(r"(^|[ \t])cargo[ \t]+", re.MULTILINE)
direct_target_re = re.compile(r"^[ \t]*(?:export[ \t]+)?CARGO_TARGET_DIR=", re.MULTILINE)
legacy_override_re = re.compile(r"GENESIS_[A-Z0-9_]+_CARGO_TARGET_DIR")
old_island_re = re.compile(r"\.genesis/build/(?:cargo|root|release_smoke|evidence_[A-Za-z0-9_-]+|health|docs_quickstart|runtime_backend_feature_matrix|selfhost_[A-Za-z0-9_-]+|task_concurrency_stress|host_bridge_fault_injection)(?:[/'\"$]|$)")
allow_string_only = {"check_test_execution_profile_matrix.sh", "render_cargo_target_dir_policy_report.sh"}
violations = []
cargo_scripts = 0
helper_scripts = 0
authority_paths = {
    root / cache.POLICY_REL,
    root / cache.SCHEMA_REL,
    root / "scripts/lib/cargo_cache.py",
    root / "scripts/lib/cargo_target_dir.sh",
    root / "scripts/lib/generated_state.py",
    root / "scripts/check_cargo_target_dir_policy.sh",
    root / "scripts/check_evidence_storage_classes.sh",
    root / "scripts/render_cargo_target_dir_policy_report.sh",
    root / "scripts/render_evidence_release_asset.sh",
    root / ".github/workflows/ci.yml",
    root / "docs/spec/CHECK_UPDATE_BOUNDARY_v0.1.md",
    root / "docs/spec/TEST_EXECUTION_PROFILES_v0.1.md",
}
for path in sorted(scripts_dir.glob("*.sh")):
    text = path.read_text(encoding="utf-8")
    if direct_target_re.search(text):
        violations.append(f"{path.name}:direct-CARGO_TARGET_DIR-assignment")
    if path.name != "cargo_target_dir.sh" and legacy_override_re.search(text):
        violations.append(f"{path.name}:legacy-script-specific-target-override")
    if old_island_re.search(text):
        violations.append(f"{path.name}:legacy-build-island-path")
    if "genesis_configure_cargo_target_dir" in text and path.name not in allow_string_only:
        helper_scripts += 1
        authority_paths.add(path)
        for scope_match in re.finditer(r"^[ \t]+(root-host|root-wasi|root-wasm|evidence-verifier-host)[ \t]*$", text, re.MULTILINE):
            require(scope_match.group(1) in {scope["id"] for scope in policy["scopes"]}, "script uses undeclared scope")
    if cargo_re.search(text) is None or path.name in allow_string_only:
        continue
    cargo_scripts += 1
    authority_paths.add(path)
    if "genesis_configure_cargo_target_dir" not in text and "cargo_cache.py" not in text:
        violations.append(f"{path.name}:cargo-without-resolver")

workflow = (root / ".github/workflows/ci.yml").read_text(encoding="utf-8")
install_count = workflow.count("name: Install Rust")
resolve_count = workflow.count("name: Resolve Cargo Cache")
if install_count != resolve_count or install_count == 0:
    violations.append(f"ci.yml:rust-install-resolver-count:{install_count}:{resolve_count}")
for required in (
    "cargo_cache.py --root . --scope root-host --format github-env",
    "ci-wasm-build root-wasm",
    "ci-wasi-build root-wasi",
):
    if required not in workflow:
        violations.append(f"ci.yml:missing:{required}")

require(not violations, "static policy violations: " + ", ".join(violations))
passed("static-script-and-ci-closure")

authority_identity = [
    {"path": path.relative_to(root).as_posix(), "sha256": cache.digest_file(path)}
    for path in sorted(authority_paths)
]
bundle_sha256 = cache.digest_bytes(cache.canonical_bytes(authority_identity))

expected_controls = {
    "arbitrary-inherited-target-rejection",
    "arbitrary-clear-rejection",
    "build-environment-sensitivity",
    "context-convergence",
    "duplicate-policy-key-rejection",
    "feature-definition-sensitivity",
    "fresh-checkout-materialization",
    "host-profile-co-residency",
    "host-path-exclusion",
    "legacy-override-rejection",
    "lease-validation-fail-closed",
    "lockfile-sensitivity",
    "metadata-tamper-rejection",
    "nested-live-lease-reuse",
    "nested-verifier-scope-reservation",
    "observed-toolchain-sensitivity",
    "root-relocation-key-stability",
    "resolved-environment-transition",
    "source-edit-incremental-stability",
    "static-script-and-ci-closure",
    "toolchain-file-sensitivity",
    "undeclared-scope-rejection",
    "unknown-policy-field-rejection",
    "workspace-target-separation",
}
require(set(controls) == expected_controls and len(controls) == len(expected_controls), "negative-control coverage drift")

doc = {
    "authorityCount": len(authority_identity),
    "buildEnvironment": policy["buildEnvironment"],
    "bundleSha256": bundle_sha256,
    "cargoScripts": cargo_scripts,
    "controls": sorted(controls),
    "helperScripts": helper_scripts,
    "kind": "genesis/cargo-target-dir-policy-v0.1",
    "policySha256": cache.digest_file(root / cache.POLICY_REL),
    "scopeIds": [scope["id"] for scope in policy["scopes"]],
    "strategyVersion": policy["strategyVersion"],
    "timestamp_unix_s": int(time.time()),
    "violation_count": 0,
    "violations": [],
    "ok": True,
}
if report_path.is_file():
    try:
        previous = cache.load_json(report_path)
        if isinstance(previous, dict) and isinstance(previous.get("violation_count"), int):
            doc["previous_violation_count"] = previous["violation_count"]
            doc["violation_delta"] = -previous["violation_count"]
    except cache.CachePolicyError:
        pass
report_path.parent.mkdir(parents=True, exist_ok=True)
history_path.parent.mkdir(parents=True, exist_ok=True)
report_path.write_text(json.dumps(doc, indent=2, sort_keys=True) + "\n", encoding="utf-8")
with history_path.open("a", encoding="utf-8") as handle:
    handle.write(json.dumps(doc, sort_keys=True) + "\n")

print(
    "cargo-target-dir-policy: ok "
    f"(scopes={len(policy['scopes'])}, cargo_scripts={cargo_scripts}, "
    f"helper_scripts={helper_scripts}, controls={len(controls)}, violations=0, "
    f"bundle={bundle_sha256})"
)
PY
