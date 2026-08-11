#!/usr/bin/env python3
from __future__ import annotations

import argparse
import copy
import hashlib
import json
import pathlib
import re
from typing import Any


class ContractError(ValueError):
    pass


DOMAIN_IDS = ("S0-K", "S0-R", "S0-P", "S0-X", "S0-A", "S0-H")
LAYER_ORDER = ("S0-R", "S0-A", "S0-P", "S0-K", "S0-X", "S0-H")
ROOT_FIELDS = {
    "canonicalSpec",
    "canonicalSpecSha256",
    "contentIdentitySha256",
    "identityInputs",
    "kind",
    "layerOrder",
    "mappingStatus",
    "nonclaims",
    "residualHostAssumptions",
    "schema",
    "schemaSha256",
    "stage0Domains",
    "tcbA",
    "version",
}
DOMAIN_FIELDS = {
    "authority",
    "demotionTasks",
    "forbiddenAuthority",
    "id",
    "implementationEvidence",
    "name",
    "tcbA",
    "trustReason",
}
REQUIRED_IDENTITY_INPUTS = {
    "build-recipe",
    "compiler-linker-target-architecture-features",
    "dependencies-and-generated-inputs",
    "domain-versions-and-source-membership",
    "semantic-and-abi-profiles",
    "selfhost-artifact-manifest-and-bootstrap-mode",
    "verifier-and-trust-roots-for-release-claims",
}
REQUIRED_ASSUMPTIONS = {
    "hardware-and-operating-system",
    "memory-allocation-and-process-isolation",
    "pinned-host-compiler-and-linker-before-H3-DDC",
    "cryptographic-provider-and-platform-drivers",
    "independent-release-verifier-and-signing-roots",
}
REQUIRED_NONCLAIMS = {
    "does-not-establish-H1-H4",
    "does-not-enforce-the-R4.1.e-dependency-firewall",
    "does-not-migrate-a-semantic-decision",
    "does-not-prove-a-bootstrap-fixpoint",
    "does-not-authorize-GenesisBench-Foundry-Challenge-or-Model-work",
}
REQUIRED_DOMAIN_AUTHORITY = {
    "S0-K": {
        "immutable-runtime-values-and-collections",
        "reference-pure-evaluation",
        "pure-primitive-allowlist",
        "lexical-scope-and-closures",
        "deterministic-resource-accounting",
        "seal-creation-sealing-unsealing-and-token-identity",
    },
    "S0-R": {
        "source-decoding",
        "canonicalization-and-printing",
        "term-ordering",
        "canonical-content-hashing",
    },
    "S0-P": {
        "reserve-unforgeable-protocol-tokens",
        "bind-minimal-protocol-constructors-and-predicates",
        "assemble-a-fresh-prelude-environment",
        "load-a-reviewed-selfhost-artifact",
    },
    "S0-X": {
        "decode-exact-versioned-compiled-artifacts",
        "validate-compiled-structure-and-resource-bounds",
        "execute-compiled-forms-under-reference-equivalence-obligations",
    },
    "S0-A": {
        "bind-source-manifest-profile-dependency-cache-and-artifact-identities",
        "reject-stale-malformed-noncanonical-over-budget-or-wrong-profile-inputs",
        "select-only-an-explicit-bootstrap-mode",
    },
    "S0-H": {
        "deny-by-default-capability-decisions",
        "bounded-host-operation-dispatch",
        "host-input-and-error-normalization",
        "hard-cancellation-where-promised-and-explicit-resource-closure",
        "deterministic-effect-logging-and-strict-replay",
        "declared-platform-transport-and-embedding",
    },
}
REQUIRED_FORBIDDEN_AUTHORITY = {
    "S0-K": {
        "source-decoding-or-canonical-printing",
        "compiled-artifact-codec-or-optimized-execution",
        "effect-interpretation-or-capability-policy",
        "artifact-promotion-or-release-authority",
        "ambient-filesystem-time-random-network-process-environment-ui-or-model-access",
    },
    "S0-R": {
        "evaluation-or-seal-issuance",
        "capability-decisions-or-effects",
        "artifact-promotion",
        "fallback-selection",
    },
    "S0-P": {
        "ambient-effects-or-policy-grants",
        "hidden-semantic-fallback",
        "package-resolution",
        "optimizer-acceptance-or-release-promotion",
    },
    "S0-X": {
        "capability-access-or-policy-authority",
        "unknown-format-acceptance",
        "workload-shaped-semantic-shortcuts",
        "self-issued-equivalence",
    },
    "S0-A": {
        "language-semantics",
        "silent-source-embedded-or-rust-fallback",
        "package-publication-or-obligation-waiver",
        "equivalence-self-approval",
    },
    "S0-H": {
        "pure-language-semantics-or-seal-minting",
        "source-canonicalization",
        "selfhost-semantic-fallback",
        "optimizer-verification",
        "package-policy-waiver-or-release-promotion",
    },
}
REQUIRED_DOMAIN_NAMES = {
    "S0-K": "pure-semantic-kernel",
    "S0-R": "coreform-representation-and-identity",
    "S0-P": "protocol-and-bootstrap-assembly",
    "S0-X": "compiled-artifact-and-optimized-execution",
    "S0-A": "bootstrap-artifact-identity-and-admission",
    "S0-H": "effect-host-abi-and-containment",
}
REQUIRED_IMPLEMENTATION_EVIDENCE = {
    "S0-K": {
        "policies/kernel_tcb_contract.toml",
        "crates/gc_kernel/src/eval_treewalk.rs",
        "crates/gc_kernel/src/eval_forms.rs",
        "crates/gc_kernel/src/eval_prims.rs",
        "crates/gc_kernel/src/value.rs",
    },
    "S0-R": {
        "crates/gc_coreform/src/lib.rs",
        "crates/gc_coreform/src/term.rs",
        "crates/gc_coreform/src/parse.rs",
        "crates/gc_coreform/src/canon.rs",
        "crates/gc_coreform/src/print.rs",
    },
    "S0-P": {
        "crates/gc_prelude/src/prelude.rs",
        "crates/gc_prelude/src/prelude_contract_effect.rs",
        "prelude/modules/manifest.toml",
        "prelude/prelude.gc",
    },
    "S0-X": {
        "crates/gc_cli_driver/src/kernel_exec.rs",
        "crates/gc_kernel/src/compiled.rs",
        "crates/gc_kernel/src/compiled_blob.rs",
        "crates/gc_kernel/src/compiled_compile.rs",
        "crates/gc_kernel/src/compiled_runtime",
    },
    "S0-A": {
        "crates/gc_prelude/src/selfhost_coreform_v1.rs",
        "crates/gc_prelude/src/selfhost_coreform_manifest.rs",
        "crates/gc_prelude/src/selfhost_compiled_cache.rs",
        "selfhost/toolchain_manifest.gc",
    },
    "S0-H": {
        "crates/gc_effects/src",
        "crates/gc_cli_driver/src/host_bridge_runtime.rs",
        "crates/gc_cli/src/main.rs",
        "crates/gc_wasi_cli/src/main.rs",
        "crates/gc_wasm/src/lib.rs",
    },
}
REQUIRED_DEMOTION_TASKS = {
    "S0-K": {"R4.5.a", "R4.5.c", "R7.2.a", "R7.2.b"},
    "S0-R": {"R4.2.a", "R4.5.a", "R4.5.c", "R7.2.f"},
    "S0-P": {"R4.2.a", "R4.2.b", "R4.4.c"},
    "S0-X": {"R3.1.f", "R3.2.d", "R4.5.c", "R7.2.d"},
    "S0-A": {"R4.4.a", "R4.4.c", "R4.4.d", "R4.5.c"},
    "S0-H": {"R4.3.b", "R4.5.b", "R4.5.c", "R5.5.a", "R7.3.a"},
}
SPEC_MARKERS = (
    "Only S0-K is TCB-A",
    "S0-R",
    "S0-A",
    "S0-P",
    "S0-K",
    "S0-X",
    "S0-H",
    "current implementation anchors",
    "R4.1.e",
    "does not claim H1-H4",
)


