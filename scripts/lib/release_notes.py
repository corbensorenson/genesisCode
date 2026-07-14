#!/usr/bin/env python3
"""Render and verify release notes from canonical repository authorities."""

from __future__ import annotations

import argparse
import copy
from hashlib import sha256
import json
from pathlib import Path
import re
import sys
from typing import Any, Dict, Mapping, Sequence, Tuple

from toml_compat import tomllib


ROOT = Path(__file__).resolve().parents[2]
POLICY_PATH = ROOT / "policies/release_notes_v0.1.json"
POLICY_SCHEMA_PATH = ROOT / "docs/spec/RELEASE_NOTES_POLICY_v0.1.schema.json"
NOTES_SCHEMA_PATH = ROOT / "docs/spec/RELEASE_NOTES_v0.1.schema.json"
SHA256_RE = re.compile(r"^[0-9a-f]{64}$")
HOST_PATH_RE = re.compile(r"(?:/Users/|/home/|/private/|[A-Za-z]:\\\\)")


class ReleaseNotesError(ValueError):
    pass


def reject_duplicate_keys(pairs: Sequence[Tuple[str, Any]]) -> Dict[str, Any]:
    result: Dict[str, Any] = {}
    for key, value in pairs:
        if key in result:
            raise ReleaseNotesError("duplicate JSON key: " + key)
        result[key] = value
    return result


def load_json(path: Path) -> Any:
    try:
        return json.loads(
            path.read_text(encoding="utf-8"), object_pairs_hook=reject_duplicate_keys
        )
    except FileNotFoundError as exc:
        raise ReleaseNotesError("missing input: " + display_path(path)) from exc
    except json.JSONDecodeError as exc:
        raise ReleaseNotesError(
            "invalid JSON in {}:{}:{}: {}".format(
                display_path(path), exc.lineno, exc.colno, exc.msg
            )
        ) from exc


def display_path(path: Path) -> str:
    try:
        return path.resolve().relative_to(ROOT).as_posix()
    except ValueError:
        return path.as_posix()


def require(condition: bool, message: str) -> None:
    if not condition:
        raise ReleaseNotesError(message)


def require_closed(value: Mapping[str, Any], fields: Sequence[str], label: str) -> None:
    require(isinstance(value, dict), label + " must be an object")
    observed = set(value)
    expected = set(fields)
    require(
        observed == expected,
        "{} fields drift: missing={} unknown={}".format(
            label, sorted(expected - observed), sorted(observed - expected)
        ),
    )


def require_relative_file(value: str, label: str) -> Path:
    require(isinstance(value, str) and value, label + " must be a path")
    path = Path(value)
    require(not path.is_absolute() and ".." not in path.parts, label + " must be relative")
    resolved = ROOT / path
    require(resolved.is_file() and not resolved.is_symlink(), label + " must name a regular file")
    return resolved


def canonical_bytes(value: Any) -> bytes:
    return json.dumps(value, sort_keys=True, separators=(",", ":"), ensure_ascii=True).encode("utf-8")


def digest(path: Path) -> str:
    return sha256(path.read_bytes()).hexdigest()


def validate_schema_contract(path: Path, expected_id: str) -> None:
    schema = load_json(path)
    require(isinstance(schema, dict), display_path(path) + " must be an object")
    require(schema.get("$schema") == "https://json-schema.org/draft/2020-12/schema", display_path(path) + " draft drift")
    require(schema.get("$id") == expected_id, display_path(path) + " id drift")
    require(schema.get("type") == "object" and schema.get("additionalProperties") is False, display_path(path) + " root must be closed")

    def walk(value: Any, location: str) -> None:
        if isinstance(value, dict):
            if value.get("type") == "object":
                require("additionalProperties" in value, location + " object schema is open")
                if value["additionalProperties"] is not False:
                    require("propertyNames" in value, location + " dynamic map lacks a key contract")
            for key, child in value.items():
                walk(child, location + "/" + key)
        elif isinstance(value, list):
            for index, child in enumerate(value):
                walk(child, location + "/" + str(index))

    walk(schema, display_path(path))


