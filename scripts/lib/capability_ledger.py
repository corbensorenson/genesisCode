#!/usr/bin/env python3
"""Validate the capability ledger and render its derived status views."""

from __future__ import annotations

import argparse
import json
import os
from pathlib import Path
import re
import sys
import tempfile
from typing import Any, Dict, Iterable, List, Mapping, Sequence, Tuple


ROOT = Path(__file__).resolve().parents[2]
DEFAULT_LEDGER = ROOT / "docs/spec/CAPABILITY_EVIDENCE_LEDGER_v0.1.json"
DEFAULT_MATRIX = ROOT / "feature_matrix.md"
DEFAULT_EVIDENCE_JSON = ROOT / "docs/spec/FEATURE_MATRIX_EVIDENCE_v0.1.json"
DEFAULT_EVIDENCE_MD = ROOT / "docs/spec/FEATURE_MATRIX_EVIDENCE_v0.1.md"
DEFAULT_PRODUCT_TARGET_JSON = ROOT / "docs/spec/PRODUCT_TARGET_MATRIX_v0.1.json"
DEFAULT_SELFHOST_STATUS = ROOT / "docs/status/SELFHOST_AUTHORITY_v0.1.md"
DEFAULT_REDTEAM_STATUS = ROOT / "docs/status/REDTEAM_REPORT.md"

MATURITY_LEVELS = tuple(f"L{i}" for i in range(6))
MATURITY_RANK = {level: index for index, level in enumerate(MATURITY_LEVELS)}
SELFHOST_LEVELS = ("N/A", "H0", "H1", "H2", "H3", "H4")
CLAIM_ID_RE = re.compile(r"^CAP-[A-Z0-9]+(?:-[A-Z0-9]+)*$")
TARGET_ID_RE = re.compile(r"^TARGET-[A-Z0-9]+(?:-[A-Z0-9]+)*$")
SCOPE_ID_RE = re.compile(r"^[a-z0-9]+(?:-[a-z0-9]+)*$")
GAP_ID_RE = re.compile(r"^(?:P\d+\.\d+|R\d+\.\d+(?:\.[a-z])?|F\d+(?:\.[a-z])?)$")
EVIDENCE_ID_RE = re.compile(r"^E[1-4]:[a-z0-9][a-z0-9._/-]*$")
DATE_RE = re.compile(r"^\d{4}-\d{2}-\d{2}$")
TARGET_SCOPE_KINDS = ("browser", "host", "runtime", "simulator", "device", "board", "lab")
TARGET_RELEASE_STATUS_BY_MATURITY = {
    "L0": "unsupported",
    "L1": "experimental",
    "L2": "experimental",
    "L3": "experimental",
    "L4": "candidate",
    "L5": "qualified",
}
REQUIRED_AGGREGATE_CLAIM_IDS = {
    "CAP-DEPLOYMENT-PIPELINE",
    "CAP-DOMAIN-STARTERS",
    "CAP-GRAPHICS-RUNTIME",
    "CAP-RUNTIME-SURFACES",
}


class LedgerError(ValueError):
    pass


def reject_duplicate_keys(pairs: Sequence[Tuple[str, Any]]) -> Dict[str, Any]:
    result: Dict[str, Any] = {}
    for key, value in pairs:
        if key in result:
            raise LedgerError(f"duplicate JSON key: {key}")
        result[key] = value
    return result


def load_json(path: Path) -> Any:
    try:
        return json.loads(
            path.read_text(encoding="utf-8"), object_pairs_hook=reject_duplicate_keys
        )
    except FileNotFoundError as exc:
        raise LedgerError(f"missing ledger: {display_path(path)}") from exc
    except json.JSONDecodeError as exc:
        raise LedgerError(
            f"invalid JSON in {display_path(path)}:{exc.lineno}:{exc.colno}: {exc.msg}"
        ) from exc


def display_path(path: Path) -> str:
    try:
        return path.resolve().relative_to(ROOT).as_posix()
    except ValueError:
        return path.as_posix()


def require_object(value: Any, label: str) -> Mapping[str, Any]:
    if not isinstance(value, dict):
        raise LedgerError(f"{label} must be an object")
    return value


def require_string(value: Any, label: str) -> str:
    if not isinstance(value, str) or not value.strip():
        raise LedgerError(f"{label} must be a non-empty string")
    return value


def require_string_list(
    value: Any, label: str, *, non_empty: bool = False
) -> List[str]:
    if not isinstance(value, list):
        raise LedgerError(f"{label} must be an array")
    if non_empty and not value:
        raise LedgerError(f"{label} must not be empty")
    result: List[str] = []
    seen = set()
    for index, item in enumerate(value):
        text = require_string(item, f"{label}[{index}]")
        if text in seen:
            raise LedgerError(f"{label} contains duplicate value: {text}")
        seen.add(text)
        result.append(text)
    return result


def reject_unknown_fields(
    value: Mapping[str, Any], allowed: Iterable[str], label: str
) -> None:
    unknown = sorted(set(value) - set(allowed))
    if unknown:
        raise LedgerError(f"{label} contains unknown fields: {', '.join(unknown)}")


def validate_relative_path(value: str, label: str) -> Path:
    path = Path(value)
    if path.is_absolute() or ".." in path.parts:
        raise LedgerError(f"{label} must be a repository-relative path: {value}")
    resolved = ROOT / path
    if not resolved.exists():
        raise LedgerError(f"{label} does not exist: {value}")
    return resolved


