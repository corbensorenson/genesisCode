#!/usr/bin/env bash
set -euo pipefail

source "$(dirname "${BASH_SOURCE[0]}")/lib/gate_telemetry.sh"
genesis_gate_telemetry_reexec "$0" "$@"

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

MATRIX="docs/program/EVIDENCE_ADVERSARIAL_MATRIX_v0.1.json"
CATALOG="docs/program/evidence/GENESIS_EVIDENCE_VERIFIER_NEGATIVE_VECTORS_v0.1.json"
SCHEMA="docs/spec/EVIDENCE_ADVERSARIAL_MATRIX_v0.1.schema.json"

python3 - "$MATRIX" "$CATALOG" "$SCHEMA" <<'PY'
import json
from pathlib import Path
import sys


def load_unique(path):
    def pairs(items):
        result = {}
        for key, value in items:
            if key in result:
                raise SystemExit(f"evidence-adversarial-matrix: duplicate JSON key in {path}: {key}")
            result[key] = value
        return result

    return json.loads(Path(path).read_text(encoding="utf-8"), object_pairs_hook=pairs)


matrix = load_unique(sys.argv[1])
catalog = load_unique(sys.argv[2])
schema = load_unique(sys.argv[3])
if (
    schema.get("$schema") != "https://json-schema.org/draft/2020-12/schema"
    or schema.get("$id") != "https://genesiscode.dev/schemas/evidence-adversarial-matrix-v0.1.json"
):
    raise SystemExit("evidence-adversarial-matrix: schema identity mismatch")
expected_requirements = {
    "dirty-input-without-policy": ["dirty-paths-digest-required", "implicit-dirty-policy"],
    "duplicate-keys": ["duplicate-json-key"],
    "forged-signatures": ["invalid-signature"],
    "missing-fields": ["missing-required-field"],
    "path-aliases": ["noncanonical-path-alias"],
    "reordered-altered-replay-facts": ["replay-fact-mutation"],
    "stale-source-identities": [
        "stale-source-repository",
        "stale-source-revision",
        "stale-source-tree",
    ],
    "unsupported-schema-versions": ["unsupported-schema-version"],
}
verifier_command = "cargo test --manifest-path tools/genesis-evidence-verifier/Cargo.toml published_adversarial_vectors_fail_closed_at_expected_boundary --locked --offline"
expected_controls = {
    "dirty-paths-digest-required": ("source-policy", "dirty-paths-digest-missing", "tools/genesis-evidence-verifier/src/verify.rs", verifier_command),
    "duplicate-json-key": ("json-parser", "bundle-duplicate-key", "tools/genesis-evidence-verifier/src/json.rs", verifier_command),
    "implicit-dirty-policy": ("source-policy", "dirty-policy-missing", "tools/genesis-evidence-verifier/src/verify.rs", verifier_command),
    "invalid-signature": ("signature", "forged-signature", "tools/genesis-evidence-verifier/src/verify.rs", verifier_command),
    "missing-required-field": ("schema-profile", "bundle-missing-field", "tools/genesis-evidence-verifier/src/verify.rs", verifier_command),
    "noncanonical-path-alias": ("artifact-path", "artifact-path-alias", "tools/genesis-evidence-verifier/src/verify.rs", verifier_command),
    "replay-fact-mutation": (
        "replay",
        "replay_adversarial_matrix_rejects_reordered_and_altered_facts",
        "crates/gc_effects/src/runner.rs",
        "cargo test -p gc_effects --lib replay_adversarial_matrix_rejects_reordered_and_altered_facts --locked --offline",
    ),
    "stale-source-repository": ("source-policy", "source-repository-stale", "tools/genesis-evidence-verifier/src/verify.rs", verifier_command),
    "stale-source-revision": ("source-policy", "source-revision-stale", "tools/genesis-evidence-verifier/src/verify.rs", verifier_command),
    "stale-source-tree": ("source-policy", "source-tree-stale", "tools/genesis-evidence-verifier/src/verify.rs", verifier_command),
    "unsupported-schema-version": ("schema-profile", "bundle-version-unsupported", "tools/genesis-evidence-verifier/src/verify.rs", verifier_command),
}
if set(matrix) != {"kind", "version", "roadmapTask", "requirements", "controls"}:
    raise SystemExit("evidence-adversarial-matrix: top-level fields mismatch")