def load_policy() -> Mapping[str, Any]:
    policy = load_json(POLICY_PATH)
    require_closed(
        policy,
        (
            "authority",
            "baselineFixtureRole",
            "dependencyLockfiles",
            "kind",
            "output",
            "securityGateAuthority",
            "securityGateEntrypoints",
            "sourcePaths",
            "version",
        ),
        "release-note policy",
    )
    require(policy["kind"] == "genesis/release-notes-policy-v0.1" and policy["version"] == "0.1", "release-note policy identity drift")
    require_closed(policy["authority"], ("documentClass", "runtimeGateClaims", "unqualifiedCapabilityRule"), "policy.authority")
    require(policy["authority"]["documentClass"] == "E1", "release notes must remain E1")
    require(policy["authority"]["runtimeGateClaims"] == "requirements-only-unverified", "static notes cannot claim runtime gate success")
    require_closed(policy["output"], ("changelog", "endMarker", "json", "startMarker"), "policy.output")
    for field in ("sourcePaths", "dependencyLockfiles", "securityGateEntrypoints"):
        values = policy[field]
        require(isinstance(values, list) and values, "policy." + field + " must be non-empty")
        require(values == sorted(values) and len(values) == len(set(values)), "policy." + field + " must be sorted and unique")
    for index, rel in enumerate(policy["sourcePaths"]):
        require_relative_file(rel, "policy.sourcePaths[{}]".format(index))
    for index, rel in enumerate(policy["dependencyLockfiles"]):
        require_relative_file(rel, "policy.dependencyLockfiles[{}]".format(index))
    require(policy["baselineFixtureRole"] == "signed-e0-baseline", "baseline fixture role drift")
    require(policy["securityGateAuthority"] == "genesis.gates.json", "security gate authority drift")
    require(policy["output"]["changelog"] == "CHANGELOG.md", "changelog output drift")
    require(policy["output"]["startMarker"] != policy["output"]["endMarker"], "changelog markers collide")
    return policy


def cargo_lock_summary(rel: str) -> Mapping[str, Any]:
    path = require_relative_file(rel, "dependency lockfile")
    try:
        data = tomllib.loads(path.read_text(encoding="utf-8"))
    except Exception as exc:
        raise ReleaseNotesError("invalid lockfile {}: {}".format(rel, exc)) from exc
    packages = data.get("package")
    require(isinstance(packages, list), rel + " must contain [[package]] records")
    sources = [pkg.get("source") for pkg in packages]
    registry = [source for source in sources if isinstance(source, str) and source.startswith("registry+")]
    git = [source for source in sources if isinstance(source, str) and source.startswith("git+")]
    require(len(registry) + len(git) + sources.count(None) == len(sources), rel + " contains an unsupported dependency source")
    identities = {
        (pkg.get("name"), pkg.get("version"), pkg.get("source")) for pkg in packages
    }
    require(all(isinstance(name, str) and isinstance(version, str) for name, version, _ in identities), rel + " has malformed package identity")
    return {
        "gitPackageRecords": len(git),
        "pathOrWorkspacePackageRecords": sources.count(None),
        "path": rel,
        "registryPackageRecords": len(registry),
        "schemaVersion": data.get("version"),
        "sha256": digest(path),
        "totalPackageRecords": len(packages),
        "uniquePackageIdentities": len(identities),
    }


def capability_is_eligible(claim: Mapping[str, Any], tier1_ids: Sequence[str]) -> bool:
    immutable = claim["immutable_evidence_ids"]
    return (
        claim["maturity"] == "L5"
        and all(claim["maturity_by_platform"][platform] == "L5" for platform in tier1_ids)
        and bool(immutable)
        and all(item.startswith(("E3:", "E4:")) for item in immutable)
    )