def parse_open_upgrade_ids() -> List[str]:
    plan = ROOT / "upgrade_plan.md"
    if not plan.is_file():
        raise LedgerError("missing upgrade_plan.md")
    ids = []
    for line in plan.read_text(encoding="utf-8").splitlines():
        match = re.match(r"^- \[ \] (P\d+\.\d+)\b", line)
        if match:
            ids.append(match.group(1))
    return sorted(set(ids))


def validate_roadmap_gap_ids(gap_ids: Iterable[str]) -> None:
    roadmap_path = ROOT / "ROADMAP.md"
    if not roadmap_path.is_file():
        raise LedgerError("missing ROADMAP.md required for roadmap gap validation")
    roadmap = roadmap_path.read_text(encoding="utf-8")
    missing = []
    for gap_id in gap_ids:
        if gap_id.startswith(("R", "F")) and f"**{gap_id} " not in roadmap:
            missing.append(gap_id)
    if missing:
        raise LedgerError(
            "gap catalog references task IDs absent from ROADMAP.md: "
            + ", ".join(sorted(missing))
        )


def validate_ledger(doc: Any) -> Mapping[str, Any]:
    ledger = require_object(doc, "ledger")
    reject_unknown_fields(
        ledger,
        (
            "kind",
            "version",
            "audit_date",
            "platforms",
            "aggregate_claim_ids",
            "active_defect_ids",
            "gap_catalog",
            "primary_evidence_paths",
            "claims",
            "product_target_claims",
        ),
        "ledger",
    )
    if ledger.get("kind") != "genesis/capability-evidence-ledger-v0.1":
        raise LedgerError(
            "ledger.kind must be genesis/capability-evidence-ledger-v0.1"
        )
    if ledger.get("version") != "0.1":
        raise LedgerError("ledger.version must be 0.1")
    audit_date = require_string(ledger.get("audit_date"), "ledger.audit_date")
    if not DATE_RE.fullmatch(audit_date):
        raise LedgerError("ledger.audit_date must use YYYY-MM-DD")

    platforms_raw = ledger.get("platforms")
    if not isinstance(platforms_raw, list) or not platforms_raw:
        raise LedgerError("ledger.platforms must be a non-empty array")
    platform_ids: List[str] = []
    tier1_platform_ids: List[str] = []
    for index, raw in enumerate(platforms_raw):
        platform = require_object(raw, f"ledger.platforms[{index}]")
        reject_unknown_fields(
            platform, ("id", "label", "tier"), f"ledger.platforms[{index}]"
        )
        platform_id = require_string(
            platform.get("id"), f"ledger.platforms[{index}].id"
        )
        require_string(platform.get("label"), f"ledger.platforms[{index}].label")
        tier = platform.get("tier")
        if tier not in (1, 2):
            raise LedgerError(f"platform {platform_id} tier must be 1 or 2")
        if platform_id in platform_ids:
            raise LedgerError(f"duplicate platform id: {platform_id}")
        platform_ids.append(platform_id)
        if tier == 1:
            tier1_platform_ids.append(platform_id)
    if not tier1_platform_ids:
        raise LedgerError("ledger must declare at least one tier-1 platform")

    gap_catalog = require_object(ledger.get("gap_catalog"), "ledger.gap_catalog")
    if not gap_catalog:
        raise LedgerError("ledger.gap_catalog must not be empty")
    for gap_id, summary in gap_catalog.items():
        if not GAP_ID_RE.fullmatch(gap_id):
            raise LedgerError(f"invalid gap id: {gap_id}")
        require_string(summary, f"ledger.gap_catalog[{gap_id}]")
    validate_roadmap_gap_ids(gap_catalog.keys())

    active_defect_ids = require_string_list(
        ledger.get("active_defect_ids"), "ledger.active_defect_ids"
    )
    for defect_id in active_defect_ids:
        if not re.fullmatch(r"P\d+\.\d+", defect_id):
            raise LedgerError(f"invalid active defect id: {defect_id}")
    open_upgrade_ids = parse_open_upgrade_ids()
    if sorted(active_defect_ids) != open_upgrade_ids:
        raise LedgerError(
            "active_defect_ids must exactly match open upgrade_plan.md IDs: "
            f"ledger={sorted(active_defect_ids)} plan={open_upgrade_ids}"
        )

    primary_paths = require_string_list(
        ledger.get("primary_evidence_paths"),
        "ledger.primary_evidence_paths",
        non_empty=True,
    )
    for index, path in enumerate(primary_paths):
        validate_relative_path(path, f"ledger.primary_evidence_paths[{index}]")

    claims_raw = ledger.get("claims")
    if not isinstance(claims_raw, list) or not claims_raw:
        raise LedgerError("ledger.claims must be a non-empty array")
    claim_ids = set()
    titles = set()
    all_evidence_ids = set()
    referenced_gap_ids = set(active_defect_ids)
    for index, raw in enumerate(claims_raw):
        claim = require_object(raw, f"ledger.claims[{index}]")
        label = f"ledger.claims[{index}]"
        reject_unknown_fields(
            claim,
            (
                "id",
                "title",
                "category",
                "owner",
                "maturity",
                "maturity_by_platform",
                "selfhost_level",
                "spec_paths",
                "implementation_paths",
                "host_binding_paths",
                "check_paths",
                "evidence_ids",
                "immutable_evidence_ids",
                "gap_ids",
                "limitations",
            ),
            label,
        )
        claim_id = require_string(claim.get("id"), f"{label}.id")
        if not CLAIM_ID_RE.fullmatch(claim_id):
            raise LedgerError(f"invalid capability id: {claim_id}")
        if claim_id in claim_ids:
            raise LedgerError(f"duplicate capability id: {claim_id}")
        claim_ids.add(claim_id)
        title = require_string(claim.get("title"), f"{label}.title")
        if title in titles:
            raise LedgerError(f"duplicate capability title: {title}")
        titles.add(title)
        require_string(claim.get("category"), f"{label}.category")
        require_string(claim.get("owner"), f"{label}.owner")

        maturity = claim.get("maturity")
        if maturity not in MATURITY_LEVELS:
            raise LedgerError(f"{claim_id}.maturity must be L0-L5")
        maturity_by_platform = require_object(
            claim.get("maturity_by_platform"), f"{claim_id}.maturity_by_platform"
        )
        if set(maturity_by_platform) != set(platform_ids):
            raise LedgerError(
                f"{claim_id}.maturity_by_platform must contain exactly: "
                + ", ".join(platform_ids)
            )
        for platform_id, level in maturity_by_platform.items():
            if level not in MATURITY_LEVELS:
                raise LedgerError(
                    f"{claim_id}.maturity_by_platform[{platform_id}] must be L0-L5"
                )
        tier1_min = min(
            MATURITY_RANK[maturity_by_platform[platform_id]]
            for platform_id in tier1_platform_ids
        )
        if MATURITY_RANK[maturity] > tier1_min:
            raise LedgerError(
                f"{claim_id}.maturity exceeds its lowest tier-1 platform maturity"
            )

        selfhost_level = claim.get("selfhost_level")
        if selfhost_level not in SELFHOST_LEVELS:
            raise LedgerError(
                f"{claim_id}.selfhost_level must be one of {', '.join(SELFHOST_LEVELS)}"
            )

        path_fields = (
            ("spec_paths", True),
            ("implementation_paths", True),
            ("host_binding_paths", False),
            ("check_paths", True),
        )
        for field, non_empty in path_fields:
            paths = require_string_list(
                claim.get(field), f"{claim_id}.{field}", non_empty=non_empty
            )
            for path_index, path in enumerate(paths):
                validate_relative_path(path, f"{claim_id}.{field}[{path_index}]")

        evidence_ids = require_string_list(
            claim.get("evidence_ids"), f"{claim_id}.evidence_ids", non_empty=True
        )
        immutable_ids = require_string_list(
            claim.get("immutable_evidence_ids"),
            f"{claim_id}.immutable_evidence_ids",
        )
        for evidence_id in evidence_ids + immutable_ids:
            if not EVIDENCE_ID_RE.fullmatch(evidence_id):
                raise LedgerError(f"invalid evidence id for {claim_id}: {evidence_id}")
            if evidence_id in all_evidence_ids:
                raise LedgerError(f"duplicate evidence id: {evidence_id}")
            all_evidence_ids.add(evidence_id)
        if len(evidence_ids) != len(claim["check_paths"]):
            raise LedgerError(
                f"{claim_id}.evidence_ids must map one-to-one to check_paths"
            )
        for evidence_id in immutable_ids:
            if not evidence_id.startswith(("E3:", "E4:")):
                raise LedgerError(
                    f"{claim_id}.immutable_evidence_ids may contain only E3/E4 IDs"
                )
        if maturity == "L5" and not immutable_ids:
            raise LedgerError(f"{claim_id} is L5 but has no immutable evidence IDs")

        gaps = require_string_list(
            claim.get("gap_ids"), f"{claim_id}.gap_ids", non_empty=maturity != "L5"
        )
        for gap_id in gaps:
            if gap_id not in gap_catalog:
                raise LedgerError(f"{claim_id} references unknown gap id: {gap_id}")
            referenced_gap_ids.add(gap_id)
        limitations = claim.get("limitations")
        if not isinstance(limitations, str):
            raise LedgerError(f"{claim_id}.limitations must be a string")
        if maturity != "L5" and not limitations.strip():
            raise LedgerError(f"{claim_id}.limitations must not be empty below L5")

    aggregate_claim_ids = set(
        require_string_list(
            ledger.get("aggregate_claim_ids"),
            "ledger.aggregate_claim_ids",
            non_empty=True,
        )
    )
    unknown_aggregates = sorted(aggregate_claim_ids - claim_ids)
    if unknown_aggregates:
        raise LedgerError(
            "aggregate_claim_ids references unknown foundation claims: "
            + ", ".join(unknown_aggregates)
        )
    missing_aggregates = sorted(REQUIRED_AGGREGATE_CLAIM_IDS - aggregate_claim_ids)
    if missing_aggregates:
        raise LedgerError(
            "aggregate_claim_ids must classify broad non-product claims: "
            + ", ".join(missing_aggregates)
        )

    product_targets_raw = ledger.get("product_target_claims")
    if not isinstance(product_targets_raw, list) or not product_targets_raw:
        raise LedgerError("ledger.product_target_claims must be a non-empty array")
    target_ids = set()
    target_profiles = set()
    target_titles = set()
    for index, raw in enumerate(product_targets_raw):
        target = require_object(raw, f"ledger.product_target_claims[{index}]")
        label = f"ledger.product_target_claims[{index}]"
        reject_unknown_fields(
            target,
            (
                "id",
                "title",
                "product_family",
                "target_profile",
                "owner",
                "maturity",
                "release_status",
                "scopes",
                "authentic_artifact_predicate",
                "one_language_source_predicate",
                "related_foundation_claim_ids",
                "spec_paths",
                "implementation_paths",
                "check_paths",
                "evidence_ids",
                "immutable_evidence_ids",
                "gap_ids",
                "limitations",
            ),
            label,
        )
        target_id = require_string(target.get("id"), f"{label}.id")
        if not TARGET_ID_RE.fullmatch(target_id):
            raise LedgerError(f"invalid product target id: {target_id}")
        if target_id in target_ids:
            raise LedgerError(f"duplicate product target id: {target_id}")
        target_ids.add(target_id)
        title = require_string(target.get("title"), f"{target_id}.title")
        if title in target_titles:
            raise LedgerError(f"duplicate product target title: {title}")
        target_titles.add(title)
        require_string(target.get("product_family"), f"{target_id}.product_family")
        profile = require_string(target.get("target_profile"), f"{target_id}.target_profile")
        if not SCOPE_ID_RE.fullmatch(profile):
            raise LedgerError(f"invalid target profile for {target_id}: {profile}")
        if profile in target_profiles:
            raise LedgerError(f"duplicate product target profile: {profile}")
        target_profiles.add(profile)
        require_string(target.get("owner"), f"{target_id}.owner")

        maturity = target.get("maturity")
        if maturity not in MATURITY_LEVELS:
            raise LedgerError(f"{target_id}.maturity must be L0-L5")
        expected_status = TARGET_RELEASE_STATUS_BY_MATURITY[maturity]
        if target.get("release_status") != expected_status:
            raise LedgerError(
                f"{target_id}.release_status must be {expected_status} at {maturity}"
            )

        scopes_raw = target.get("scopes")
        if not isinstance(scopes_raw, list) or not scopes_raw:
            raise LedgerError(f"{target_id}.scopes must be a non-empty array")
        scope_ids = set()
        required_scope_levels = []
        for scope_index, raw_scope in enumerate(scopes_raw):
            scope = require_object(raw_scope, f"{target_id}.scopes[{scope_index}]")
            scope_label = f"{target_id}.scopes[{scope_index}]"
            reject_unknown_fields(
                scope,
                ("id", "label", "kind", "required_for_release", "maturity"),
                scope_label,
            )
            scope_id = require_string(scope.get("id"), f"{scope_label}.id")
            if not SCOPE_ID_RE.fullmatch(scope_id):
                raise LedgerError(f"invalid scope id for {target_id}: {scope_id}")
            if scope_id in scope_ids:
                raise LedgerError(f"duplicate scope id for {target_id}: {scope_id}")
            scope_ids.add(scope_id)
            require_string(scope.get("label"), f"{scope_label}.label")
            if scope.get("kind") not in TARGET_SCOPE_KINDS:
                raise LedgerError(
                    f"{scope_label}.kind must be one of {', '.join(TARGET_SCOPE_KINDS)}"
                )
            if not isinstance(scope.get("required_for_release"), bool):
                raise LedgerError(f"{scope_label}.required_for_release must be boolean")
            scope_maturity = scope.get("maturity")
            if scope_maturity not in MATURITY_LEVELS:
                raise LedgerError(f"{scope_label}.maturity must be L0-L5")
            if scope["required_for_release"]:
                required_scope_levels.append(MATURITY_RANK[scope_maturity])
        if not required_scope_levels:
            raise LedgerError(f"{target_id} must have at least one release-required scope")
        if MATURITY_RANK[maturity] > min(required_scope_levels):
            raise LedgerError(
                f"{target_id}.maturity exceeds its lowest release-required scope maturity"
            )

        for predicate_name in (
            "authentic_artifact_predicate",
            "one_language_source_predicate",
        ):
            predicate = require_string(target.get(predicate_name), f"{target_id}.{predicate_name}")
            if len(predicate) < 20:
                raise LedgerError(f"{target_id}.{predicate_name} must be specific and testable")

        related = require_string_list(
            target.get("related_foundation_claim_ids"),
            f"{target_id}.related_foundation_claim_ids",
            non_empty=True,
        )
        unknown_related = sorted(set(related) - claim_ids)
        if unknown_related:
            raise LedgerError(
                f"{target_id} references unknown foundation claims: "
                + ", ".join(unknown_related)
            )

        for field in ("spec_paths", "implementation_paths", "check_paths"):
            paths = require_string_list(target.get(field), f"{target_id}.{field}", non_empty=True)
            for path_index, path in enumerate(paths):
                validate_relative_path(path, f"{target_id}.{field}[{path_index}]")
        evidence_ids = require_string_list(
            target.get("evidence_ids"), f"{target_id}.evidence_ids", non_empty=True
        )
        immutable_ids = require_string_list(
            target.get("immutable_evidence_ids"), f"{target_id}.immutable_evidence_ids"
        )
        if len(evidence_ids) != len(target["check_paths"]):
            raise LedgerError(f"{target_id}.evidence_ids must map one-to-one to check_paths")
        for evidence_id in evidence_ids + immutable_ids:
            if not EVIDENCE_ID_RE.fullmatch(evidence_id):
                raise LedgerError(f"invalid evidence id for {target_id}: {evidence_id}")
            if evidence_id in all_evidence_ids:
                raise LedgerError(f"duplicate evidence id: {evidence_id}")
            all_evidence_ids.add(evidence_id)
        for evidence_id in immutable_ids:
            if not evidence_id.startswith(("E3:", "E4:")):
                raise LedgerError(
                    f"{target_id}.immutable_evidence_ids may contain only E3/E4 IDs"
                )
        if maturity == "L5" and not immutable_ids:
            raise LedgerError(f"{target_id} is L5 but has no immutable evidence IDs")
        gaps = require_string_list(
            target.get("gap_ids"), f"{target_id}.gap_ids", non_empty=maturity != "L5"
        )
        for gap_id in gaps:
            if gap_id not in gap_catalog:
                raise LedgerError(f"{target_id} references unknown gap id: {gap_id}")
            referenced_gap_ids.add(gap_id)
        limitations = target.get("limitations")
        if not isinstance(limitations, str):
            raise LedgerError(f"{target_id}.limitations must be a string")
        if maturity != "L5" and not limitations.strip():
            raise LedgerError(f"{target_id}.limitations must not be empty below L5")

    stale_gaps = sorted(set(gap_catalog) - referenced_gap_ids)
    if stale_gaps:
        raise LedgerError(
            "gap_catalog contains unreferenced IDs: " + ", ".join(stale_gaps)
        )
    return ledger


