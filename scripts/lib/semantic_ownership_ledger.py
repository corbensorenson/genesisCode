#!/usr/bin/env python3
"""Validate exhaustive CLI-to-semantic ownership and current H-level truth."""

from __future__ import annotations

import argparse
import copy
import hashlib
import json
import re
from pathlib import Path
from typing import Any, Callable


class LedgerError(ValueError):
    pass


ROOT_FIELDS = {
    "auditDate", "canonicalSpec", "canonicalSpecSha256", "closureContract",
    "closureContractIdentitySha256", "commandBindings", "commandInventoryCount",
    "commandSourcePaths", "contentIdentitySha256", "kind", "nonclaims", "schema",
    "schemaSha256", "semanticDecisions", "version",
}
BINDING_FIELDS = {
    "expectedLeafCount", "includesHidden", "routingImplementationPaths",
    "selector", "semanticDecisionIds", "testPaths",
}
DECISION_FIELDS = {
    "applicability", "commandSelectors", "currentLevel", "fallbackReachability",
    "hostBindingPaths", "id", "internalOnly", "limitations", "migrationTasks",
    "producingImplementationPaths", "productionAuthorityPaths", "rollbackPosture",
    "specAuthorityPaths", "stage0Domains", "testPaths", "title", "verifierPaths",
}
COMMAND_SOURCES = (
    "crates/gc_cli_driver/src/cli_args.rs",
    "crates/gc_cli_driver/src/cli_args/command_groups.rs",
    "crates/gc_cli_driver/src/cli_args/sync_registry_cmd.rs",
    "crates/gc_cli_driver/src/cli_args/pkg_cmd.rs",
    "crates/gc_cli_driver/src/cli_args/policy_gc_vcs_cmd.rs",
)
TOP_LEVEL_COMMANDS = {
    "agent-index", "agent-plan", "apply-patch", "bench", "cli-schema", "commit",
    "debug", "eval", "explain", "fmt", "gc", "keygen", "mcp", "optimize",
    "pack", "parse", "pkg", "policy", "refs", "registry", "replay", "run",
    "selfhost-artifact", "selfhost-dashboard", "semantic-edit", "session", "sign",
    "store", "sync", "test", "transparency-verify", "typecheck", "vcs", "verify",
    "warm",
}
DECISION_IDS = {
    "SD-AGENT-INDEX", "SD-AGENT-PLAN", "SD-ARTIFACT-GC", "SD-BENCH",
    "SD-CANON-IDENTITY", "SD-CLI-SCHEMA", "SD-COMMIT", "SD-COMPILED-EXECUTION",
    "SD-DEBUG-TRACE", "SD-EFFECT-DISPATCH", "SD-EFFECT-POLICY",
    "SD-EVIDENCE-VERIFY", "SD-FRONTEND-CANON-IDENTITY", "SD-OBLIGATION", "SD-OPTIMIZATION", "SD-PACKAGE-ABI-DEPLOY",
    "SD-PACKAGE-DISTRIBUTION", "SD-PACKAGE-EXEC", "SD-PACKAGE-RESOLUTION",
    "SD-PACKAGE-WORKSPACE", "SD-PATCH", "SD-POLICY-ALIAS", "SD-PRINT-FORMAT",
    "SD-PURE-EVAL", "SD-REFS", "SD-REGISTRY", "SD-REMOTE-SYNC", "SD-REPLAY",
    "SD-ROUTE-SELECTION", "SD-SELFHOST-ARTIFACT", "SD-SESSION", "SD-SIGNING",
    "SD-SOURCE-DECODE", "SD-STORE", "SD-TYPE-EFFECT", "SD-VCS",
    "SD-WARM-MCP", "SD-WASM-TRANSLATION",
}
LEVELS = ("H0", "H1", "H2", "H3", "H4")
STAGE0_DOMAINS = {"S0-K", "S0-R", "S0-P", "S0-X", "S0-A", "S0-H"}
SPEC_MARKERS = (
    "every leaf reachable from the Clap `Cmd` enum",
    "hidden internal commands",
    "`SD-ROUTE-SELECTION` is deliberately separate",
    "`currentLevel: null` means the applicable decision has not proven H0",
    "Every command leaf is covered exactly once",
    "only H2 or higher may claim",
    "changes no route, implementation, fallback, or production authority",
)


def fail(message: str) -> None:
    raise LedgerError(message)


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