def render_document() -> Mapping[str, Any]:
    policy = load_policy()
    versions = load_json(ROOT / "genesis.version-surfaces.json")
    compatibility = load_json(ROOT / "genesis.compatibility.json")
    ledger = load_json(ROOT / "docs/spec/CAPABILITY_EVIDENCE_LEDGER_v0.1.json")
    fixtures = load_json(ROOT / "docs/program/EVIDENCE_FIXTURE_CLASSIFICATION_v0.1.json")
    mirror = load_json(ROOT / "genesis.dependency-mirror.json")
    package_lock = load_json(ROOT / "package-lock.json")
    gates = load_json(ROOT / policy["securityGateAuthority"])

    release_train = versions.get("release_train")
    require(isinstance(release_train, str), "version surfaces release train is missing")
    output_name = Path(policy["output"]["json"]).name
    require(release_train in output_name, "release-note output does not identify the release train")

    surfaces = []
    for item in versions["surfaces"]:
        surfaces.append(
            {
                "acceptedReaders": item["accepted_readers"],
                "currentWriter": item["current_writer"],
                "id": item["id"],
                "kind": item["kind"],
                "migrations": item["migrations"],
            }
        )
    compatibility_entries = []
    for item in compatibility["entries"]:
        compatibility_entries.append(
            {
                "candidateId": item["candidateId"],
                "compatibilityClass": item["compatibilityClass"],
                "components": item["components"],
                "dependencies": item["dependencies"],
                "key": item["key"],
                "stableId": item["stableId"],
                "state": item["state"],
            }
        )

    tier1_ids = [item["id"] for item in ledger["platforms"] if item["tier"] == 1]
    claims = []
    authorized = []
    for item in ledger["claims"]:
        eligible = capability_is_eligible(item, tier1_ids)
        if eligible:
            authorized.append(item["id"])
        claims.append(
            {
                "evidenceIds": item["evidence_ids"],
                "gapIds": item["gap_ids"],
                "id": item["id"],
                "immutableEvidenceIds": item["immutable_evidence_ids"],
                "limitations": item["limitations"],
                "maturity": item["maturity"],
                "maturityByPlatform": item["maturity_by_platform"],
                "releaseClaimEligible": eligible,
                "selfhostLevel": item["selfhost_level"],
                "title": item["title"],
            }
        )

    gap_claims = {gap_id: [] for gap_id in ledger["gap_catalog"]}
    for claim in ledger["claims"]:
        for gap_id in claim["gap_ids"]:
            gap_claims[gap_id].append(claim["id"])
    gaps = [
        {
            "affectedClaims": sorted(gap_claims[gap_id]),
            "id": gap_id,
            "summary": summary,
        }
        for gap_id, summary in sorted(ledger["gap_catalog"].items())
    ]

    fixture_rows = fixtures["files"]
    baselines = []
    dynamic_sources = []
    for fixture in fixture_rows:
        path = require_relative_file(fixture["path"], "evidence fixture")
        require(digest(path) == fixture["sha256"], "evidence fixture hash drift: " + fixture["path"])
        dynamic_sources.append(fixture["path"])
        if fixture["role"] == policy["baselineFixtureRole"]:
            bundle = load_json(path)
            require(bundle["evidenceClass"] == "E0" and bundle["authority"] == "observation", "baseline authority escalation")
            require(bundle["signing"]["signatureGrantsAuthority"] is False, "baseline signature authority escalation")
            baselines.append(
                {
                    "authority": bundle["authority"],
                    "baselineIdentitySha256": bundle["statement"]["baselineIdentitySha256"],
                    "evidenceClass": bundle["evidenceClass"],
                    "overall": bundle["statement"]["overall"],
                    "path": fixture["path"],
                    "signatureGrantsAuthority": False,
                }
            )
    require(baselines, "release notes require at least one signed E0 baseline")

    npm_packages = package_lock.get("packages")
    require(isinstance(npm_packages, dict), "package-lock.json packages must be an object")
    external_npm = [key for key in npm_packages if key]
    integrity_algorithms = sorted(
        {
            value["integrity"].split("-", 1)[0]
            for key, value in npm_packages.items()
            if key and isinstance(value, dict) and isinstance(value.get("integrity"), str)
        }
    )

    gate_by_path = {item["entrypoint"]: item for item in gates["gates"]}
    mandatory_checks = []
    for entrypoint in policy["securityGateEntrypoints"]:
        require(entrypoint in gate_by_path, "security gate absent from manifest: " + entrypoint)
        gate = gate_by_path[entrypoint]
        require(gate["readOnly"] is True, "security gate is not read-only: " + entrypoint)
        mandatory_checks.append(
            {
                "claim": "required-but-not-attested",
                "entrypoint": entrypoint,
                "executionIdentitySha256": gate["executionIdentitySha256"],
                "gateId": gate["id"],
                "kind": gate["kind"],
                "networkMode": gate["network"]["mode"],
                "profile": gate["profile"],
            }
        )

    sources = sorted(set(policy["sourcePaths"]) | set(dynamic_sources))
    source_identities = {
        rel: digest(require_relative_file(rel, "release-note source")) for rel in sources
    }
    document = {
        "authority": copy.deepcopy(policy["authority"]),
        "kind": "genesis/release-notes-v0.1",
        "releaseTrain": release_train,
        "sections": {
            "capabilityEvidence": {
                "authorizedUnqualifiedClaims": authorized,
                "claims": claims,
                "platforms": ledger["platforms"],
            },
            "compatibility": {
                "entries": compatibility_entries,
                "releaseClaim": compatibility["releaseClaim"],
                "surfaces": surfaces,
            },
            "dependencies": {
                "lockfiles": [cargo_lock_summary(rel) for rel in policy["dependencyLockfiles"]],
                "mirror": {
                    "authorityFiles": mirror["authorityFiles"],
                    "fetchPolicy": mirror["fetchPolicy"],
                    "offlineChecks": mirror["offlineChecks"],
                    "storeMode": mirror["mirror"]["storeMode"],
                },
                "npm": {
                    "externalPackageRecords": len(external_npm),
                    "integrityAlgorithms": integrity_algorithms,
                    "lockfileVersion": package_lock.get("lockfileVersion"),
                    "path": "package-lock.json",
                    "sha256": digest(ROOT / "package-lock.json"),
                },
            },
            "evidence": {
                "baselines": baselines,
                "fixtureAuthority": fixtures["authority"],
                "fixtureDistributionClass": fixtures["distributionClass"],
                "fixtures": fixture_rows,
            },
            "knownGaps": {
                "activeDefectIds": ledger["active_defect_ids"],
                "roadmapGaps": gaps,
            },
            "migrations": {"entries": versions["migrations"]},
            "security": {
                "activeDefectIds": ledger["active_defect_ids"],
                "claimState": "requirements-only-unverified",
                "mandatoryChecks": mandatory_checks,
                "passedChecks": [],
                "policySha256": digest(ROOT / "deny.toml"),
            },
        },
        "sourceIdentities": source_identities,
        "version": "0.1",
    }
    document["contentIdentitySha256"] = sha256(canonical_bytes(document)).hexdigest()
    return document


