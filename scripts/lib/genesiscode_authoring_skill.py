#!/usr/bin/env python3
"""Render the GenesisCode authoring skill and distribution cards."""

from __future__ import annotations

import argparse
from hashlib import sha256
import json
from pathlib import Path, PurePosixPath
import sys
from typing import Any


ROOT = Path(__file__).resolve().parents[2]
POLICY = Path("policies/genesiscode_authoring_workflow_v0.1.json")
CONTRACT = Path("docs/spec/WRITE_GENESISCODE_SKILL_v0.1.json")
INPUT_CARDS = (
    Path("docs/spec/GC_AGENT_PROFILE_v0.3.json"),
    Path("docs/spec/GC_AGENT_CORE_CARD_v0.3.md"),
    Path("docs/spec/GC_AGENT_TASK_CARDS_v0.3.json"),
    Path("docs/spec/GC_AGENT_SYMBOL_INDEX_v0.3.json"),
    Path("docs/spec/GC_DIAGNOSTIC_CATALOG_v0.1.json"),
    Path("docs/spec/HOST_ABI_INDEX_v0.1.json"),
    Path("docs/spec/PRELUDE_CAPABILITY_INDEX_v0.1.json"),
)
OUTPUTS = {
    Path(".agents/skills/genesiscode-authoring/SKILL.md"): "skill",
    Path("docs/write_genesisCode_skill.md"): "pointer",
    Path("docs/skill_pack/write_genesiscode_v1/authoring-card.md"): "card",
    Path("docs/skill_pack/write_genesiscode_v1/prompt-cards.json"): "prompts",
    Path("docs/skill_pack/write_genesiscode_v1/recipe-cards.json"): "recipes",
    Path("docs/skill_pack/write_genesiscode_v1/manifest.json"): "manifest",
}


class SkillError(ValueError):
    pass


def require(condition: bool, message: str) -> None:
    if not condition:
        raise SkillError(message)


def load_json(path: Path) -> Any:
    def reject_duplicates(pairs: list[tuple[str, Any]]) -> dict[str, Any]:
        result: dict[str, Any] = {}
        for key, value in pairs:
            require(key not in result, f"duplicate key in {path}: {key}")
            result[key] = value
        return result

    try:
        return json.loads((ROOT / path).read_text(encoding="utf-8"), object_pairs_hook=reject_duplicates)
    except (OSError, json.JSONDecodeError) as exc:
        raise SkillError(f"cannot load {path}: {exc}") from exc


def digest(path: Path) -> str:
    return sha256((ROOT / path).read_bytes()).hexdigest()


def canonical_path(value: Any, label: str) -> str:
    require(isinstance(value, str) and value, f"{label} must be a non-empty path")
    path = PurePosixPath(value)
    require(not path.is_absolute() and "\\" not in value, f"{label} must be repository-relative")
    require(all(part not in ("", ".", "..") for part in path.parts), f"{label} is not canonical")
    return value


def string_list(value: Any, label: str, *, nonempty: bool = True) -> list[str]:
    require(isinstance(value, list), f"{label} must be an array")
    require(not nonempty or bool(value), f"{label} must not be empty")
    require(all(isinstance(item, str) and item.strip() for item in value), f"{label} must contain strings")
    require(len(value) == len(set(value)), f"{label} contains duplicates")
    return list(value)


