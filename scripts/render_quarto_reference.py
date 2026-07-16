#!/usr/bin/env python3
"""Render exhaustive Quarto reference pages from GenesisCode authorities."""

from __future__ import annotations

import argparse
import hashlib
import html
import json
import re
import sys
from collections import defaultdict
from pathlib import Path
from typing import Any

ROOT = Path(__file__).resolve().parents[1]
GENERATED = ROOT / "reference" / "generated"
SYMBOLS_PATH = ROOT / "docs/spec/GC_AGENT_SYMBOL_INDEX_v0.3.json"
CAPS_PATH = ROOT / "docs/spec/HOST_ABI_INDEX_v0.1.json"
DIAGS_PATH = ROOT / "docs/spec/GC_DIAGNOSTIC_CATALOG_v0.1.json"
EXCLUDED_SOURCE_DIRECTORIES = {
    ".cargo-install-target", ".genesis", ".git", ".quarto", ".tmp",
    "__pycache__", "node_modules", "target",
}


def read_json(path: Path) -> dict[str, Any]:
    return json.loads(path.read_text(encoding="utf-8"))


def sha256(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def md(value: Any) -> str:
    text = str(value).replace("\n", " ").strip()
    return text.replace("|", "\\|").replace("`", "\\`")


def code(value: Any) -> str:
    return f"`{str(value).replace('`', '&#96;')}`"


def slug(value: str) -> str:
    stem = re.sub(r"[^a-z0-9]+", "-", value.lower()).strip("-") or "item"
    digest = hashlib.sha256(value.encode()).hexdigest()[:8]
    return f"{stem}-{digest}"


def title_of(path: Path) -> str:
    if path.suffix == ".json":
        try:
            doc = read_json(path)
            return str(doc.get("title") or doc.get("kind") or path.name)
        except (json.JSONDecodeError, OSError):
            return path.name
    try:
        for line in path.read_text(encoding="utf-8").splitlines():
            if line.startswith("# "):
                return line[2:].strip().replace("`", "")
    except UnicodeDecodeError:
        pass
    return path.name


def bullet_list(values: list[Any], empty: str = "None declared.") -> str:
    if not values:
        return empty
    return "\n".join(f"- {md(v)}" for v in values)


def source_materialization_files(directory: str) -> list[Path]:
    """Enumerate publishable sources identically with or without Git metadata."""
    base = ROOT / directory
    paths = []
    for path in base.rglob("*"):
        relative = path.relative_to(base)
        if any(part in EXCLUDED_SOURCE_DIRECTORIES for part in relative.parts):
            continue
        if path.is_file() and path.name != ".DS_Store" and path.suffix != ".pyc":
            paths.append(path)
    return sorted(paths, key=lambda path: path.relative_to(ROOT).as_posix())


def render_symbols(doc: dict[str, Any]) -> str:
    symbols = doc["symbols"]
    lines = [
        "---",
        'title: "Symbol reference"',
        'description: "Every symbol in the frozen GenesisCode agent profile."',
        "toc: false",
        "---",
        "",
        "# Frozen symbol inventory",
        "",
        f"**{len(symbols)} symbols** · profile {code(doc['profileId'])} · identity {code(doc['indexIdentitySha256'])}",
        "",
        "This page is generated from the closed agent symbol index. Search by exact spelling; symbol names are case-sensitive.",
        "",
        "| Symbol | Kind | Signature | Domain |",
        "|---|---|---|---|",
    ]
    for item in symbols:
        sig = item.get("signature", {})
        anchor = slug(item["symbol"])
        lines.append(
            f"| [{code(item['symbol'])}](#{anchor}) | {md(sig.get('kind', ''))} | {md(sig.get('notation', ''))} | {md(', '.join(item.get('domains', [])))} |"
        )
    for item in symbols:
        sig = item.get("signature", {})
        anchor = slug(item["symbol"])
        lines += [
            "",
            f"## {code(item['symbol'])} {{#{anchor}}}",
            "",
            f"<span class=\"status-chip\">{html.escape(str(item.get('profileStatus', 'unknown')))}</span>",
            "",
            f"**Signature:** {code(sig.get('notation', 'not specified'))}",
            "",
            f"**Domains:** {md(', '.join(item.get('domains', [])) or 'none')}<br>",
            f"**Effects:** {md('; '.join(item.get('effects', [])) or 'none')}<br>",
            f"**Capabilities:** {md('; '.join(item.get('capabilities', [])) or 'none')}<br>",
            f"**Diagnostics:** {md(', '.join(item.get('diagnostics', [])) or 'none')}",
            "",
            "<details><summary>Contracts and examples</summary>",
            "",
            "**Contracts**",
            "",
            bullet_list(item.get("contracts", [])),
            "",
            "**Examples**",
            "",
        ]
        examples = item.get("examples", [])
        if examples:
            for ex in examples:
                lines += [f"- **{md(ex.get('id', 'example'))}:** {code(ex.get('source', ''))} → {code(ex.get('expected', ''))}"]
        else:
            lines.append("- None declared.")
        lines += ["", "**Sources**", ""]
        for source in item.get("sources", []):
            anchors = ", ".join(source.get("anchors", []))
            lines.append(f"- {code(source.get('path', ''))}: {md(anchors)}")
        lines += ["", "</details>"]
    return "\n".join(lines) + "\n"


def render_capabilities(doc: dict[str, Any]) -> str:
    operations = set(doc["operations"])
    contracts = doc.get("operation_contracts", {})
    lines = [
        "---",
        'title: "Capability reference"',
        'description: "Every GenesisCode host ABI operation, grouped by capability family."',
        "toc: true",
        "toc-depth: 2",
        "---",
        "",
        "# Host operation inventory",
        "",
        f"**{len(operations)} operations** across **{len(doc['families'])} families**. The runner is deny-by-default: listing an operation does not authorize it.",
        "",
        "Policy fields and payload/response contracts remain normative in [Host ABI](../docs/spec/HOST_ABI.md) and [Caps TOML](../docs/spec/CAPS_TOML.md).",
    ]
    covered: set[str] = set()
    for family, ops in sorted(doc["families"].items()):
        lines += ["", f"## {code(family)}", "", "| Operation | Operation-specific gates |", "|---|---|"]
        for op in ops:
            covered.add(op)
            gates = contracts.get(op, {}).get("policy_gates", [])
            lines.append(f"| {code(op)} | {md('; '.join(gates) or 'See family policy')} |")
            contract = contracts.get(op)
            if contract:
                lines += ["", f"<details><summary>{html.escape(op)} typed contract</summary>", "", "```json", json.dumps(contract, indent=2, sort_keys=True), "```", "", "</details>"]
    missing = sorted(operations - covered)
    if missing:
        lines += ["", "## Unclassified operations", ""] + [f"- {code(op)}" for op in missing]
    return "\n".join(lines) + "\n"


def render_diagnostics(doc: dict[str, Any]) -> str:
    groups: dict[str, list[dict[str, Any]]] = defaultdict(list)
    for item in doc["diagnostics"]:
        groups[item.get("phase", "unspecified")].append(item)
    lines = [
        "---",
        'title: "Diagnostic and repair reference"',
        'description: "Every stable diagnostic code and its policy-preserving repair actions."',
        "toc: true",
        "toc-depth: 2",
        "---",
        "",
        "# Structured failures",
        "",
        f"**{len(doc['diagnostics'])} diagnostics** · catalog identity {code(doc['catalogIdentitySha256'])}",
        "",
        "Repairs are conditional plans, not permission to bypass policy or obligations. Re-check identity and preconditions before automation.",
    ]
    for phase, items in sorted(groups.items()):
        lines += ["", f"## {code(phase)}", "", "| Code | Severity | Safe action |", "|---|---|---|"]
        for item in sorted(items, key=lambda x: x["code"]):
            actions = item.get("safeRepairActions", [])
            action = actions[0].get("id", "inspect") if actions else "inspect"
            lines.append(f"| [{code(item['code'])}](#{slug(item['code'])}) | {md(item.get('severity', ''))} | {code(action)} |")
        for item in sorted(items, key=lambda x: x["code"]):
            lines += ["", f"### {code(item['code'])} {{#{slug(item['code'])}}}", "", f"**Severity:** {md(item.get('severity', ''))} · **ID:** {code(item.get('id', ''))}", "", "**Likely causes**", "", bullet_list(item.get("likelyCauses", [])), "", "**Safe repairs**", ""]
            actions = item.get("safeRepairActions", [])
            if not actions:
                lines.append("- No automatic repair declared; inspect and abstain if uncertain.")
            for action in actions:
                review = "review required" if action.get("requiresReview") else "review not required by catalog"
                auto = "automatic eligible" if action.get("automaticEligible") else "not automatic"
                lines += [f"- **{code(action.get('id', 'repair'))}** ({auto}; {review}): {md(action.get('description', ''))}"]
            docs = item.get("documentation", [])
            if docs:
                lines += ["", "**Documentation:** " + "; ".join(code(d.get("path", "")) for d in docs)]
    return "\n".join(lines) + "\n"


def render_examples(files: list[Path]) -> str:
    groups: dict[str, list[Path]] = defaultdict(list)
    for path in files:
        groups[path.relative_to(ROOT / "examples").parts[0]].append(path)
    lines = [
        "---",
        'title: "Example catalog"',
        'description: "Every tracked GenesisCode example and agent workflow."',
        "toc: true",
        "toc-depth: 2",
        "---",
        "",
        "# Examples by workflow",
        "",
        f"**{len(groups)} workflows** · **{len(files)} files**. Examples demonstrate composition; normative behavior comes from linked specifications.",
    ]
    for group, paths in sorted(groups.items()):
        readme = next((p for p in paths if p.name.lower() == "readme.md"), None)
        description = title_of(readme) if readme else group.replace("_", " ").title()
        lines += ["", f"## {code(group)}", "", description, "", "| File | Kind | Source |", "|---|---|---|"]
        for path in paths:
            rel = path.relative_to(ROOT).as_posix()
            ext = path.suffix.lstrip(".") or "file"
            url = f"https://github.com/corbensorenson/genesisCode/blob/master/{rel}"
            lines.append(f"| {code(path.name)} | {md(ext)} | [open]({url}) |")
    return "\n".join(lines) + "\n"


def source_files() -> list[Path]:
    paths = source_materialization_files("docs")
    for name in ["AGENTS.md", "README.md", "CHANGELOG.md", "ROADMAP.md", "feature_matrix.md", "upgrade_plan.md", "genesisCode.md"]:
        path = ROOT / name
        if path.exists():
            paths.append(path)
    return sorted(paths, key=lambda p: p.relative_to(ROOT).as_posix())


def render_source_catalog(files: list[Path]) -> str:
    groups: dict[str, list[Path]] = defaultdict(list)
    for path in files:
        rel = path.relative_to(ROOT)
        if rel.parts[0] != "docs":
            group = "Repository roots"
        elif len(rel.parts) == 2:
            group = "Top-level documentation"
        else:
            group = f"docs/{rel.parts[1]}"
        groups[group].append(path)
    lines = [
        "---",
        'title: "Canonical source catalog"',
        'description: "Every documentation authority, schema, policy view, status report, and planning source."',
        "toc: true",
        "toc-depth: 2",
        "---",
        "",
        "# Documentation and authority inventory",
        "",
        f"**{len(files)} tracked sources**. SHA-256 prefixes identify the exact content used to generate this catalog.",
    ]
    for group, paths in sorted(groups.items()):
        lines += ["", f"## {group}", "", "| Source | Title / kind | Type | SHA-256 |", "|---|---|---|---|"]
        for path in paths:
            rel = path.relative_to(ROOT).as_posix()
            href = "../" + rel
            kind = "schema" if path.name.endswith(".schema.json") else path.suffix.lstrip(".")
            lines.append(f"| [{code(rel)}]({href}) | {md(title_of(path))} | {md(kind)} | {code(sha256(path)[:12])} |")
    return "\n".join(lines) + "\n"


def schema_catalog(files: list[Path]) -> str:
    schemas = [p for p in files if p.name.endswith(".schema.json")]
    lines = [f"**{len(schemas)} closed JSON schemas**"]
    for path in schemas:
        rel = path.relative_to(ROOT).as_posix()
        lines.append(f"- [{code(rel)}](../{rel})")
    return "\n".join(lines) + "\n"


def cli_commands() -> str:
    text = (ROOT / "docs/spec/CLI.md").read_text(encoding="utf-8")
    commands: set[str] = set()
    for match in re.finditer(r"`(genesis(?: [^`\n]+)?)`", text):
        command = match.group(1).strip().rstrip(".,")
        if len(command) <= 120:
            commands.add(command)
    stable = [
        "fmt", "eval", "explain", "debug/*", "run", "replay", "optimize", "typecheck",
        "test", "apply-patch", "semantic-edit", "pack", "verify", "selfhost-artifact",
        "selfhost-dashboard", "cli-schema", "agent-index", "agent-plan", "bench", "keygen", "sign",
        "transparency-verify", "store/*", "refs/*", "commit/*", "pkg/* (gcpm/* alias)",
        "policy/*", "sync/*", "gc/*", "vcs/*",
    ]
    lines = ["**Stable routed families**", "", ", ".join(code(x) for x in stable), "", "**Command forms explicitly frozen in the CLI specification**", ""]
    lines += [f"- {code(x)}" for x in sorted(commands)]
    return "\n".join(lines) + "\n"


def stats_fragments(symbols: int, caps: int, diags: int, examples: int, sources: int, schemas: int) -> tuple[str, str]:
    home = f'''::: {{.metric-strip}}
::: {{.metric}}
<strong>{symbols}</strong><span>frozen symbols</span>
:::
::: {{.metric}}
<strong>{caps}</strong><span>host operations</span>
:::
::: {{.metric}}
<strong>{diags}</strong><span>diagnostics</span>
:::
::: {{.metric}}
<strong>{sources}</strong><span>authority sources</span>
:::
:::
'''
    reference = f"This build indexes **{symbols} symbols**, **{caps} host operations**, **{diags} diagnostics**, **{examples} example files**, **{schemas} schemas**, and **{sources} canonical sources**.\n"
    return home, reference


def render_llms(symbol_doc: dict[str, Any], cap_doc: dict[str, Any], diag_doc: dict[str, Any], files: list[Path]) -> str:
    lines = [
        "# GenesisCode documentation index for language models",
        "",
        "> GenesisCode v0.2 is pre-1.0. The pure kernel is deterministic; all host work is an explicit deny-by-default effect. EFFECT, ERROR, and UNHANDLED are trusted only through seals.",
        "",
        "## Start",
        "- https://corbensorenson.github.io/genesisCode/docs/spec/GC_AGENT_CORE_CARD_v0.3.html",
        "- https://corbensorenson.github.io/genesisCode/docs/AGENT_ONBOARDING_v0.1.html",
        "- https://corbensorenson.github.io/genesisCode/learn/agent-loop.html",
        "- https://corbensorenson.github.io/genesisCode/reference/index.html",
        "",
        "## Machine authorities",
        f"- Symbol index ({len(symbol_doc['symbols'])}): https://corbensorenson.github.io/genesisCode/docs/spec/GC_AGENT_SYMBOL_INDEX_v0.3.json",
        f"- Host ABI index ({len(cap_doc['operations'])}): https://corbensorenson.github.io/genesisCode/docs/spec/HOST_ABI_INDEX_v0.1.json",
        f"- Diagnostic catalog ({len(diag_doc['diagnostics'])}): https://corbensorenson.github.io/genesisCode/docs/spec/GC_DIAGNOSTIC_CATALOG_v0.1.json",
        "- Agent profile: https://corbensorenson.github.io/genesisCode/docs/spec/GC_AGENT_PROFILE_v0.3.json",
        "- Agent corpus: https://corbensorenson.github.io/genesisCode/docs/spec/GC_AGENT_CORPUS_v0.1.json",
        "- Canonical examples: https://corbensorenson.github.io/genesisCode/examples/canonical_language/v0.1/suite.json",
        "- Agent task benchmark: https://corbensorenson.github.io/genesisCode/benchmarks/agent_tasks/v0.1/suite.json",
        "- Model-agnostic benchmark scoring: https://corbensorenson.github.io/genesisCode/docs/spec/GC_AGENT_BENCHMARK_SCORING_v0.1.json",
        "- Benchmark score schema: https://corbensorenson.github.io/genesisCode/docs/spec/GC_AGENT_BENCHMARK_SCORE_v0.1.schema.json",
        "- Reproducible benchmark run schema: https://corbensorenson.github.io/genesisCode/docs/spec/GC_AGENT_BENCHMARK_RUN_v0.1.schema.json",
        "- GenesisBench construct-validity policy: https://corbensorenson.github.io/genesisCode/policies/genesisbench_construct_validity_v0.1.json",
        "- GenesisBench construct-validity report: https://corbensorenson.github.io/genesisCode/benchmarks/genesisbench/v0.1/construct-validity/report.json",
        "- Local benchmark model effect: https://corbensorenson.github.io/genesisCode/docs/spec/GC_AGENT_MODEL_RUNNER_EFFECT_v0.1.html",
        "- Canonical reproducible run: https://corbensorenson.github.io/genesisCode/examples/agent_benchmark_reproducibility/run.json",
        "- Held-out commitments (public ledger only; never retrieve private custody): https://corbensorenson.github.io/genesisCode/docs/spec/GC_AGENT_HELD_OUT_EVALUATION_v0.1.json",
        "- Reference index: https://corbensorenson.github.io/genesisCode/reference/generated/reference-index.json",
        "",
        "## Retrieval protocol",
        "1. Load the compact core card.",
        "2. Select exactly one task card.",
        "3. Resolve exact symbols, diagnostics, and CLI options through machine indexes.",
        "4. Retrieve one domain recipe and canonical spec only as needed.",
        "5. Preserve policy and obligations; abstain when repair preconditions drift.",
        "",
        "## Canonical source roots",
    ]
    for path in files:
        if path.suffix == ".md" and (path.parent == ROOT / "docs" or path.parent == ROOT / "docs/spec"):
            rel = path.relative_to(ROOT).with_suffix(".html").as_posix()
            lines.append(f"- {title_of(path)}: https://corbensorenson.github.io/genesisCode/{rel}")
    return "\n".join(lines) + "\n"


def build_outputs() -> dict[Path, str]:
    symbol_doc = read_json(SYMBOLS_PATH)
    cap_doc = read_json(CAPS_PATH)
    diag_doc = read_json(DIAGS_PATH)
    files = source_files()
    example_files = source_materialization_files("examples")
    schema_count = sum(p.name.endswith(".schema.json") for p in files)
    home_stats, ref_stats = stats_fragments(
        len(symbol_doc["symbols"]), len(cap_doc["operations"]), len(diag_doc["diagnostics"]),
        len(example_files), len(files), schema_count,
    )
    index = {
        "kind": "genesis/quarto-reference-index-v0.1",
        "version": 1,
        "counts": {
            "symbols": len(symbol_doc["symbols"]), "hostOperations": len(cap_doc["operations"]),
            "diagnostics": len(diag_doc["diagnostics"]), "exampleFiles": len(example_files),
            "schemas": schema_count, "canonicalSources": len(files),
        },
        "authorities": {
            str(SYMBOLS_PATH.relative_to(ROOT)): sha256(SYMBOLS_PATH),
            str(CAPS_PATH.relative_to(ROOT)): sha256(CAPS_PATH),
            str(DIAGS_PATH.relative_to(ROOT)): sha256(DIAGS_PATH),
        },
        "sources": [{"path": str(p.relative_to(ROOT)), "sha256": sha256(p)} for p in files],
    }
    return {
        ROOT / "reference/symbols.qmd": render_symbols(symbol_doc),
        ROOT / "reference/capabilities.qmd": render_capabilities(cap_doc),
        ROOT / "reference/diagnostics.qmd": render_diagnostics(diag_doc),
        ROOT / "reference/examples.qmd": render_examples(example_files),
        ROOT / "reference/source-catalog.qmd": render_source_catalog(files),
        GENERATED / "home-stats.mdinc": home_stats,
        GENERATED / "reference-stats.mdinc": ref_stats,
        GENERATED / "schema-catalog.mdinc": schema_catalog(files),
        GENERATED / "cli-commands.mdinc": cli_commands(),
        GENERATED / "reference-index.json": json.dumps(index, indent=2, sort_keys=True) + "\n",
        ROOT / "llms.txt": render_llms(symbol_doc, cap_doc, diag_doc, files),
    }


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--check", action="store_true", help="fail if tracked generated references are stale")
    args = parser.parse_args()
    outputs = build_outputs()
    stale: list[str] = []
    for path, content in outputs.items():
        if args.check:
            if not path.exists() or path.read_text(encoding="utf-8") != content:
                stale.append(str(path.relative_to(ROOT)))
        else:
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_text(content, encoding="utf-8")
    if stale:
        print("quarto-reference: stale generated files: " + ", ".join(stale), file=sys.stderr)
        print("run: python3 scripts/render_quarto_reference.py", file=sys.stderr)
        return 1
    action = "fresh" if args.check else "updated"
    print(f"quarto-reference: {action} ({len(outputs)} artifacts)")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