def render_json() -> str:
    return json.dumps(render_document(), indent=2, sort_keys=True, ensure_ascii=True) + "\n"


def render_markdown() -> str:
    doc = render_document()
    sections = doc["sections"]
    lines = [
        "### Generated Release Facts",
        "",
        "This block is generated from canonical repository inputs by `scripts/update_release_notes.sh`. It is E1 traceability, not runtime or release authority.",
        "",
        "#### Compatibility",
        "",
        "V1 registry claim: `{}`. Reserved IDs are not stable compatibility promises.".format(sections["compatibility"]["releaseClaim"]),
        "",
        "| Surface | Current writer | Accepted readers | Migrations |",
        "|---|---|---|---|",
    ]
    for surface in sections["compatibility"]["surfaces"]:
        lines.append(
            "| `{}` | `{}` | {} | {} |".format(
                surface["id"],
                surface["currentWriter"].replace("|", "\\|"),
                ", ".join("`{}`".format(value) for value in surface["acceptedReaders"]),
                ", ".join("`{}`".format(value) for value in surface["migrations"]) or "none",
            )
        )
    lines.extend(["", "#### Migration Notes", ""])
    for item in sections["migrations"]["entries"]:
        lines.append(
            "- `{}`: {} User action: {} Support: {}".format(
                item["id"], item["semantic_delta"], item["user_action"], item["retirement"]
            )
        )
    lines.extend(["", "#### Known Gaps", ""])
    for item in sections["knownGaps"]["roadmapGaps"]:
        lines.append(
            "- `{}`: {} Affected claims: {}.".format(
                item["id"],
                item["summary"],
                ", ".join("`{}`".format(value) for value in item["affectedClaims"]),
            )
        )
    active = sections["knownGaps"]["activeDefectIds"]
    lines.append("- Active P0/P1 defect IDs: {}. Roadmap gaps above remain open.".format(", ".join("`{}`".format(value) for value in active) or "none"))
    evidence = sections["evidence"]
    capabilities = sections["capabilityEvidence"]
    lines.extend(
        [
            "",
            "#### Evidence",
            "",
            "- Release-note authority: `E1`; runtime gate results are not asserted.",
            "- Retained fixture distribution class: `{}`; fixture authority: `{}`.".format(evidence["fixtureDistributionClass"], str(evidence["fixtureAuthority"]).lower()),
            "- Authorized unqualified capability claims: {}.".format(", ".join("`{}`".format(value) for value in capabilities["authorizedUnqualifiedClaims"]) or "none"),
        ]
    )
    for baseline in evidence["baselines"]:
        overall = baseline["overall"]
        lines.append(
            "- Baseline `{}` is signed `{}` observation only: {} passing, {} failing, {} unavailable, {} decision-gated; its signature grants no authority.".format(
                baseline["baselineIdentitySha256"],
                baseline["evidenceClass"],
                overall["budgetPassing"],
                overall["budgetFailing"],
                overall["runnerUnavailable"],
                overall["decisionGated"],
            )
        )
    lines.extend(["", "#### Dependencies", "", "| Lockfile | Records | Registry | Git | SHA-256 |", "|---|---:|---:|---:|---|"])
    for lock in sections["dependencies"]["lockfiles"]:
        lines.append("| `{}` | {} | {} | {} | `{}` |".format(lock["path"], lock["totalPackageRecords"], lock["registryPackageRecords"], lock["gitPackageRecords"], lock["sha256"]))
    npm = sections["dependencies"]["npm"]
    lines.append("| `{}` | {} | {} integrity | 0 | `{}` |".format(npm["path"], npm["externalPackageRecords"], ",".join(npm["integrityAlgorithms"]), npm["sha256"]))
    lines.extend(["", "#### Security", "", "No security gate is represented as passed by this static document. Release authority requires fresh retained evidence for every mandatory check:", ""])
    for gate in sections["security"]["mandatoryChecks"]:
        lines.append("- `{}` (`{}`, `{}`, network `{}`): required, not attested here.".format(gate["entrypoint"], gate["kind"], gate["profile"], gate["networkMode"]))
    lines.extend(["", "Machine-readable identity: `{}`.".format(doc["contentIdentitySha256"])])
    return "\n".join(lines) + "\n"