def fail(message: str) -> None:
    raise ContractError(message)


def pairs_no_duplicates(pairs: list[tuple[str, Any]]) -> dict[str, Any]:
    result: dict[str, Any] = {}
    for key, value in pairs:
        if key in result:
            fail(f"duplicate JSON key: {key}")
        result[key] = value
    return result


def load_json(path: pathlib.Path) -> Any:
    try:
        return json.loads(path.read_text(encoding="utf-8"), object_pairs_hook=pairs_no_duplicates)
    except (OSError, json.JSONDecodeError) as error:
        fail(f"cannot read {path}: {error}")


def require_closed(value: dict[str, Any], expected: set[str], label: str) -> None:
    missing = sorted(expected - set(value))
    unknown = sorted(set(value) - expected)
    if missing:
        fail(f"{label} missing fields: {', '.join(missing)}")
    if unknown:
        fail(f"{label} unknown fields: {', '.join(unknown)}")


def require_string_set(value: Any, expected: set[str], label: str) -> None:
    if not isinstance(value, list) or not all(isinstance(item, str) and item for item in value):
        fail(f"{label} must be a non-empty string array")
    if len(value) != len(set(value)):
        fail(f"{label} contains duplicates")
    if set(value) != expected:
        fail(f"{label} differs from the reviewed inventory")