def sha256(value: bytes) -> str:
    return hashlib.sha256(value).hexdigest()


def canonical_identity(value: dict[str, Any]) -> str:
    payload = {key: item for key, item in value.items() if key != "contentIdentitySha256"}
    return sha256(json.dumps(payload, sort_keys=True, separators=(",", ":"), ensure_ascii=True).encode())


def require_closed(value: Any, fields: set[str], label: str) -> dict[str, Any]:
    if not isinstance(value, dict):
        fail(f"{label} must be an object")
    missing = sorted(fields - set(value))
    unknown = sorted(set(value) - fields)
    if missing or unknown:
        fail(f"{label} field drift: missing={missing}, unknown={unknown}")
    return value


def strings(value: Any, label: str, *, allow_empty: bool = False) -> list[str]:
    if not isinstance(value, list) or not all(isinstance(item, str) and item for item in value):
        fail(f"{label} must be a string array")
    if not allow_empty and not value:
        fail(f"{label} must not be empty")
    if len(value) != len(set(value)):
        fail(f"{label} contains duplicates")
    return value


def mask_rust(source: str) -> str:
    out = list(source)
    i = 0
    state = "code"
    block_depth = 0
    while i < len(source):
        if state == "code":
            if source.startswith("//", i):
                out[i] = out[i + 1] = " "
                i += 2
                state = "line"
            elif source.startswith("/*", i):
                out[i] = out[i + 1] = " "
                i += 2
                block_depth = 1
                state = "block"
            elif source[i] == '"':
                out[i] = " "
                i += 1
                state = "string"
            elif source[i] == "'" and i + 2 < len(source) and source[i + 2] == "'":
                out[i] = " "
                i += 1
                state = "char"
            else:
                i += 1
        elif state == "line":
            if source[i] == "\n":
                state = "code"
            else:
                out[i] = " "
            i += 1
        elif state == "block":
            if source.startswith("/*", i):
                out[i] = out[i + 1] = " "
                block_depth += 1
                i += 2
            elif source.startswith("*/", i):
                out[i] = out[i + 1] = " "
                block_depth -= 1
                i += 2
                if block_depth == 0:
                    state = "code"
            else:
                out[i] = " "
                i += 1
        else:
            if source[i] == "\\":
                out[i] = " "
                i += 1
                if i < len(source):
                    out[i] = " "
                    i += 1
            elif (state == "string" and source[i] == '"') or (state == "char" and source[i] == "'"):
                out[i] = " "
                i += 1
                state = "code"
            else:
                out[i] = " "
                i += 1
    return "".join(out)


def matching_delimiter(masked: str, start: int, opening: str, closing: str) -> int:
    depth = 0
    for index in range(start, len(masked)):
        if masked[index] == opening:
            depth += 1
        elif masked[index] == closing:
            depth -= 1
            if depth == 0:
                return index
    fail(f"unclosed Rust delimiter at byte {start}")


def parse_cli_commands(root: Path) -> dict[str, bool]:
    source = "\n".join((root / path).read_text(encoding="utf-8") for path in COMMAND_SOURCES)
    masked = mask_rust(source)
    enums: dict[str, list[tuple[str, str | None, bool]]] = {}
    for match in re.finditer(r"\benum\s+(\w+)\s*\{", masked):
        enum_name = match.group(1)
        block_start = match.end() - 1
        block_end = matching_delimiter(masked, block_start, "{", "}")
        block = source[block_start + 1:block_end]
        block_mask = masked[block_start + 1:block_end]
        tokens: list[tuple[str, str]] = []
        token_start = 0
        depth = 0
        for index, char in enumerate(block_mask):
            if char in "({[":
                depth += 1
            elif char in ")} ]".replace(" ", ""):
                depth -= 1
            elif char == "," and depth == 0:
                tokens.append((block[token_start:index], block_mask[token_start:index]))
                token_start = index + 1
        if block[token_start:].strip():
            tokens.append((block[token_start:], block_mask[token_start:]))

        variants: list[tuple[str, str | None, bool]] = []
        for original, token_mask in tokens:
            position = 0
            while True:
                while position < len(token_mask) and token_mask[position].isspace():
                    position += 1
                if not token_mask.startswith("#", position):
                    break
                bracket = token_mask.find("[", position)
                if bracket < 0:
                    break
                position = matching_delimiter(token_mask, bracket, "[", "]") + 1
            variant = re.match(r"(\w+)\b", token_mask[position:])
            if not variant:
                continue
            rust_name = variant.group(1)
            attributes = original[:position]
            explicit = re.search(r"command\s*\([^\]]*?name\s*=\s*\"([^\"]+)\"", attributes, re.S)
            cli_name = explicit.group(1) if explicit else re.sub(r"(?<!^)(?=[A-Z])", "-", rust_name).lower()
            hidden = bool(re.search(r"command\s*\([^\]]*?hide\s*=\s*true", attributes, re.S))
            nested = re.search(
                r"#\s*\[\s*command\s*\(\s*subcommand\s*\)\s*\]\s*\w+\s*:\s*(\w+)",
                original,
                re.S,
            )
            variants.append((cli_name, nested.group(1) if nested else None, hidden))
        enums[enum_name] = variants

    if "Cmd" not in enums:
        fail("root Cmd enum missing")
    commands: dict[str, bool] = {}

    def walk(enum_name: str, prefix: tuple[str, ...] = ()) -> None:
        if enum_name not in enums:
            fail(f"nested command enum missing: {enum_name}")
        for cli_name, nested, hidden in enums[enum_name]:
            path = prefix + (cli_name,)
            if nested:
                before = set(commands)
                walk(nested, path)
                for child in set(commands) - before:
                    commands[child] = commands[child] or hidden
            else:
                rendered = " ".join(path)
                if rendered in commands:
                    fail(f"duplicate command leaf: {rendered}")
                commands[rendered] = hidden

    walk("Cmd")
    return commands