def validate_candidate(candidate: Any) -> None:
    expected = render_document()
    require(isinstance(candidate, dict), "release-note artifact must be an object")
    require(candidate == expected, "release-note artifact contains stale or unsupported claims")
    identity = candidate.get("contentIdentitySha256")
    require(isinstance(identity, str) and SHA256_RE.fullmatch(identity) is not None, "release-note identity is malformed")
    material = dict(candidate)
    del material["contentIdentitySha256"]
    require(sha256(canonical_bytes(material)).hexdigest() == identity, "release-note content identity mismatch")
    rendered = json.dumps(candidate, sort_keys=True, ensure_ascii=True)
    require(HOST_PATH_RE.search(rendered) is None, "release-note artifact leaks an absolute host path")


def changelog_block(text: str, start: str, end: str) -> str:
    require(text.count(start) == 1 and text.count(end) == 1, "CHANGELOG generated markers must occur exactly once")
    before, remainder = text.split(start, 1)
    body, after = remainder.split(end, 1)
    require(before.find("## [Unreleased]") >= 0, "generated release notes must remain under Unreleased")
    require(after.find("## [0.2.0]") >= 0, "released changelog history must remain after generated notes")
    return body.lstrip("\n").rstrip("\n") + "\n"


def replace_changelog_block(text: str, start: str, end: str, generated: str) -> str:
    require(text.count(start) == 1 and text.count(end) == 1, "CHANGELOG generated markers must occur exactly once")
    prefix, remainder = text.split(start, 1)
    _, suffix = remainder.split(end, 1)
    require("## [Unreleased]" in prefix, "generated release notes must remain under Unreleased")
    require("## [0.2.0]" in suffix, "released changelog history must remain after generated notes")
    return prefix + start + "\n" + generated.rstrip("\n") + "\n" + end + suffix