def render_matrix(ledger: Mapping[str, Any]) -> str:
    platforms = ledger["platforms"]
    claims = ledger["claims"]
    lines = [
        f"# GenesisCode Feature Matrix (Audit Date: {ledger['audit_date']})",
        "",
        "<!-- GENERATED by scripts/update_capability_status_views.sh from docs/spec/CAPABILITY_EVIDENCE_LEDGER_v0.1.json. DO NOT EDIT. -->",
        "",
        "Scope: capability maturity for AI-agent autonomy, semantic selfhost closure, and production runtime trust.",
        "",
        "This is a generated status view, not release evidence. The canonical source is `docs/spec/CAPABILITY_EVIDENCE_LEDGER_v0.1.json`.",
        "Product and deployment qualification is reported in this document's generated product/target section and `docs/spec/PRODUCT_TARGET_MATRIX_v0.1.json`; foundation maturity never implies a child target.",
        "",
        "Maturity legend:",
        "- `L0` specified",
        "- `L1` implemented/reachable",
        "- `L2` verified on the named reference host",
        "- `L3` reproducible on the supported host matrix",
        "- `L4` product-proven under declared SLOs",
        "- `L5` release-attested with immutable evidence",
        "",
        "Selfhost legend: `H0` routed, `H1` GenesisCode implementation, `H2` GenesisCode production authority, `H3` reproducible bootstrap fixpoint, `H4` independently conformant, `N/A` deliberately outside selfhost closure.",
        "",
        "## Supported Host Scope",
        "",
        "| Platform ID | Host | Tier |",
        "|---|---|---|",
    ]
    for platform in platforms:
        lines.append(
            f"| `{platform['id']}` | {platform['label']} | {platform['tier']} |"
        )

    platform_headers = " | ".join(platform["label"] for platform in platforms)
    lines.extend(
        [
            "",
            "## Capability Maturity",
            "",
            f"| ID | Capability | Class | Overall | {platform_headers} | Selfhost | Owner |",
            "|---|---|---|---|" + "---|" * len(platforms) + "---|---|",
        ]
    )
    for claim in claims:
        platform_cells = " | ".join(
            claim["maturity_by_platform"][platform["id"]]
            for platform in platforms
        )
        lines.append(
            f"| `{claim['id']}` | {claim['title']} | "
            f"{'aggregate foundation' if claim['id'] in ledger['aggregate_claim_ids'] else 'foundation'} | "
            f"**{claim['maturity']}** | "
            f"{platform_cells} | {claim['selfhost_level']} | `{claim['owner']}` |"
        )

    tier1 = [platform for platform in platforms if platform["tier"] == 1]
    release_eligible = []
    lines.extend(
        [
            "",
            "## Release Claim Eligibility",
            "",
            "A capability is eligible for an unqualified v1 release claim only at L5 on every required tier-1 platform. Lower levels must be described with their platform and evidence limitations.",
            "",
            "| ID | Capability | Overall | Tier-1 Maturity | Immutable Evidence | Eligible |",
            "|---|---|---|---|---|---|",
        ]
    )
    for claim in claims:
        tier1_levels = ", ".join(
            f"{platform['label']}={claim['maturity_by_platform'][platform['id']]}"
            for platform in tier1
        )
        immutable = "<br>".join(
            f"`{item}`" for item in claim["immutable_evidence_ids"]
        ) or "none"
        is_eligible = (
            claim["maturity"] == "L5"
            and all(
                claim["maturity_by_platform"][platform["id"]] == "L5"
                for platform in tier1
            )
            and bool(claim["immutable_evidence_ids"])
        )
        if is_eligible:
            release_eligible.append(claim["id"])
        lines.append(
            f"| `{claim['id']}` | {claim['title']} | {claim['maturity']} | "
            f"{tier1_levels} | {immutable} | {'yes' if is_eligible else '**no**'} |"
        )
    lines.extend(["", "Authorized unqualified claims:"])
    if release_eligible:
        lines.extend(f"- `{claim_id}`" for claim_id in release_eligible)
    else:
        lines.append("- None. No capability currently carries immutable L5 release evidence.")

    gap_to_claims: Dict[str, List[str]] = {gap: [] for gap in ledger["gap_catalog"]}
    for claim in claims:
        for gap_id in claim["gap_ids"]:
            gap_to_claims[gap_id].append(claim["id"])
    for target in ledger["product_target_claims"]:
        for gap_id in target["gap_ids"]:
            gap_to_claims[gap_id].append(target["id"])
    lines.extend(["", "Known GenesisCode gaps", ""])
    for gap_id, summary in ledger["gap_catalog"].items():
        linked = ", ".join(f"`{claim_id}`" for claim_id in gap_to_claims[gap_id])
        lines.append(f"- `{gap_id}`: {summary} Affected claims: {linked}")
    if not ledger["active_defect_ids"]:
        lines.append(
            "- Active `upgrade_plan.md` P0/P1 defects: none. Roadmap maturity gaps above remain open and are not erased by an empty defect queue."
        )

    lines.extend(render_product_target_sections(ledger))
    lines.extend(["", "Primary evidence paths:", ""])
    for path in ledger["primary_evidence_paths"]:
        lines.append(f"- `{path}`")
    lines.extend(
        [
            "",
            "## Interpretation",
            "",
            "- A path or gate mapping establishes traceability; it does not by itself raise maturity.",
            "- Mutable `.genesis/perf/` reports are E0 local observations and cannot establish L5.",
            "- Overall maturity cannot exceed the weakest required tier-1 platform.",
            "- Selfhost levels describe semantic authority, not the existence of a `.gc` wrapper.",
            "- Aggregate foundation claims report shared plumbing only. They cannot authorize any product or target listed in the product/target matrix.",
            "- `upgrade_plan.md` is the active P0/P1 defect queue; `ROADMAP.md` contains strategic maturity gaps.",
        ]
    )
    return "\n".join(lines) + "\n"