def canonical_identity(value: dict[str, Any]) -> str:
    payload = {key: item for key, item in value.items() if key != "contentIdentitySha256"}
    encoded = json.dumps(
        payload, sort_keys=True, separators=(",", ":"), ensure_ascii=True
    ).encode("utf-8")
    return hashlib.sha256(encoded).hexdigest()


def validate_schema(schema: Any) -> None:
    if not isinstance(schema, dict):
        fail("schema root must be an object")
    if schema.get("$schema") != "https://json-schema.org/draft/2020-12/schema":
        fail("schema draft identity drift")
    if schema.get("$id") != "https://genesiscode.dev/schemas/stage0-trust-contract-v0.1.json":
        fail("schema ID drift")
    definitions = schema.get("$defs", {})
    for label, node in (("root", schema), ("domain", definitions.get("domain"))):
        if not isinstance(node, dict) or node.get("type") != "object" or node.get("additionalProperties") is not False:
            fail(f"schema {label} object must be closed")
    if set(schema.get("required", [])) != ROOT_FIELDS or set(schema.get("properties", {})) != ROOT_FIELDS:
        fail("schema root field inventory drift")
    domain = definitions["domain"]
    if set(domain.get("required", [])) != DOMAIN_FIELDS or set(domain.get("properties", {})) != DOMAIN_FIELDS:
        fail("schema domain field inventory drift")
    properties = schema["properties"]
    for field in ("canonicalSpecSha256", "contentIdentitySha256", "schemaSha256"):
        if properties.get(field, {}).get("pattern") != "^[0-9a-f]{64}$":
            fail(f"schema {field} contract drift")
    if properties.get("layerOrder", {}).get("const") != list(LAYER_ORDER):
        fail("schema layer order contract drift")
    domain_ids = domain["properties"]["id"].get("enum", [])
    if domain_ids != list(DOMAIN_IDS):
        fail("schema domain identity inventory drift")

    def require_closed_objects(value: Any, location: str) -> None:
        if isinstance(value, dict):
            if value.get("type") == "object" and value.get("additionalProperties") is not False:
                fail(f"schema object is open: {location}")
            for key, child in value.items():
                require_closed_objects(child, f"{location}/{key}")
        elif isinstance(value, list):
            for index, child in enumerate(value):
                require_closed_objects(child, f"{location}/{index}")

    require_closed_objects(schema, "schema")