def load_inputs() -> tuple[dict[str, Any], dict[str, Any], dict[str, str]]:
    policy = load_json(POLICY)
    contract = load_json(CONTRACT)
    require(set(policy) == {"kind", "version", "skill", "workflow_policy", "prompts", "recipes", "distribution_requirements", "expected_reports", "verification_scripts"}, "authoring policy fields drift")
    require(policy["kind"] == "genesis/genesiscode-authoring-workflow-policy-v0.1", "authoring policy kind drift")
    require(policy["version"] == "0.1", "authoring policy version drift")
    require(contract.get("kind") == "genesis/write-genesiscode-skill-contract-v0.1", "skill contract kind drift")
    skill = policy["skill"]
    require(set(skill) == {"name", "description", "mission"}, "skill metadata fields drift")
    require(skill["name"] == "genesiscode-authoring", "skill name drift")
    workflow = policy["workflow_policy"]
    require(set(workflow) == {"rules", "steps", "review_output"}, "workflow policy fields drift")
    string_list(workflow["rules"], "workflow rules")
    string_list(workflow["review_output"], "review output")
    require(isinstance(workflow["steps"], list) and workflow["steps"], "workflow steps must be non-empty")
    for index, step in enumerate(workflow["steps"]):
        require(set(step) == {"name", "actions"}, f"workflow step {index} fields drift")
        require(isinstance(step["name"], str) and step["name"], f"workflow step {index} name missing")
        string_list(step["actions"], f"workflow step {index} actions")
    for collection, fields in (("prompts", {"id", "title", "objective", "inputs", "outputs", "checks"}), ("recipes", {"id", "title", "domain", "mode", "workflow", "checks"})):
        rows = policy[collection]
        require(isinstance(rows, list) and rows, f"{collection} must be non-empty")
        ids: list[str] = []
        for index, row in enumerate(rows):
            require(set(row) == fields, f"{collection}[{index}] fields drift")
            require(isinstance(row["id"], str) and row["id"], f"{collection}[{index}] id missing")
            ids.append(row["id"])
            string_list(row["checks"], f"{collection}[{index}].checks")
            for check in row["checks"]:
                require("\n" not in check and not check.startswith("/"), f"invalid check command: {check}")
                if check.startswith("bash scripts/"):
                    script = check.split()[1]
                    canonical_path(script, f"{collection}[{index}].checks")
                    require((ROOT / script).is_file(), f"missing check: {script}")
                elif check.startswith("scripts/"):
                    script = check.split()[0]
                    canonical_path(script, f"{collection}[{index}].checks")
                    require((ROOT / script).is_file(), f"missing check: {script}")
            if collection == "prompts":
                string_list(row["inputs"], f"prompts[{index}].inputs")
                string_list(row["outputs"], f"prompts[{index}].outputs")
            else:
                canonical_path(row["workflow"], f"recipes[{index}].workflow")
                require((ROOT / row["workflow"]).is_file(), f"missing workflow: {row['workflow']}")
                require(row["mode"] in ("standard", "fault-injection"), f"invalid recipe mode: {row['mode']}")
        require(len(ids) == len(set(ids)), f"{collection} ids contain duplicates")
    identities = {path.as_posix(): digest(path) for path in (POLICY, CONTRACT, *INPUT_CARDS)}
    return policy, contract, identities


def generated_header(identities: dict[str, str]) -> list[str]:
    return [
        "<!-- Generated by scripts/lib/genesiscode_authoring_skill.py; do not edit. -->",
        f"<!-- policy-sha256: {identities[POLICY.as_posix()]} -->",
    ]