def expand_selector(selector: str, commands: dict[str, bool]) -> list[str]:
    if selector.endswith("/*"):
        prefix = selector[:-2] + " "
        return sorted(path for path in commands if path.startswith(prefix))
    return [selector] if selector in commands else []


def validate_schema(schema: Any) -> None:
    if not isinstance(schema, dict):
        fail("schema root must be an object")
    if schema.get("$id") != "https://genesiscode.dev/schemas/semantic-ownership-ledger-v0.1.json":
        fail("schema ID drift")
    if schema.get("additionalProperties") is not False:
        fail("schema root must be closed")
    if set(schema.get("required", [])) != ROOT_FIELDS or set(schema.get("properties", {})) != ROOT_FIELDS:
        fail("schema root field inventory drift")
    defs = schema.get("$defs", {})
    for name, fields in (("commandBinding", BINDING_FIELDS), ("semanticDecision", DECISION_FIELDS)):
        item = defs.get(name)
        if not isinstance(item, dict) or item.get("additionalProperties") is not False:
            fail(f"schema {name} must be closed")
        if set(item.get("required", [])) != fields:
            fail(f"schema {name} required fields drift")


def validate(
    root: Path,
    ledger: Any,
    schema: Any,
    spec_bytes: bytes,
    schema_bytes: bytes,
    closure: Any,
) -> None:
    value = require_closed(ledger, ROOT_FIELDS, "ledger")
    validate_schema(schema)
    expected = {
        "kind": "genesis/semantic-ownership-ledger-v0.1",
        "version": "0.1",
        "canonicalSpec": "docs/spec/SEMANTIC_OWNERSHIP_LEDGER_v0.1.md",
        "closureContract": "docs/spec/SELFHOST_CLOSURE_LEVELS_v0.1.json",
        "schema": "docs/spec/SEMANTIC_OWNERSHIP_LEDGER_v0.1.schema.json",
    }
    for field, expected_value in expected.items():
        if value.get(field) != expected_value:
            fail(f"{field} drift")
    if not re.fullmatch(r"\d{4}-\d{2}-\d{2}", str(value.get("auditDate", ""))):
        fail("auditDate must be ISO date")
    if value.get("canonicalSpecSha256") != sha256(spec_bytes):
        fail("canonical prose identity mismatch")
    if value.get("schemaSha256") != sha256(schema_bytes):
        fail("schema identity mismatch")
    if value.get("contentIdentitySha256") != canonical_identity(value):
        fail("content identity mismatch")
    if not isinstance(closure, dict) or value.get("closureContractIdentitySha256") != closure.get("contentIdentitySha256"):
        fail("closure contract identity mismatch")
    if tuple(value.get("commandSourcePaths", [])) != COMMAND_SOURCES:
        fail("command source closure drift")

    commands = parse_cli_commands(root)
    if value.get("commandInventoryCount") != len(commands):
        fail("command inventory count drift")
    top = {path.split()[0] for path in commands}
    if top != TOP_LEVEL_COMMANDS:
        fail("top-level command inventory drift")

    bindings = value.get("commandBindings")
    if not isinstance(bindings, list) or not bindings:
        fail("commandBindings must be non-empty")
    selectors: list[str] = []
    coverage: dict[str, str] = {}
    decision_references: dict[str, set[str]] = {}
    for item in bindings:
        binding = require_closed(item, BINDING_FIELDS, "command binding")
        selector = binding.get("selector")
        if not isinstance(selector, str) or not selector:
            fail("command selector must be a string")
        selectors.append(selector)
        matches = expand_selector(selector, commands)
        if not matches or binding.get("expectedLeafCount") != len(matches):
            fail(f"selector count drift: {selector}")
        if binding.get("includesHidden") is not any(commands[path] for path in matches):
            fail(f"hidden-command posture drift: {selector}")
        refs = strings(binding.get("semanticDecisionIds"), f"{selector} decision refs")
        if refs[0] != "SD-ROUTE-SELECTION" or len(refs) < 2:
            fail(f"{selector} must separate route selection from functional decisions")
        for decision_id in refs:
            decision_references.setdefault(decision_id, set()).add(selector)
        for path in matches:
            if path in coverage:
                fail(f"command covered more than once: {path}")
            coverage[path] = selector
        for field in ("routingImplementationPaths", "testPaths"):
            for path in strings(binding.get(field), f"{selector} {field}"):
                if not (root / path).exists():
                    fail(f"{selector} missing path: {path}")
    if len(selectors) != len(set(selectors)):
        fail("duplicate command selector")
    if set(coverage) != set(commands):
        fail("command leaf coverage is not exact")
    if {selector.removesuffix("/*") for selector in selectors} != TOP_LEVEL_COMMANDS:
        fail("command binding top-level inventory drift")

    rows = value.get("semanticDecisions")
    if not isinstance(rows, list) or not rows:
        fail("semanticDecisions must be non-empty")
    by_id: dict[str, dict[str, Any]] = {}
    path_fields = (
        "specAuthorityPaths", "producingImplementationPaths", "productionAuthorityPaths",
        "hostBindingPaths", "verifierPaths", "testPaths",
    )
    for item in rows:
        row = require_closed(item, DECISION_FIELDS, "semantic decision")
        decision_id = row.get("id")
        if not isinstance(decision_id, str) or decision_id in by_id:
            fail(f"invalid or duplicate decision ID: {decision_id}")
        by_id[decision_id] = row
        domains = set(strings(row.get("stage0Domains"), f"{decision_id} stage0Domains"))
        if not domains.issubset(STAGE0_DOMAINS):
            fail(f"{decision_id} has unknown stage0 domain")
        applicability = row.get("applicability")
        level = row.get("currentLevel")
        fallback = row.get("fallbackReachability")
        if applicability == "applicable":
            if level is not None and level not in LEVELS:
                fail(f"{decision_id} has invalid H-level")
            if fallback == "declared-stage0-residual":
                fail(f"{decision_id} applicable decision cannot use residual fallback")
        elif applicability == "residual-stage0":
            if level is not None or not domains.issubset({"S0-K", "S0-H"}) or fallback != "declared-stage0-residual":
                fail(f"{decision_id} residual-stage0 disposition drift")
        else:
            fail(f"{decision_id} unsupported applicability")
        if fallback == "none-proven" and (level not in LEVELS or LEVELS.index(level) < 2):
            fail(f"{decision_id} claims no fallback below H2")
        for field in path_fields:
            for path in strings(row.get(field), f"{decision_id} {field}", allow_empty=field == "hostBindingPaths"):
                if path.startswith("/") or ".." in Path(path).parts or not (root / path).exists():
                    fail(f"{decision_id} missing or noncanonical path: {path}")
        if level == "H0" and not any(path.endswith(".gc") or path == "selfhost/toolchain.gc" for path in row["producingImplementationPaths"]):
            fail(f"{decision_id} H0 lacks exact GenesisCode producer route")
        if level in ("H2", "H3", "H4") and set(row["productionAuthorityPaths"]) & set(row["verifierPaths"]):
            fail(f"{decision_id} H2+ producer verifies itself")
        row_selectors = set(strings(row.get("commandSelectors"), f"{decision_id} commandSelectors", allow_empty=True))
        internal = row.get("internalOnly")
        if internal is True and row_selectors:
            fail(f"{decision_id} internal-only row names commands")
        if internal is not True and not row_selectors:
            fail(f"{decision_id} non-internal row lacks commands")
        if row_selectors != decision_references.get(decision_id, set()):
            fail(f"{decision_id} command backlink drift")
        strings(row.get("migrationTasks"), f"{decision_id} migrationTasks")
        strings(row.get("limitations"), f"{decision_id} limitations")
        if not isinstance(row.get("rollbackPosture"), str) or not row["rollbackPosture"]:
            fail(f"{decision_id} rollback posture missing")
    if set(by_id) != DECISION_IDS:
        fail("semantic decision inventory drift")
    unknown_refs = set(decision_references) - set(by_id)
    if unknown_refs:
        fail(f"unknown command decision references: {sorted(unknown_refs)}")
    if by_id["SD-ROUTE-SELECTION"]["currentLevel"] != "H0":
        fail("route selection must remain a distinct H0 row")

    strings(value.get("nonclaims"), "nonclaims")
    spec_text = spec_bytes.decode("utf-8")
    missing = [marker for marker in SPEC_MARKERS if marker not in spec_text]
    if missing:
        fail(f"normative prose markers missing: {missing}")


