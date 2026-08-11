#!/usr/bin/env python3
"""Validate the normative self-host closure lattice and its mutation controls."""

from __future__ import annotations

import argparse
import copy
import hashlib
import json
from pathlib import Path
from typing import Any, Callable


class ContractError(ValueError):
    pass


ROOT_FIELDS = {
    "aggregationRules",
    "applicabilityDispositions",
    "canonicalSpec",
    "canonicalSpecSha256",
    "contentIdentitySha256",
    "evidenceClasses",
    "forbiddenShortcuts",
    "kind",
    "levelOrder",
    "levels",
    "nonclaims",
    "promotionInvalidators",
    "schema",
    "schemaSha256",
    "stage0Contract",
    "stage0ContractIdentitySha256",
    "unitOfAssessment",
    "version",
}
LEVEL_FIELDS = {
    "id",
    "name",
    "negativeControls",
    "nonclaims",
    "predicates",
    "requiredEvidenceClasses",
    "requiresLevels",
}
DISPOSITION_FIELDS = {
    "canSatisfyPrerequisite",
    "countsTowardAggregate",
    "criteria",
    "id",
    "ledgerValue",
}
EVIDENCE_FIELDS = {"id", "minimumControls", "requiredFields"}
LEVEL_ORDER = ("H0", "H1", "H2", "H3", "H4")
LEVEL_NAMES = {
    "H0": "routed",
    "H1": "GenesisCode-implementation",
    "H2": "GenesisCode-production-authority",
    "H3": "reproducible-bootstrap-fixpoint",
    "H4": "independently-reimplemented-and-conformant",
}
EVIDENCE_ORDER = (
    "route-custody",
    "implementation-conformance",
    "production-authority",
    "bootstrap-fixpoint",
    "independent-conformance",
)
UNIT_FIELDS = {
    "semantic-decision-id",
    "normative-specification-and-profile-version",
    "command-or-API-surface",
    "target-and-runtime-profile",
    "canonical-input-and-output-contract-identities",
    "producing-implementation-and-artifact-identities",
    "production-authority-and-host-binding-identities",
    "verifier-identity",
    "fallback-policy",
    "evidence-closure-identity",
}
REQUIRED_PREDICATES = {
    "H0": {
        "explicit-versioned-deterministic-route-bound-to-decision-key",
        "all-alternate-routes-and-fallbacks-are-enumerated-and-observable",
    },
    "H1": {
        "reviewed-GenesisCode-source-computes-the-semantic-decision",
        "host-calls-are-declared-non-semantic-S0-H-adapters",
        "normative-differential-golden-malformed-adversarial-and-resource-corpora-pass",
    },
    "H2": {
        "accepted-GenesisCode-artifact-is-sole-reachable-production-semantic-producer",
        "no-host-embedded-source-compatibility-environment-error-timeout-recovery-or-debug-semantic-fallback",
        "independently-controlled-verifier-checks-reachability-identity-fallback-absence-and-result-custody",
        "all-in-profile-production-and-release-entrypoints-agree",
    },
    "H3": {
        "stage0-builds-stage1-stage1-builds-stage2-stage2-builds-stage3",
        "canonical-stage2-and-stage3-identities-are-equal",
        "two-clean-runs-per-qualified-host-reproduce-identities",
        "two-independently-administered-qualified-host-classes-share-fixpoint-identity",
        "DDC-or-equivalent-independent-source-to-binary-witness-binds-reviewed-source",
    },
    "H4": {
        "independent-path-has-separate-authorship-review-build-operation-and-custody",
        "no-producer-source-generated-implementation-semantic-library-parser-evaluator-codegen-or-test-oracle-is-shared",
        "hidden-or-held-back-controls-prove-non-replay",
        "every-disagreement-blocks-promotion-and-release",
        "independent-path-cannot-promote-itself-or-rewrite-its-evaluator",
    },
}
REQUIRED_NEGATIVE_CONTROLS = {
    "H0": {"bypass-route", "stale-artifact"},
    "H1": {"opaque-host-semantic-delegation", "malformed-input"},
    "H2": {"restore-each-host-semantic-fallback", "unapproved-rollback"},
    "H3": {"host-profile-substitution", "trusting-trust-substitution"},
    "H4": {"shared-producer-source-or-generated-code", "producer-mutation", "disagreement-suppression"},
}
FORBIDDEN_SHORTCUTS = {
    "repository-wide-level-without-closed-decision-inventory",
    "routing-wrapper-or-file-extension-as-implementation-proof",
    "differential-parity-as-production-authority",
    "feature-flag-or-convention-as-no-fallback-proof",
    "producer-generated-status-as-promotion-authority",
    "one-stage-one-run-or-one-host-as-bootstrap-fixpoint",
    "unreviewed-normalization-of-artifact-differences",
    "shared-producer-code-or-generated-port-as-independent-reimplementation",
    "producer-verdict-as-independent-verification",
    "N/A-disposition-used-to-hide-an-applicable-decision",
    "model-optimizer-benchmark-solver-or-candidate-self-verification",
}
AGGREGATION_RULES = {
    "aggregate-is-minimum-proven-level-across-closed-applicable-inventory",
    "omitted-disputed-or-unknown-row-fails-aggregate-closed",
    "H2-aggregate-requires-all-release-profile-decisions-at-H2-or-higher",
    "H3-aggregate-requires-all-bootstrap-graph-members-at-H3-under-one-fixpoint-identity",
    "H4-aggregate-requires-all-critical-semantic-and-artifact-acceptance-roots-at-H4",
    "N/A-requires-reviewed-disposition-and-never-raises-aggregate",
}
INVALIDATORS = {
    "evidence-expiry-or-supersession",
    "decision-key-or-profile-drift",
    "route-or-dependency-drift",
    "newly-reachable-fallback",
    "artifact-or-verifier-identity-mismatch",
    "verifier-conflict-or-conformance-disagreement",
    "independence-manifest-violation",
}
NONCLAIMS = {
    "does-not-assign-any-current-H-level",
    "does-not-change-production-authority",
    "does-not-broaden-stage0",
    "does-not-establish-bootstrap-closure",
    "does-not-close-R4.1.c-through-R4.1.e",
    "does-not-authorize-GenesisBench-Foundry-Challenge-or-Model-work",
}
SPEC_MARKERS = (
    "Levels apply to one exact semantic decision",
    "`N/A` is a disposition, not a level",
    "Levels are monotonic predicates: Hn requires every predicate",
    "### H0: Routed",
    "### H1: GenesisCode Implementation",
    "### H2: GenesisCode Production Authority",
    "### H3: Reproducible Bootstrap Fixpoint",
    "### H4: Independently Reimplemented and Conformant",
    "No host-semantic, embedded, source, compatibility, environment-variable, error,",
    "At least two clean runs per host",
    "independently administered qualified host classes",
    "shares no producer source, generated implementation, semantic library",
    "claim is the minimum proven level",
    "assigns no current H-level",
)