def validate_contract(
    root: pathlib.Path,
    contract: Any,
    spec_text: str,
    schema_bytes: bytes,
) -> None:
    try:
        roadmap_text = (root / "ROADMAP.md").read_text(encoding="utf-8")
    except OSError as error:
        fail(f"cannot read ROADMAP.md: {error}")
    if not isinstance(contract, dict):
        fail("contract root must be an object")
    require_closed(contract, ROOT_FIELDS, "contract")
    if contract["kind"] != "genesis/stage0-trust-contract-v0.1" or contract["version"] != "0.1":
        fail("contract identity drift")
    if contract["canonicalSpec"] != "docs/spec/SELF_HOST_BOUNDARY.md":
        fail("canonical spec drift")
    if contract["schema"] != "docs/spec/STAGE0_TRUST_CONTRACT_v0.1.schema.json":
        fail("contract schema path drift")
    for field in ("canonicalSpecSha256", "contentIdentitySha256", "schemaSha256"):
        if not isinstance(contract[field], str) or not re.fullmatch(r"[0-9a-f]{64}", contract[field]):
            fail(f"{field} must be lowercase SHA-256")
    if contract["canonicalSpecSha256"] != hashlib.sha256(spec_text.encode("utf-8")).hexdigest():
        fail("canonical spec identity mismatch")
    if contract["schemaSha256"] != hashlib.sha256(schema_bytes).hexdigest():
        fail("schema identity mismatch")
    if contract["contentIdentitySha256"] != canonical_identity(contract):
        fail("content identity mismatch")
    if contract["tcbA"] != "S0-K":
        fail("TCB-A must be exactly S0-K")
    if contract["mappingStatus"] != "responsibility-domains-with-current-implementation-anchors; exhaustive-source-membership-and-dependency-edges-deferred-to-R4.1.e":
        fail("mapping status must reserve exact file/dependency enforcement for R4.1.e")
    if contract["layerOrder"] != list(LAYER_ORDER):
        fail("stage0 layer order drift")
    require_string_set(contract["identityInputs"], REQUIRED_IDENTITY_INPUTS, "identityInputs")
    require_string_set(contract["residualHostAssumptions"], REQUIRED_ASSUMPTIONS, "residualHostAssumptions")
    require_string_set(contract["nonclaims"], REQUIRED_NONCLAIMS, "nonclaims")

    domains = contract["stage0Domains"]
    if not isinstance(domains, list) or len(domains) != len(DOMAIN_IDS):
        fail("stage0Domains must contain exactly six domains")
    if [raw.get("id") if isinstance(raw, dict) else None for raw in domains] != list(DOMAIN_IDS):
        fail("stage0Domains order or identity drift")
    by_id: dict[str, dict[str, Any]] = {}
    for index, raw in enumerate(domains):
        if not isinstance(raw, dict):
            fail(f"stage0Domains[{index}] must be an object")
        require_closed(raw, DOMAIN_FIELDS, f"stage0Domains[{index}]")
        domain_id = raw["id"]
        if domain_id not in DOMAIN_IDS or domain_id in by_id:
            fail(f"unknown or duplicate stage0 domain: {domain_id}")
        by_id[domain_id] = raw
        if raw["name"] != REQUIRED_DOMAIN_NAMES[domain_id]:
            fail(f"{domain_id}.name differs from the reviewed inventory")
        if raw["tcbA"] is not (domain_id == "S0-K"):
            fail(f"{domain_id}.tcbA expands or omits TCB-A")
        for field in ("authority", "forbiddenAuthority", "implementationEvidence", "demotionTasks"):
            values = raw[field]
            if not isinstance(values, list) or not values or not all(isinstance(item, str) and item for item in values):
                fail(f"{domain_id}.{field} must be a non-empty string array")
            if len(values) != len(set(values)):
                fail(f"{domain_id}.{field} contains duplicates")
        if set(raw["authority"]) != REQUIRED_DOMAIN_AUTHORITY[domain_id]:
            fail(f"{domain_id}.authority differs from the reviewed inventory")
        if set(raw["forbiddenAuthority"]) != REQUIRED_FORBIDDEN_AUTHORITY[domain_id]:
            fail(f"{domain_id}.forbiddenAuthority differs from the reviewed inventory")
        if set(raw["implementationEvidence"]) != REQUIRED_IMPLEMENTATION_EVIDENCE[domain_id]:
            fail(f"{domain_id}.implementationEvidence differs from the reviewed anchors")
        if set(raw["demotionTasks"]) != REQUIRED_DEMOTION_TASKS[domain_id]:
            fail(f"{domain_id}.demotionTasks differs from the reviewed path")
        if set(raw["authority"]) & set(raw["forbiddenAuthority"]):
            fail(f"{domain_id} both grants and forbids the same authority")
        if not isinstance(raw["trustReason"], str) or not raw["trustReason"].strip():
            fail(f"{domain_id}.trustReason is empty")
        for path_string in raw["implementationEvidence"]:
            path = pathlib.PurePosixPath(path_string)
            if path.is_absolute() or ".." in path.parts:
                fail(f"{domain_id} has unsafe implementation anchor: {path_string}")
            if not (root / path).exists():
                fail(f"{domain_id} implementation anchor does not exist: {path_string}")
        for task in raw["demotionTasks"]:
            if re.fullmatch(r"R[0-9]+\.[0-9]+\.[a-z]", task) is None:
                fail(f"{domain_id} has invalid demotion task: {task}")
            if f"**{task} " not in roadmap_text:
                fail(f"{domain_id} has unknown demotion task: {task}")
    if set(by_id) != set(DOMAIN_IDS):
        fail("stage0 domain inventory drift")

    for marker in SPEC_MARKERS:
        if marker not in spec_text:
            fail(f"canonical spec missing marker: {marker}")
    if "TCB-A kernel crates (`gc_coreform`, `gc_kernel`, `gc_prelude`)" in spec_text:
        fail("canonical spec still labels three broad crates as TCB-A")