if (
    matrix["kind"] != "genesis/evidence-adversarial-matrix-v0.1"
    or matrix["version"] != "0.1"
    or matrix["roadmapTask"] != "R0.2.e"
):
    raise SystemExit("evidence-adversarial-matrix: identity mismatch")
requirements = matrix["requirements"]
controls = matrix["controls"]
requirement_ids = [item.get("id") for item in requirements]
control_ids = [item.get("id") for item in controls]
if sorted(requirement_ids) != sorted(expected_requirements) or len(requirement_ids) != len(set(requirement_ids)):
    raise SystemExit("evidence-adversarial-matrix: requirement coverage mismatch")
if control_ids != sorted(set(control_ids)):
    raise SystemExit("evidence-adversarial-matrix: control ids must be sorted and unique")
control_by_id = {item["id"]: item for item in controls}
referenced = []
for requirement in requirements:
    if set(requirement) != {"id", "statement", "controlIds"} or not requirement["statement"]:
        raise SystemExit(f"evidence-adversarial-matrix: malformed requirement: {requirement!r}")
    if not requirement["controlIds"] or requirement["controlIds"] != sorted(set(requirement["controlIds"])):
        raise SystemExit(f"evidence-adversarial-matrix: invalid control references for {requirement['id']}")
    if requirement["controlIds"] != expected_requirements[requirement["id"]]:
        raise SystemExit(f"evidence-adversarial-matrix: control binding drift for {requirement['id']}")
    referenced.extend(requirement["controlIds"])
if sorted(referenced) != control_ids:
    raise SystemExit("evidence-adversarial-matrix: controls must be referenced exactly once")
catalog_by_id = {item["id"]: item for item in catalog["cases"]}
for control in controls:
    if set(control) != {"id", "layer", "fixture", "command", "expectedDiagnostic", "authority"}:
        raise SystemExit(f"evidence-adversarial-matrix: malformed control: {control!r}")
    for field in ("fixture", "command", "expectedDiagnostic", "authority"):
        if not isinstance(control[field], str) or not control[field]:
            raise SystemExit(f"evidence-adversarial-matrix: empty {field} for {control['id']}")
    if not Path(control["authority"]).is_file():
        raise SystemExit(f"evidence-adversarial-matrix: missing authority for {control['id']}")
    observed_binding = (
        control["layer"],
        control["fixture"],
        control["authority"],
        control["command"],
    )
    if observed_binding != expected_controls[control["id"]]:
        raise SystemExit(f"evidence-adversarial-matrix: control binding drift for {control['id']}")
    if control["layer"] == "replay":
        if control["fixture"] != "replay_adversarial_matrix_rejects_reordered_and_altered_facts":
            raise SystemExit("evidence-adversarial-matrix: replay fixture mismatch")
        continue
    fixture = catalog_by_id.get(control["fixture"])
    if fixture is None:
        raise SystemExit(f"evidence-adversarial-matrix: unknown verifier fixture: {control['fixture']}")
    if fixture["expectedDiagnostic"] != control["expectedDiagnostic"]:
        raise SystemExit(f"evidence-adversarial-matrix: diagnostic drift for {control['id']}")
PY

python3 scripts/lib/genesis_evidence_verifier_vectors.py --check

source "$ROOT_DIR/scripts/lib/cargo_target_dir.sh"
genesis_configure_cargo_target_dir \
  "$ROOT_DIR" \
  "evidence-adversarial-verifier" \
  evidence-verifier-host
cargo test \
  --manifest-path tools/genesis-evidence-verifier/Cargo.toml \
  published_adversarial_vectors_fail_closed_at_expected_boundary \
  --locked --offline

genesis_configure_cargo_target_dir \
  "$ROOT_DIR" \
  "evidence-adversarial-runtime" \
  root-host
cargo test -p gc_effects --lib \
  replay_adversarial_matrix_rejects_reordered_and_altered_facts \
  --locked --offline

echo "evidence-adversarial-matrix-contract: ok (requirements=8 controls=11 verifier_vectors=30 replay_mutations=16)"