def fail(message: str) -> None:
    raise ContractError(message)


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
    except (OSError, json.JSONDecodeError) as error:
        fail(f"cannot read {path}: {error}")


def sha256_bytes(value: bytes) -> str:
    return hashlib.sha256(value).hexdigest()


def canonical_identity(value: dict[str, Any]) -> str:
    payload = {key: item for key, item in value.items() if key != "contentIdentitySha256"}
    return sha256_bytes(
        json.dumps(payload, sort_keys=True, separators=(",", ":"), ensure_ascii=True).encode("utf-8")
    )


def require_closed(value: Any, fields: set[str], label: str) -> dict[str, Any]:
    if not isinstance(value, dict):
        fail(f"{label} must be an object")
    missing = sorted(fields - set(value))
    unknown = sorted(set(value) - fields)
    if missing or unknown:
        fail(f"{label} field drift: missing={missing}, unknown={unknown}")
    return value


def string_set(value: Any, label: str, *, nonempty: bool = True) -> set[str]:
    if not isinstance(value, list) or not all(isinstance(item, str) and item for item in value):
        fail(f"{label} must be a string array")
    if nonempty and not value:
        fail(f"{label} must not be empty")
    if len(value) != len(set(value)):
        fail(f"{label} contains duplicates")
    return set(value)


def require_exact_set(value: Any, expected: set[str], label: str) -> None:
    actual = string_set(value, label)
    if actual != expected:
        fail(f"{label} differs from reviewed inventory")