def render_evidence_json(ledger: Mapping[str, Any]) -> str:
    entries = []
    for claim in ledger["claims"]:
        entries.append(
            {
                "capability_id": claim["id"],
                "capability": claim["title"],
                "maturity": claim["maturity"],
                "maturity_by_platform": claim["maturity_by_platform"],
                "selfhost_level": claim["selfhost_level"],
                "owner": claim["owner"],
                "spec_paths": claim["spec_paths"],
                "implementation_paths": claim["implementation_paths"],
                "host_binding_paths": claim["host_binding_paths"],
                "check_paths": claim["check_paths"],
                "evidence_ids": claim["evidence_ids"],
                "immutable_evidence_ids": claim["immutable_evidence_ids"],
                "gap_ids": claim["gap_ids"],
                "limitations": claim["limitations"],
            }
        )
    output = {
        "kind": "genesis/feature-matrix-evidence-v0.1",
        "version": "0.1",
        "source_ledger": "docs/spec/CAPABILITY_EVIDENCE_LEDGER_v0.1.json",
        "audit_date": ledger["audit_date"],
        "aggregate_foundation_claim_ids": ledger["aggregate_claim_ids"],
        "entry_count": len(entries),
        "entries": entries,
    }
    return json.dumps(output, indent=2, sort_keys=True) + "\n"