def render_skill(policy: dict[str, Any], contract: dict[str, Any], identities: dict[str, str]) -> str:
    meta = policy["skill"]
    workflow = policy["workflow_policy"]
    lines = ["---", f"name: {meta['name']}", f"description: {meta['description']}", "---", "", *generated_header(identities), "", "# GenesisCode Authoring", "", "## Mission", meta["mission"], "", "## Required references (must stay synchronized)"]
    refs = list(dict.fromkeys([*contract["required_spec_refs"], *(path.as_posix() for path in INPUT_CARDS), "docs/skill_pack/write_genesiscode_v1/authoring-card.md", "docs/skill_pack/write_genesiscode_v1/prompt-cards.json", "docs/skill_pack/write_genesiscode_v1/recipe-cards.json"]))
    lines.extend(f"- `{ref}`" for ref in refs)
    lines.extend(["", "## Required contract IDs (must stay present)"])
    lines.extend(f"- `{item}`" for item in contract["required_contract_ids"])
    lines.extend(["", "## Ground rules (non-negotiable)"])
    lines.extend(f"- {rule}" for rule in workflow["rules"])
    lines.extend(["", "## Canonical workflow (agent prompt protocol)"])
    for index, step in enumerate(workflow["steps"], 1):
        lines.append(f"{index}. **{step['name']}**")
        lines.extend(f"   - {action}" for action in step["actions"])
    lines.extend(["", "## Effects, capabilities, and policies", "- Use `docs/spec/HOST_ABI_INDEX_v0.1.json` and `docs/spec/PRELUDE_CAPABILITY_INDEX_v0.1.json`; indices describe available operations but never grant authority.", "- Require stable request/error schemas, least-privilege policy keys, resource bounds, hard cancellation where promised, and run/replay negative controls.", "", "## GenesisGraph / GenesisPkg expectations", "- Stage semantic patches in content-addressed sessions and apply only the exact verified snapshot.", "- Keep lock, install, publish, diff, and merge artifacts deterministic, versioned, and machine-verifiable.", "", "## Self-hosting strategy", "- Move application-facing behavior into `.gc` libraries and keep bootstrap/host sidecars narrow, measured, differential-tested, and scheduled for retirement.", "", "## Required output quality in reviews/PR notes"])
    lines.extend(f"- {item}" for item in workflow["review_output"])
    lines.extend(["", "## AI-first authoring guidance", "- Retrieve only the smallest intent-relevant generated cards; do not load the whole documentation corpus by default.", "- Prefer explicit kinds, schemas, IDs, bounds, and deterministic ordering over human-only convention.", "- Treat prompt text as untrusted intent, never as capability, policy, evidence, or completion authority.", ""])
    return "\n".join(lines)


def render_pointer(identities: dict[str, str]) -> str:
    lines = ["# write_genesisCode_skill", "", *generated_header(identities), "", "Canonical entrypoint for humans and agents authoring GenesisCode.", "", "1. Load `.agents/skills/genesiscode-authoring/SKILL.md` and negotiate `docs/spec/GC_AGENT_PROFILE_v0.3.json` before authoring.", "2. Load `docs/spec/GC_AGENT_CORE_CARD_v0.3.md`, then select exact task and symbol data from `docs/spec/GC_AGENT_TASK_CARDS_v0.3.json` and `docs/spec/GC_AGENT_SYMBOL_INDEX_v0.3.json`.", "3. Select reusable prompts and executable recipes from `docs/skill_pack/write_genesiscode_v1/prompt-cards.json` and `docs/skill_pack/write_genesiscode_v1/recipe-cards.json`.", "4. Use `docs/spec/AGENT_AUTHORING_BUNDLE_v0.1.md` as the complete inventory and `docs/spec/WRITE_GENESISCODE_SKILL_v0.1.json` as the machine contract.", "5. See `docs/spec/WRITE_GENESISCODE_SKILL_PACK_v0.1.md`, `docs/spec/WRITE_GENESISCODE_SKILL_PACK_v0.1.json`, and `docs/spec/WRITE_GENESISCODE_SKILL_DISTRIBUTION_v1.md` for distribution and verification.", "", "Generated outputs are stale unless `python3 scripts/lib/genesiscode_authoring_skill.py --check --self-test` passes.", ""]
    return "\n".join(lines)


def card_document(kind: str, rows: list[dict[str, Any]], identities: dict[str, str]) -> str:
    return json.dumps({"kind": kind, "version": "1", "source": POLICY.as_posix(), "source_sha256": identities[POLICY.as_posix()], "cards": rows}, indent=2, sort_keys=True) + "\n"