def render_changelog() -> str:
    policy = load_policy()
    changelog = require_relative_file(policy["output"]["changelog"], "release-note changelog")
    return replace_changelog_block(
        changelog.read_text(encoding="utf-8"),
        policy["output"]["startMarker"],
        policy["output"]["endMarker"],
        render_markdown(),
    )


def check() -> None:
    policy = load_policy()
    validate_schema_contract(POLICY_SCHEMA_PATH, "https://genesiscode.dev/schemas/release-notes-policy-v0.1.json")
    validate_schema_contract(NOTES_SCHEMA_PATH, "https://genesiscode.dev/schemas/release-notes-v0.1.json")
    output = require_relative_file(policy["output"]["json"], "release-note output")
    retained_text = output.read_text(encoding="utf-8")
    require(retained_text == render_json(), "release-note JSON is stale; run: bash scripts/update_release_notes.sh")
    candidate = load_json(output)
    validate_candidate(candidate)
    changelog = require_relative_file(policy["output"]["changelog"], "release-note changelog")
    observed = changelog_block(changelog.read_text(encoding="utf-8"), policy["output"]["startMarker"], policy["output"]["endMarker"])
    require(observed == render_markdown(), "generated CHANGELOG release notes are stale; run: bash scripts/update_release_notes.sh")
    require(render_changelog() == changelog.read_text(encoding="utf-8"), "release-note updater is not idempotent")
    require(render_json() == render_json() and render_markdown() == render_markdown(), "release-note rendering is nondeterministic")
    print(
        "release-notes: ok (release={} compatibility={} migrations={} gaps={} claims={} security_checks={} identity={})".format(
            candidate["releaseTrain"],
            len(candidate["sections"]["compatibility"]["entries"]),
            len(candidate["sections"]["migrations"]["entries"]),
            len(candidate["sections"]["knownGaps"]["roadmapGaps"]),
            len(candidate["sections"]["capabilityEvidence"]["claims"]),
            len(candidate["sections"]["security"]["mandatoryChecks"]),
            candidate["contentIdentitySha256"],
        )
    )