def product_target_is_eligible(target: Mapping[str, Any]) -> bool:
    required_scopes = [
        scope for scope in target["scopes"] if scope["required_for_release"]
    ]
    return (
        target["release_status"] == "qualified"
        and target["maturity"] == "L5"
        and bool(required_scopes)
        and all(scope["maturity"] == "L5" for scope in required_scopes)
        and bool(target["immutable_evidence_ids"])
    )


def render_product_target_json(ledger: Mapping[str, Any]) -> str:
    entries = []
    for target in ledger["product_target_claims"]:
        entry = dict(target)
        entry["release_eligible"] = product_target_is_eligible(target)
        entries.append(entry)
    output = {
        "kind": "genesis/product-target-matrix-v0.1",
        "version": "0.1",
        "source_ledger": "docs/spec/CAPABILITY_EVIDENCE_LEDGER_v0.1.json",
        "audit_date": ledger["audit_date"],
        "entry_count": len(entries),
        "aggregate_foundation_claim_ids": ledger["aggregate_claim_ids"],
        "entries": entries,
    }
    return json.dumps(output, indent=2, sort_keys=True) + "\n"


def render_product_target_sections(ledger: Mapping[str, Any]) -> List[str]:
    lines = [
        "",
        "## Product and Target Qualification",
        "",
        "This matrix is the authority for product and target support. Foundation capability rows, host-operation reachability, archive suffixes, descriptors, empty exports, headless plans, synthetic launchers, and simulator-only runs do not raise a product target's maturity.",
        "",
        "Release states are mechanically tied to maturity: `L0` unsupported, `L1-L3` experimental, `L4` candidate, and `L5` qualified. Release eligibility additionally requires L5 on every required scope plus immutable E3/E4 evidence.",
        "",
        "### Qualification Summary",
        "",
        "| ID | Product / target | Family | Profile | Overall | Release state | Scope maturity | Eligible | Owner |",
        "|---|---|---|---|---|---|---|---|---|",
    ]
    for target in ledger["product_target_claims"]:
        scopes = "<br>".join(
            f"`{scope['id']}`={scope['maturity']} ({scope['kind']}; "
            f"{'required' if scope['required_for_release'] else 'optional'})"
            for scope in target["scopes"]
        )
        lines.append(
            f"| `{target['id']}` | {target['title']} | {target['product_family']} | "
            f"`{target['target_profile']}` | **{target['maturity']}** | "
            f"**{target['release_status']}** | {scopes} | "
            f"{'yes' if product_target_is_eligible(target) else '**no**'} | `{target['owner']}` |"
        )
    lines.extend(["", "### Predicates and Evidence", ""])
    for target in ledger["product_target_claims"]:
        related = ", ".join(f"`{item}`" for item in target["related_foundation_claim_ids"])
        gaps = ", ".join(f"`{item}`" for item in target["gap_ids"])
        evidence = ", ".join(f"`{item}`" for item in target["evidence_ids"])
        lines.extend(
            [
                f"#### `{target['id']}`: {target['title']}",
                "",
                f"- **Authentic artifact:** {target['authentic_artifact_predicate']}",
                f"- **One-language source:** {target['one_language_source_predicate']}",
                f"- **Related foundations, not inherited support:** {related}",
                f"- **Current evidence:** {evidence}",
                f"- **Open gaps:** {gaps}",
                f"- **Limitations:** {target['limitations']}",
                "",
            ]
        )
    lines.extend(
        [
            "### Aggregate Claim Boundary",
            "",
            "The following capability claims are aggregate foundations and cannot imply support for any row above:",
            "",
        ]
    )
    lines.extend(f"- `{claim_id}`" for claim_id in ledger["aggregate_claim_ids"])
    return lines