def run_self_test(
    root: pathlib.Path,
    contract: dict[str, Any],
    spec_text: str,
    schema_bytes: bytes,
) -> int:
    controls: list[tuple[str, dict[str, Any], str]] = []
    def add(label: str, mutation: dict[str, Any], candidate_spec: str = spec_text) -> None:
        mutation["canonicalSpecSha256"] = hashlib.sha256(
            candidate_spec.encode("utf-8")
        ).hexdigest()
        mutation["schemaSha256"] = hashlib.sha256(schema_bytes).hexdigest()
        mutation["contentIdentitySha256"] = canonical_identity(mutation)
        controls.append((label, mutation, candidate_spec))

    mutation = copy.deepcopy(contract); mutation["unexpected"] = True
    add("unknown-root", mutation)
    mutation = copy.deepcopy(contract); mutation["tcbA"] = "S0-R"
    add("tcb-expansion", mutation)
    mutation = copy.deepcopy(contract); mutation["layerOrder"] = list(reversed(LAYER_ORDER))
    add("layer-order", mutation)
    mutation = copy.deepcopy(contract); mutation["residualHostAssumptions"].pop()
    add("undeclared-assumption", mutation)
    mutation = copy.deepcopy(contract); mutation["stage0Domains"].pop()
    add("missing-domain", mutation)
    mutation = copy.deepcopy(contract); mutation["stage0Domains"][1]["id"] = "S0-Z"
    add("unknown-domain", mutation)
    mutation = copy.deepcopy(contract); mutation["stage0Domains"][1]["id"] = "S0-K"
    add("duplicate-domain", mutation)
    mutation = copy.deepcopy(contract); mutation["stage0Domains"][0], mutation["stage0Domains"][1] = mutation["stage0Domains"][1], mutation["stage0Domains"][0]
    add("domain-order", mutation)
    mutation = copy.deepcopy(contract); mutation["stage0Domains"][1]["tcbA"] = True
    add("domain-tcb-expansion", mutation)
    mutation = copy.deepcopy(contract); mutation["stage0Domains"][0]["implementationEvidence"] = []
    add("missing-anchor", mutation)
    mutation = copy.deepcopy(contract); mutation["stage0Domains"][0]["implementationEvidence"][0] = "missing/stage0.rs"
    add("nonexistent-anchor", mutation)
    mutation = copy.deepcopy(contract); mutation["stage0Domains"][0]["implementationEvidence"][0] = "README.md"
    add("existing-anchor-substitution", mutation)
    mutation = copy.deepcopy(contract); mutation["stage0Domains"][3]["forbiddenAuthority"].remove("self-issued-equivalence")
    add("lost-forbidden-authority", mutation)
    mutation = copy.deepcopy(contract); mutation["stage0Domains"][0]["authority"].append("future-authority")
    add("authority-expansion", mutation)
    mutation = copy.deepcopy(contract); mutation["stage0Domains"][0]["demotionTasks"] = ["R9.4.f"]
    add("demotion-path-substitution", mutation)
    mutation = copy.deepcopy(contract); mutation["stage0Domains"][0]["name"] = "renamed-kernel"
    add("domain-name-substitution", mutation)
    mutation = copy.deepcopy(contract); mutation["contentIdentitySha256"] = "0" * 64
    controls.append(("content-identity", mutation, spec_text))
    mutation = copy.deepcopy(contract)
    add(
        "broad-crate-tcb",
        mutation,
        spec_text + "\nTCB-A kernel crates (`gc_coreform`, `gc_kernel`, `gc_prelude`)\n",
    )
    mutation = copy.deepcopy(contract)
    add(
        "missing-spec-claim",
        mutation,
        spec_text.replace("Only S0-K is TCB-A", "S0-K is one trust domain", 1),
    )
    for label, candidate, candidate_spec in controls:
        try:
            validate_contract(root, candidate, candidate_spec, schema_bytes)
        except ContractError:
            continue
        fail(f"self-test accepted mutation: {label}")

    duplicate = '{"kind":"one","kind":"two"}'
    try:
        json.loads(duplicate, object_pairs_hook=pairs_no_duplicates)
    except ContractError:
        pass
    else:
        fail("self-test accepted duplicate JSON key")
    return len(controls) + 1


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--root", type=pathlib.Path, default=pathlib.Path.cwd())
    parser.add_argument("--contract", type=pathlib.Path, required=True)
    parser.add_argument("--schema", type=pathlib.Path, required=True)
    parser.add_argument("--spec", type=pathlib.Path, required=True)
    parser.add_argument("--self-test", action="store_true")
    args = parser.parse_args()
    root = args.root.resolve()
    contract = load_json(args.contract)
    try:
        schema_bytes = args.schema.read_bytes()
    except OSError as error:
        fail(f"cannot read {args.schema}: {error}")
    schema = load_json(args.schema)
    validate_schema(schema)
    try:
        spec_text = args.spec.read_text(encoding="utf-8")
    except OSError as error:
        fail(f"cannot read {args.spec}: {error}")
    validate_contract(root, contract, spec_text, schema_bytes)
    controls = run_self_test(root, contract, spec_text, schema_bytes) if args.self_test else 0
    suffix = f" negative_controls={controls}" if args.self_test else ""
    print(f"stage0-trust-contract: ok (domains={len(DOMAIN_IDS)}{suffix})")
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except ContractError as error:
        raise SystemExit(f"stage0-trust-contract: {error}")