def validate_schema(schema: Any) -> None:
    if not isinstance(schema, dict):
        fail("schema root must be an object")
    if schema.get("$schema") != "https://json-schema.org/draft/2020-12/schema":
        fail("schema draft drift")
    if schema.get("$id") != "https://genesiscode.dev/schemas/selfhost-closure-levels-v0.1.json":
        fail("schema ID drift")
    if schema.get("additionalProperties") is not False:
        fail("schema root must be closed")
    if set(schema.get("required", [])) != ROOT_FIELDS:
        fail("schema required root fields drift")
    if set(schema.get("properties", {})) != ROOT_FIELDS:
        fail("schema root property inventory drift")
    defs = schema.get("$defs")
    if not isinstance(defs, dict):
        fail("schema definitions missing")
    for name in ("disposition", "evidenceClass", "level"):
        definition = defs.get(name)
        if not isinstance(definition, dict) or definition.get("additionalProperties") is not False:
            fail(f"schema {name} must be a closed object")
    if set(defs["level"].get("required", [])) != LEVEL_FIELDS:
        fail("schema level required fields drift")
    if set(defs["disposition"].get("required", [])) != DISPOSITION_FIELDS:
        fail("schema disposition required fields drift")
    if set(defs["evidenceClass"].get("required", [])) != EVIDENCE_FIELDS:
        fail("schema evidence-class required fields drift")


def validate_contract(
    contract: Any,
    schema: Any,
    spec_bytes: bytes,
    schema_bytes: bytes,
    stage0: Any,
) -> None:
    root = require_closed(contract, ROOT_FIELDS, "contract")
    validate_schema(schema)

    expected_scalars = {
        "kind": "genesis/selfhost-closure-levels-v0.1",
        "version": "0.1",
        "canonicalSpec": "docs/spec/SELFHOST_CLOSURE_LEVELS_v0.1.md",
        "stage0Contract": "docs/spec/STAGE0_TRUST_CONTRACT_v0.1.json",
        "schema": "docs/spec/SELFHOST_CLOSURE_LEVELS_v0.1.schema.json",
    }
    for field, expected in expected_scalars.items():
        if root.get(field) != expected:
            fail(f"{field} drift")

    if root.get("canonicalSpecSha256") != sha256_bytes(spec_bytes):
        fail("canonical prose identity mismatch")
    if root.get("schemaSha256") != sha256_bytes(schema_bytes):
        fail("schema identity mismatch")
    if root.get("contentIdentitySha256") != canonical_identity(root):
        fail("content identity mismatch")
    if not isinstance(stage0, dict):
        fail("stage0 contract must be an object")
    if root.get("stage0ContractIdentitySha256") != stage0.get("contentIdentitySha256"):
        fail("stage0 contract identity mismatch")

    require_exact_set(root["unitOfAssessment"], UNIT_FIELDS, "unitOfAssessment")
    if tuple(root["levelOrder"]) != LEVEL_ORDER:
        fail("level order drift")

    dispositions = root["applicabilityDispositions"]
    if not isinstance(dispositions, list) or [item.get("id") for item in dispositions if isinstance(item, dict)] != [
        "applicable", "residual-stage0", "not-semantic-decision"
    ]:
        fail("applicability disposition order or identity drift")
    for item in dispositions:
        disposition = require_closed(item, DISPOSITION_FIELDS, "disposition")
        string_set(disposition["criteria"], f"{disposition['id']} criteria")
    if dispositions[0]["ledgerValue"] != "H0-H4" or dispositions[0]["canSatisfyPrerequisite"] is not True or dispositions[0]["countsTowardAggregate"] is not True:
        fail("applicable disposition semantics drift")
    for disposition in dispositions[1:]:
        if disposition["ledgerValue"] != "N/A" or disposition["canSatisfyPrerequisite"] is not False or disposition["countsTowardAggregate"] is not False:
            fail(f"{disposition['id']} must be non-promoting N/A")
    if "exact-decision-is-named-S0-K-reference-semantics-or-unavoidable-S0-H-physical-adapter" not in dispositions[1]["criteria"]:
        fail("residual-stage0 boundary drift")

    evidence = root["evidenceClasses"]
    if not isinstance(evidence, list) or [item.get("id") for item in evidence if isinstance(item, dict)] != list(EVIDENCE_ORDER):
        fail("evidence class order or identity drift")
    for item in evidence:
        evidence_class = require_closed(item, EVIDENCE_FIELDS, "evidence class")
        required = string_set(evidence_class["requiredFields"], f"{evidence_class['id']} required fields")
        string_set(evidence_class["minimumControls"], f"{evidence_class['id']} controls")
        if "cryptographic-identity" not in required:
            fail(f"{evidence_class['id']} lacks cryptographic identity")

    levels = root["levels"]
    if not isinstance(levels, list) or [item.get("id") for item in levels if isinstance(item, dict)] != list(LEVEL_ORDER):
        fail("level inventory or order drift")
    for index, item in enumerate(levels):
        level = require_closed(item, LEVEL_FIELDS, "level")
        level_id = level["id"]
        if level["name"] != LEVEL_NAMES[level_id]:
            fail(f"{level_id} name drift")
        if level["requiresLevels"] != list(LEVEL_ORDER[:index]):
            fail(f"{level_id} is not cumulative over every lower level")
        if level["requiredEvidenceClasses"] != list(EVIDENCE_ORDER[: index + 1]):
            fail(f"{level_id} evidence classes are not cumulative")
        predicates = string_set(level["predicates"], f"{level_id} predicates")
        controls = string_set(level["negativeControls"], f"{level_id} controls")
        string_set(level["nonclaims"], f"{level_id} nonclaims")
        if not REQUIRED_PREDICATES[level_id].issubset(predicates):
            fail(f"{level_id} required predicate missing")
        if not REQUIRED_NEGATIVE_CONTROLS[level_id].issubset(controls):
            fail(f"{level_id} required negative control missing")

    require_exact_set(root["aggregationRules"], AGGREGATION_RULES, "aggregationRules")
    require_exact_set(root["promotionInvalidators"], INVALIDATORS, "promotionInvalidators")
    require_exact_set(root["forbiddenShortcuts"], FORBIDDEN_SHORTCUTS, "forbiddenShortcuts")
    require_exact_set(root["nonclaims"], NONCLAIMS, "nonclaims")

    spec_text = spec_bytes.decode("utf-8")
    missing_markers = [marker for marker in SPEC_MARKERS if marker not in spec_text]
    if missing_markers:
        fail(f"normative prose markers missing: {missing_markers}")