def render_evidence_md(ledger: Mapping[str, Any]) -> str:
    lines = [
        "# Feature Matrix Evidence Ledger v0.1",
        "",
        "<!-- GENERATED by scripts/update_capability_status_views.sh from docs/spec/CAPABILITY_EVIDENCE_LEDGER_v0.1.json. DO NOT EDIT. -->",
        "",
        "Machine-readable source: `docs/spec/FEATURE_MATRIX_EVIDENCE_v0.1.json`.",
        "Generated status view: `feature_matrix.md`.",
        "Canonical source: `docs/spec/CAPABILITY_EVIDENCE_LEDGER_v0.1.json`.",
        "",
        f"- Audit date: `{ledger['audit_date']}`",
        f"- Capability entries: `{len(ledger['claims'])}`",
        "- L5 release-eligible entries: `"
        + str(sum(1 for claim in ledger["claims"] if claim["maturity"] == "L5"))
        + "`",
        "",
        "| ID | Capability | Level | Specs | Implementations | Checks | Evidence IDs | Gaps |",
        "|---|---|---|---|---|---|---|---|",
    ]
    for claim in ledger["claims"]:
        specs = "<br>".join(f"`{path}`" for path in claim["spec_paths"])
        implementations = "<br>".join(
            f"`{path}`" for path in claim["implementation_paths"]
        )
        checks = "<br>".join(f"`{path}`" for path in claim["check_paths"])
        evidence = "<br>".join(f"`{item}`" for item in claim["evidence_ids"])
        gaps = "<br>".join(f"`{item}`" for item in claim["gap_ids"])
        lines.append(
            f"| `{claim['id']}` | {claim['title']} | {claim['maturity']} / "
            f"{claim['selfhost_level']} | {specs} | {implementations} | {checks} | "
            f"{evidence} | {gaps} |"
        )
    return "\n".join(lines) + "\n"