def reseal(ledger: dict[str, Any], spec_bytes: bytes, schema_bytes: bytes) -> None:
    ledger["canonicalSpecSha256"] = sha256(spec_bytes)
    ledger["schemaSha256"] = sha256(schema_bytes)
    ledger["contentIdentitySha256"] = canonical_identity(ledger)


def run_self_tests(root: Path, ledger: dict[str, Any], schema: dict[str, Any], spec: bytes, schema_bytes: bytes, closure: dict[str, Any]) -> int:
    mutations: list[tuple[str, Callable[[dict[str, Any], dict[str, Any], bytearray, dict[str, Any]], None]]] = [
        ("unknown-root-field", lambda d, s, p, c: d.__setitem__("unknown", True)),
        ("command-count-drift", lambda d, s, p, c: d.__setitem__("commandInventoryCount", 1)),
        ("command-source-loss", lambda d, s, p, c: d["commandSourcePaths"].pop()),
        ("selector-count-drift", lambda d, s, p, c: d["commandBindings"][0].__setitem__("expectedLeafCount", 99)),
        ("selector-hidden-drift", lambda d, s, p, c: next(x for x in d["commandBindings"] if x["selector"] == "bench/*").__setitem__("includesHidden", False)),
        ("selector-overlap", lambda d, s, p, c: d["commandBindings"].append(copy.deepcopy(d["commandBindings"][0]))),
        ("command-functional-decision-loss", lambda d, s, p, c: d["commandBindings"][0].__setitem__("semanticDecisionIds", ["SD-ROUTE-SELECTION"])),
        ("route-decision-order-loss", lambda d, s, p, c: d["commandBindings"][0]["semanticDecisionIds"].reverse()),
        ("unknown-decision-reference", lambda d, s, p, c: d["commandBindings"][0]["semanticDecisionIds"].append("SD-UNKNOWN")),
        ("routing-path-missing", lambda d, s, p, c: d["commandBindings"][0]["routingImplementationPaths"].append("missing.rs")),
        ("decision-removal", lambda d, s, p, c: d["semanticDecisions"].pop()),
        ("decision-duplicate", lambda d, s, p, c: d["semanticDecisions"].append(copy.deepcopy(d["semanticDecisions"][0]))),
        ("decision-backlink-drift", lambda d, s, p, c: d["semanticDecisions"][0]["commandSelectors"].pop()),
        ("decision-path-missing", lambda d, s, p, c: d["semanticDecisions"][0]["testPaths"].append("missing")),
        ("residual-domain-broadening", lambda d, s, p, c: next(x for x in d["semanticDecisions"] if x["applicability"] == "residual-stage0")["stage0Domains"].append("S0-R")),
        ("residual-level-promotion", lambda d, s, p, c: next(x for x in d["semanticDecisions"] if x["applicability"] == "residual-stage0").__setitem__("currentLevel", "H0")),
        ("H0-without-GC-producer", lambda d, s, p, c: next(x for x in d["semanticDecisions"] if x["currentLevel"] == "H0").__setitem__("producingImplementationPaths", ["crates/gc_cli_driver/src/lib.rs"])),
        ("no-fallback-below-H2", lambda d, s, p, c: d["semanticDecisions"][0].__setitem__("fallbackReachability", "none-proven")),
        ("internal-row-has-command", lambda d, s, p, c: next(x for x in d["semanticDecisions"] if x["internalOnly"]).__setitem__("commandSelectors", ["eval"])),
        ("noninternal-row-no-command", lambda d, s, p, c: next(x for x in d["semanticDecisions"] if not x["internalOnly"]).__setitem__("commandSelectors", [])),
        ("route-level-loss", lambda d, s, p, c: next(x for x in d["semanticDecisions"] if x["id"] == "SD-ROUTE-SELECTION").__setitem__("currentLevel", None)),
        ("closure-binding-drift", lambda d, s, p, c: c.__setitem__("contentIdentitySha256", "f" * 64)),
        ("schema-open-root", lambda d, s, p, c: s.__setitem__("additionalProperties", True)),
        ("prose-marker-loss", lambda d, s, p, c: p.__setitem__(slice(None), bytes(p).replace(b"hidden internal commands", b"excluded internal commands"))),
    ]
    for name, mutate in mutations:
        candidate = copy.deepcopy(ledger)
        candidate_schema = copy.deepcopy(schema)
        candidate_spec = bytearray(spec)
        candidate_closure = copy.deepcopy(closure)
        mutate(candidate, candidate_schema, candidate_spec, candidate_closure)
        candidate_schema_bytes = json.dumps(candidate_schema, sort_keys=True, separators=(",", ":")).encode()
        reseal(candidate, bytes(candidate_spec), candidate_schema_bytes)
        try:
            validate(root, candidate, candidate_schema, bytes(candidate_spec), candidate_schema_bytes, candidate_closure)
        except LedgerError:
            continue
        fail(f"negative control accepted: {name}")
    try:
        json.loads('{"a":1,"a":2}', object_pairs_hook=no_duplicate_pairs)
    except LedgerError:
        pass
    else:
        fail("duplicate-key negative control accepted")
    return len(mutations) + 1


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--root", type=Path, default=Path(__file__).resolve().parents[2])
    parser.add_argument("--ledger", default="docs/spec/SEMANTIC_OWNERSHIP_LEDGER_v0.1.json")
    parser.add_argument("--schema", default="docs/spec/SEMANTIC_OWNERSHIP_LEDGER_v0.1.schema.json")
    parser.add_argument("--spec", default="docs/spec/SEMANTIC_OWNERSHIP_LEDGER_v0.1.md")
    parser.add_argument("--closure", default="docs/spec/SELFHOST_CLOSURE_LEVELS_v0.1.json")
    parser.add_argument("--self-test", action="store_true")
    parser.add_argument("--refresh-identities", action="store_true")
    args = parser.parse_args()
    root = args.root.resolve()
    ledger_path, schema_path, spec_path, closure_path = (
        root / args.ledger, root / args.schema, root / args.spec, root / args.closure
    )
    ledger, schema, closure = load_json(ledger_path), load_json(schema_path), load_json(closure_path)
    spec_bytes, schema_bytes = spec_path.read_bytes(), schema_path.read_bytes()
    if args.refresh_identities:
        ledger["closureContractIdentitySha256"] = closure["contentIdentitySha256"]
        reseal(ledger, spec_bytes, schema_bytes)
        validate(root, ledger, schema, spec_bytes, schema_bytes, closure)
        ledger_path.write_text(
            json.dumps(ledger, indent=2, ensure_ascii=True) + "\n",
            encoding="utf-8",
        )
        print(f"semantic-ownership-ledger: refreshed {ledger_path.relative_to(root)}")
        return 0
    validate(root, ledger, schema, spec_bytes, schema_bytes, closure)
    controls = run_self_tests(root, ledger, schema, spec_bytes, schema_bytes, closure) if args.self_test else 0
    commands = parse_cli_commands(root)
    null_levels = sum(1 for row in ledger["semanticDecisions"] if row["applicability"] == "applicable" and row["currentLevel"] is None)
    print(
        "semantic-ownership-ledger: ok "
        f"identity={ledger['contentIdentitySha256']} commands={len(commands)} "
        f"hidden={sum(commands.values())} decisions={len(ledger['semanticDecisions'])} "
        f"below_H0={null_levels} controls={controls}"
    )
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except LedgerError as error:
        print(f"semantic-ownership-ledger: failed: {error}")
        raise SystemExit(1)