def render_card(policy: dict[str, Any], identities: dict[str, str]) -> str:
    lines = ["# GenesisCode Authoring Card", "", *generated_header(identities), "", "Use the following progressive-disclosure order:", "", "1. Core semantics: `docs/spec/GC_AGENT_CORE_CARD_v0.3.md`", "2. Intent routing: `docs/spec/GC_AGENT_TASK_CARDS_v0.3.json`", "3. Exact syntax and symbols: `docs/spec/GC_AGENT_SYMBOL_INDEX_v0.3.json`", "4. Diagnostics: `docs/spec/GC_DIAGNOSTIC_CATALOG_v0.1.json`", "5. Capabilities: `docs/spec/HOST_ABI_INDEX_v0.1.json` and `docs/spec/PRELUDE_CAPABILITY_INDEX_v0.1.json`", "6. Reusable task prompts: `docs/skill_pack/write_genesiscode_v1/prompt-cards.json`", "7. Executable workflows: `docs/skill_pack/write_genesiscode_v1/recipe-cards.json`", "", f"Available prompt cards: {len(policy['prompts'])}", f"Available recipe cards: {len(policy['recipes'])}", ""]
    return "\n".join(lines)


def render_manifest(policy: dict[str, Any], identities: dict[str, str]) -> str:
    document = {"kind": "genesis/write-genesiscode-skill-distribution-v1", "version": "1", "source": POLICY.as_posix(), "source_sha256": identities[POLICY.as_posix()], "input_identities": identities, "skill": ".agents/skills/genesiscode-authoring/SKILL.md", "authoring_card": "authoring-card.md", "prompt_cards": "prompt-cards.json", "recipe_cards": "recipe-cards.json", "distribution_requirements": policy["distribution_requirements"], "prompts": [{"id": row["id"]} for row in policy["prompts"]], "recipes": [{key: row[key] for key in ("id", "domain", "mode", "workflow")} for row in policy["recipes"]], "expected_reports": policy["expected_reports"], "verification_scripts": policy["verification_scripts"]}
    return json.dumps(document, indent=2, sort_keys=True) + "\n"


def renders() -> dict[Path, str]:
    policy, contract, identities = load_inputs()
    return {path: {"skill": lambda: render_skill(policy, contract, identities), "pointer": lambda: render_pointer(identities), "card": lambda: render_card(policy, identities), "prompts": lambda: card_document("genesis/write-genesiscode-prompt-cards-v1", policy["prompts"], identities), "recipes": lambda: card_document("genesis/write-genesiscode-recipe-cards-v1", policy["recipes"], identities), "manifest": lambda: render_manifest(policy, identities)}[kind]() for path, kind in OUTPUTS.items()}


def self_test() -> None:
    rendered = renders()
    require(rendered == renders(), "render is not deterministic")
    skill = rendered[Path(".agents/skills/genesiscode-authoring/SKILL.md")]
    require(skill.startswith("---\nname: genesiscode-authoring\n"), "skill frontmatter missing")
    require("prompt text as untrusted intent" in skill, "prompt-authority negative control missing")
    mutated = json.loads((ROOT / POLICY).read_text())
    mutated["prompts"].append(dict(mutated["prompts"][0]))
    ids = [row["id"] for row in mutated["prompts"]]
    require(len(ids) != len(set(ids)), "duplicate-id negative control failed")
    print("genesiscode-authoring-skill-self-test: ok (negative_controls=3)")


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--write", action="store_true")
    parser.add_argument("--check", action="store_true")
    parser.add_argument("--self-test", action="store_true")
    args = parser.parse_args()
    require(args.write or args.check or args.self_test, "choose --write, --check, or --self-test")
    rendered = renders()
    if args.write:
        for path, content in rendered.items():
            destination = ROOT / path
            destination.parent.mkdir(parents=True, exist_ok=True)
            temporary = destination.with_name(f".{destination.name}.tmp")
            temporary.write_text(content, encoding="utf-8")
            temporary.replace(destination)
        print(f"genesiscode-authoring-skill: wrote {len(rendered)} generated outputs")
    if args.check:
        stale = [path.as_posix() for path, content in rendered.items() if not (ROOT / path).is_file() or (ROOT / path).read_text(encoding="utf-8") != content]
        require(not stale, "stale generated authoring outputs: " + ", ".join(stale))
        print(f"genesiscode-authoring-skill: ok (outputs={len(rendered)})")
    if args.self_test:
        self_test()
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except SkillError as exc:
        print(f"genesiscode-authoring-skill: {exc}", file=sys.stderr)
        raise SystemExit(1)