def self_test() -> None:
    expected = render_document()
    controls = []

    def reject(label: str, mutate: Any) -> None:
        candidate = copy.deepcopy(expected)
        mutate(candidate)
        try:
            validate_candidate(candidate)
        except ReleaseNotesError:
            controls.append(label)
        else:
            raise ReleaseNotesError("self-test accepted " + label)

    reject("compatibility-escalation", lambda d: d["sections"]["compatibility"].__setitem__("releaseClaim", "stable"))
    reject("migration-omission", lambda d: d["sections"]["migrations"]["entries"].pop())
    reject("gap-omission", lambda d: d["sections"]["knownGaps"]["roadmapGaps"].pop())
    reject("capability-escalation", lambda d: d["sections"]["capabilityEvidence"]["claims"][0].__setitem__("releaseClaimEligible", True))
    reject("limitation-omission", lambda d: d["sections"]["capabilityEvidence"]["claims"][0].__setitem__("limitations", ""))
    reject("evidence-authority-escalation", lambda d: d["sections"]["evidence"].__setitem__("fixtureAuthority", True))
    reject("baseline-authority-escalation", lambda d: d["sections"]["evidence"]["baselines"][0].__setitem__("signatureGrantsAuthority", True))
    reject("dependency-hash-tamper", lambda d: d["sections"]["dependencies"]["lockfiles"][0].__setitem__("sha256", "0" * 64))
    reject("security-success-injection", lambda d: d["sections"]["security"]["passedChecks"].append("gate/supply-chain"))
    reject("security-gate-omission", lambda d: d["sections"]["security"]["mandatoryChecks"].pop())
    reject("source-identity-tamper", lambda d: d["sourceIdentities"].__setitem__(next(iter(d["sourceIdentities"])), "0" * 64))
    reject("unknown-field", lambda d: d.__setitem__("trustMe", True))
    reject("host-path-injection", lambda d: d["sections"]["capabilityEvidence"]["claims"][0].__setitem__("limitations", "/Users/example/private"))
    reject("content-identity-tamper", lambda d: d.__setitem__("contentIdentitySha256", "0" * 64))
    try:
        json.loads('{"kind":"a","kind":"b"}', object_pairs_hook=reject_duplicate_keys)
    except ReleaseNotesError:
        controls.append("duplicate-json-key")
    else:
        raise ReleaseNotesError("self-test accepted duplicate-json-key")
    policy = load_policy()
    start = policy["output"]["startMarker"]
    end = policy["output"]["endMarker"]
    valid = "## [Unreleased]\n\n{}\nold\n{}\n\n## [0.2.0]\nreleased\n".format(start, end)
    for label, candidate in (
        ("duplicate-changelog-marker", valid.replace(start, start + "\n" + start, 1)),
        ("released-history-before-generated", valid.replace("## [Unreleased]", "## [0.2.0]", 1).replace("## [0.2.0]\nreleased", "## [Unreleased]\nreleased", 1)),
    ):
        try:
            replace_changelog_block(candidate, start, end, "generated")
        except ReleaseNotesError:
            controls.append(label)
        else:
            raise ReleaseNotesError("self-test accepted " + label)
    require(len(controls) == 17, "release-note self-test control inventory drift")
    print("release-notes: self-test ok (negative_controls={})".format(len(controls)))


def main(argv: Sequence[str]) -> int:
    parser = argparse.ArgumentParser()
    mode = parser.add_mutually_exclusive_group(required=True)
    mode.add_argument("--render-json", action="store_true")
    mode.add_argument("--render-markdown", action="store_true")
    mode.add_argument("--render-changelog", action="store_true")
    mode.add_argument("--check", action="store_true")
    mode.add_argument("--self-test", action="store_true")
    args = parser.parse_args(argv)
    try:
        if args.render_json:
            sys.stdout.write(render_json())
        elif args.render_markdown:
            sys.stdout.write(render_markdown())
        elif args.render_changelog:
            sys.stdout.write(render_changelog())
        elif args.check:
            check()
        else:
            self_test()
    except ReleaseNotesError as exc:
        print("release-notes: " + str(exc), file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