def reseal(contract: dict[str, Any], spec_bytes: bytes, schema_bytes: bytes) -> None:
    contract["canonicalSpecSha256"] = sha256_bytes(spec_bytes)
    contract["schemaSha256"] = sha256_bytes(schema_bytes)
    contract["contentIdentitySha256"] = canonical_identity(contract)


def run_self_tests(
    contract: dict[str, Any], schema: dict[str, Any], spec_bytes: bytes, schema_bytes: bytes, stage0: dict[str, Any]
) -> int:
    controls: list[tuple[str, Callable[[dict[str, Any], dict[str, Any], bytearray, dict[str, Any]], None]]] = []

    controls.append(("unknown-root-field", lambda c, s, p, t: c.__setitem__("surprise", True)))
    controls.append(("kind-drift", lambda c, s, p, t: c.__setitem__("kind", "genesis/wrong")))
    controls.append(("unit-of-assessment-loss", lambda c, s, p, t: c["unitOfAssessment"].pop()))
    controls.append(("level-order-swap", lambda c, s, p, t: c["levelOrder"].reverse()))
    controls.append(("level-removal", lambda c, s, p, t: c["levels"].pop()))
    controls.append(("level-skip", lambda c, s, p, t: c["levels"][3]["requiresLevels"].remove("H1")))
    controls.append(("evidence-not-cumulative", lambda c, s, p, t: c["levels"][2]["requiredEvidenceClasses"].remove("implementation-conformance")))
    controls.append(("H0-route-predicate-loss", lambda c, s, p, t: c["levels"][0]["predicates"].remove("explicit-versioned-deterministic-route-bound-to-decision-key")))
    controls.append(("H1-host-delegation-predicate-loss", lambda c, s, p, t: c["levels"][1]["predicates"].remove("host-calls-are-declared-non-semantic-S0-H-adapters")))
    controls.append(("H2-no-fallback-predicate-loss", lambda c, s, p, t: c["levels"][2]["predicates"].remove("no-host-embedded-source-compatibility-environment-error-timeout-recovery-or-debug-semantic-fallback")))
    controls.append(("H2-fallback-control-loss", lambda c, s, p, t: c["levels"][2]["negativeControls"].remove("restore-each-host-semantic-fallback")))
    controls.append(("H3-cross-host-predicate-loss", lambda c, s, p, t: c["levels"][3]["predicates"].remove("two-independently-administered-qualified-host-classes-share-fixpoint-identity")))
    controls.append(("H3-DDC-predicate-loss", lambda c, s, p, t: c["levels"][3]["predicates"].remove("DDC-or-equivalent-independent-source-to-binary-witness-binds-reviewed-source")))
    controls.append(("H4-independence-predicate-loss", lambda c, s, p, t: c["levels"][4]["predicates"].remove("no-producer-source-generated-implementation-semantic-library-parser-evaluator-codegen-or-test-oracle-is-shared")))
    controls.append(("H4-disagreement-control-loss", lambda c, s, p, t: c["levels"][4]["negativeControls"].remove("disagreement-suppression")))
    controls.append(("N-A-promotes", lambda c, s, p, t: c["applicabilityDispositions"][1].__setitem__("countsTowardAggregate", True)))
    controls.append(("aggregate-minimum-loss", lambda c, s, p, t: c["aggregationRules"].remove("aggregate-is-minimum-proven-level-across-closed-applicable-inventory")))
    controls.append(("forbidden-shortcut-loss", lambda c, s, p, t: c["forbiddenShortcuts"].remove("producer-generated-status-as-promotion-authority")))
    controls.append(("stage0-binding-drift", lambda c, s, p, t: t.__setitem__("contentIdentitySha256", "f" * 64)))
    controls.append(("schema-open-object", lambda c, s, p, t: s.__setitem__("additionalProperties", True)))
    controls.append(("normative-prose-marker-loss", lambda c, s, p, t: p.__setitem__(slice(None), bytes(p).replace(b"### H4: Independently Reimplemented and Conformant", b"### H4"))))

    for name, mutate in controls:
        candidate = copy.deepcopy(contract)
        candidate_schema = copy.deepcopy(schema)
        candidate_spec = bytearray(spec_bytes)
        candidate_stage0 = copy.deepcopy(stage0)
        mutate(candidate, candidate_schema, candidate_spec, candidate_stage0)
        candidate_schema_bytes = json.dumps(candidate_schema, sort_keys=True, separators=(",", ":")).encode("utf-8")
        reseal(candidate, bytes(candidate_spec), candidate_schema_bytes)
        try:
            validate_contract(candidate, candidate_schema, bytes(candidate_spec), candidate_schema_bytes, candidate_stage0)
        except ContractError:
            continue
        fail(f"negative control accepted: {name}")
    return len(controls)


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--root", type=Path, default=Path(__file__).resolve().parents[2])
    parser.add_argument("--contract", default="docs/spec/SELFHOST_CLOSURE_LEVELS_v0.1.json")
    parser.add_argument("--schema", default="docs/spec/SELFHOST_CLOSURE_LEVELS_v0.1.schema.json")
    parser.add_argument("--spec", default="docs/spec/SELFHOST_CLOSURE_LEVELS_v0.1.md")
    parser.add_argument("--stage0", default="docs/spec/STAGE0_TRUST_CONTRACT_v0.1.json")
    parser.add_argument("--self-test", action="store_true")
    args = parser.parse_args()

    root = args.root.resolve()
    contract_path = root / args.contract
    schema_path = root / args.schema
    spec_path = root / args.spec
    stage0_path = root / args.stage0
    contract = load_json(contract_path)
    schema = load_json(schema_path)
    stage0 = load_json(stage0_path)
    try:
        spec_bytes = spec_path.read_bytes()
        schema_bytes = schema_path.read_bytes()
    except OSError as error:
        fail(f"cannot read bound authority: {error}")

    validate_contract(contract, schema, spec_bytes, schema_bytes, stage0)
    controls = run_self_tests(contract, schema, spec_bytes, schema_bytes, stage0) if args.self_test else 0
    print(
        "selfhost-closure-levels: ok "
        f"identity={contract['contentIdentitySha256']} levels={len(contract['levels'])} controls={controls}"
    )
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except ContractError as error:
        print(f"selfhost-closure-levels: failed: {error}")
        raise SystemExit(1)