def render_selfhost_status(ledger: Mapping[str, Any]) -> str:
    claims = [
        claim for claim in ledger["claims"] if claim["selfhost_level"] != "N/A"
    ]
    counts = {level: 0 for level in SELFHOST_LEVELS if level != "N/A"}
    for claim in claims:
        counts[claim["selfhost_level"]] += 1
    lines = [
        "# GenesisCode Semantic Selfhost Authority Status v0.1",
        "",
        "<!-- GENERATED by scripts/update_capability_status_views.sh from docs/spec/CAPABILITY_EVIDENCE_LEDGER_v0.1.json. DO NOT EDIT. -->",
        "",
        f"Audit date: `{ledger['audit_date']}`",
        "",
        "This view reports semantic ownership and bootstrap maturity. It is intentionally separate from `docs/status/SELFHOST_CUTOVER.md`, which reports command routing only. A command routed through a `.gc` artifact remains H0 until GenesisCode owns the production semantic decision.",
        "",
        "Levels: `H0` routed, `H1` GenesisCode implementation, `H2` GenesisCode production authority, `H3` reproducible bootstrap fixpoint, `H4` independently conformant.",
        "",
        "## Summary",
        "",
        "| Level | Claims |",
        "|---|---|",
    ]
    for level in ("H0", "H1", "H2", "H3", "H4"):
        lines.append(f"| {level} | {counts[level]} |")
    lines.extend(
        [
            "",
            "## Semantic Authority",
            "",
            "| ID | Capability | Selfhost | Product Maturity | Implementation Authority | Owner | Open Gaps |",
            "|---|---|---|---|---|---|---|",
        ]
    )
    for claim in claims:
        implementations = "<br>".join(
            f"`{path}`" for path in claim["implementation_paths"]
        )
        gaps = "<br>".join(f"`{gap}`" for gap in claim["gap_ids"])
        lines.append(
            f"| `{claim['id']}` | {claim['title']} | **{claim['selfhost_level']}** | "
            f"{claim['maturity']} | {implementations} | `{claim['owner']}` | {gaps} |"
        )
    lines.extend(
        [
            "",
            "## Claim Boundary",
            "",
            "- H0 proves routing only; it does not prove GenesisCode implementation or authority.",
            "- H1 requires a GenesisCode implementation but permits another production authority.",
            "- H2 requires GenesisCode to make the production semantic decision with strict no-fallback evidence.",
            "- H3 additionally requires a reproducible cross-host bootstrap fixpoint.",
            "- H4 additionally requires an independently authored conformant implementation or verifier.",
            "- The minimal pure kernel and unavoidable host effects may remain `N/A` by explicit stage0 contract; moving them into GenesisCode is not required merely to increase a score.",
        ]
    )
    return "\n".join(lines) + "\n"


def render_redteam_status(ledger: Mapping[str, Any]) -> str:
    lines = [
        "# GenesisCode Red-Team Report (P0/P1 Active Risk Summary)",
        "",
        "<!-- GENERATED by scripts/update_capability_status_views.sh from docs/spec/CAPABILITY_EVIDENCE_LEDGER_v0.1.json. DO NOT EDIT. -->",
        "",
        f"Last updated: {ledger['audit_date']}",
        "",
        "Scope:",
        "- Track only unresolved P0/P1 defects from `upgrade_plan.md`.",
        "- Keep exact active IDs synchronized with `docs/spec/CAPABILITY_EVIDENCE_LEDGER_v0.1.json`.",
        "- Do not interpret an empty defect queue as roadmap completion, selfhost authority, cross-host reproducibility, or release readiness.",
        "- Mutable `.genesis/perf/` reports are local observations and are not the authority for this generated status.",
        "",
        "## Active Risks (P0/P1)",
        "",
    ]
    if ledger["active_defect_ids"]:
        lines.extend(f"- `{defect_id}`" for defect_id in ledger["active_defect_ids"])
    else:
        lines.append("No active P0/P1 risks.")
    return "\n".join(lines) + "\n"


def expected_outputs(ledger: Mapping[str, Any]) -> Mapping[Path, str]:
    return {
        DEFAULT_MATRIX: render_matrix(ledger),
        DEFAULT_EVIDENCE_JSON: render_evidence_json(ledger),
        DEFAULT_EVIDENCE_MD: render_evidence_md(ledger),
        DEFAULT_PRODUCT_TARGET_JSON: render_product_target_json(ledger),
        DEFAULT_SELFHOST_STATUS: render_selfhost_status(ledger),
        DEFAULT_REDTEAM_STATUS: render_redteam_status(ledger),
    }


def resolve_output(default: Path, env_name: str) -> Path:
    override = os.environ.get(env_name)
    return Path(override).resolve() if override else default


def configured_outputs(ledger: Mapping[str, Any]) -> Mapping[Path, str]:
    rendered = expected_outputs(ledger)
    return {
        resolve_output(DEFAULT_MATRIX, "GENESIS_FEATURE_MATRIX_PATH"): rendered[
            DEFAULT_MATRIX
        ],
        resolve_output(
            DEFAULT_EVIDENCE_JSON, "GENESIS_FEATURE_MATRIX_EVIDENCE_JSON"
        ): rendered[DEFAULT_EVIDENCE_JSON],
        resolve_output(
            DEFAULT_EVIDENCE_MD, "GENESIS_FEATURE_MATRIX_EVIDENCE_MD"
        ): rendered[DEFAULT_EVIDENCE_MD],
        resolve_output(
            DEFAULT_PRODUCT_TARGET_JSON, "GENESIS_PRODUCT_TARGET_MATRIX_JSON"
        ): rendered[DEFAULT_PRODUCT_TARGET_JSON],
        resolve_output(
            DEFAULT_SELFHOST_STATUS, "GENESIS_SELFHOST_AUTHORITY_STATUS"
        ): rendered[DEFAULT_SELFHOST_STATUS],
        resolve_output(DEFAULT_REDTEAM_STATUS, "GENESIS_REDTEAM_REPORT_FILE"): rendered[
            DEFAULT_REDTEAM_STATUS
        ],
    }


def atomic_write(path: Path, content: str) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    handle, temporary = tempfile.mkstemp(prefix=f".{path.name}.", dir=path.parent)
    try:
        with os.fdopen(handle, "w", encoding="utf-8", newline="\n") as stream:
            stream.write(content)
        os.replace(temporary, path)
    except BaseException:
        try:
            os.unlink(temporary)
        except FileNotFoundError:
            pass
        raise


def check_outputs(outputs: Mapping[Path, str]) -> None:
    stale = []
    for path, expected in outputs.items():
        if not path.is_file() or path.read_text(encoding="utf-8") != expected:
            stale.append(display_path(path))
    if stale:
        raise LedgerError(
            "generated capability view drift: "
            + ", ".join(stale)
            + "; run scripts/update_capability_status_views.sh"
        )


def main(argv: Sequence[str]) -> int:
    parser = argparse.ArgumentParser()
    mode = parser.add_mutually_exclusive_group(required=True)
    mode.add_argument("--check", action="store_true")
    mode.add_argument("--update", action="store_true")
    parser.add_argument(
        "--ledger",
        type=Path,
        default=Path(os.environ.get("GENESIS_CAPABILITY_LEDGER_PATH", DEFAULT_LEDGER)),
    )
    args = parser.parse_args(argv)
    try:
        ledger = validate_ledger(load_json(args.ledger.resolve()))
        outputs = configured_outputs(ledger)
        if args.check:
            check_outputs(outputs)
            print(
                "capability-evidence-ledger: ok "
                f"(claims={len(ledger['claims'])} platforms={len(ledger['platforms'])} "
                f"targets={len(ledger['product_target_claims'])} "
                f"l5={sum(1 for claim in ledger['claims'] if claim['maturity'] == 'L5')})"
            )
        else:
            for path, content in outputs.items():
                atomic_write(path, content)
            print(
                "update-capability-status-views: wrote "
                + ", ".join(display_path(path) for path in outputs)
                + f" (claims={len(ledger['claims'])} targets={len(ledger['product_target_claims'])})"
            )
    except LedgerError as exc:
        print(f"capability-evidence-ledger: {exc}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
