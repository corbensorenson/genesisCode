use super::*;
use std::collections::{BTreeMap, BTreeSet};

const HOST_ABI_INDEX_PATH: &str = "docs/spec/HOST_ABI_INDEX_v0.1.json";
const HOST_ABI_SCHEMA_INDEX_PATH: &str = "docs/spec/HOST_ABI_SCHEMA_INDEX_v0.1.json";
const PRELUDE_CAP_INDEX_PATH: &str = "docs/spec/PRELUDE_CAPABILITY_INDEX_v0.1.json";
const AGENT_SYMBOL_INDEX_PATH: &str = "docs/spec/GC_AGENT_SYMBOL_INDEX_v0.3.json";
const AGENT_SYMBOL_INDEX_JSON: &str =
    include_str!("../../../docs/spec/GC_AGENT_SYMBOL_INDEX_v0.3.json");
const SELFHOST_TOOLCHAIN_MANIFEST_PATH: &str = "selfhost/toolchain_manifest.gc";
const SELFHOST_SYMBOL_OWNERSHIP_SCHEMA: &str = "genesis/selfhost-symbol-ownership-index-v0.1";

#[derive(Debug, Clone)]
struct SelfhostSymbolOwner {
    module_path: String,
    module_intent: Option<String>,
}

fn resolve_repo_root(start: &Path) -> PathBuf {
    for candidate in start.ancestors() {
        let has_indices = candidate.join(HOST_ABI_INDEX_PATH).is_file()
            && candidate.join(HOST_ABI_SCHEMA_INDEX_PATH).is_file()
            && candidate.join(PRELUDE_CAP_INDEX_PATH).is_file();
        let has_examples = candidate.join("examples").is_dir();
        if has_indices && has_examples {
            return candidate.to_path_buf();
        }
    }
    start.to_path_buf()
}

fn read_json_file(path: &Path) -> Option<serde_json::Value> {
    let src = std::fs::read_to_string(path).ok()?;
    serde_json::from_str::<serde_json::Value>(&src).ok()
}

fn collect_reference_workflows(examples_dir: &Path) -> Vec<serde_json::Value> {
    let mut names = Vec::new();
    if let Ok(rd) = std::fs::read_dir(examples_dir) {
        for entry in rd.flatten() {
            if let Ok(ft) = entry.file_type()
                && ft.is_dir()
            {
                let name = entry.file_name().to_string_lossy().to_string();
                if name.starts_with("agent_") {
                    names.push(name);
                }
            }
        }
    }
    names.sort();
    names
        .into_iter()
        .map(|name| {
            let base = examples_dir.join(&name);
            let path = base.to_string_lossy().replace('\\', "/");
            serde_json::json!({
                "name": name,
                "path": path,
                "has_package_toml": base.join("package.toml").is_file(),
                "has_workflow_script": base.join("workflow.sh").is_file(),
            })
        })
        .collect()
}

fn parse_def(form: &Term) -> Option<(String, Term)> {
    let items = form.as_proper_list()?;
    if items.len() != 3 {
        return None;
    }
    if !matches!(items[0], Term::Symbol(s) if s == "def") {
        return None;
    }
    let Term::Symbol(name) = items[1] else {
        return None;
    };
    Some((name.clone(), items[2].clone()))
}

fn quoted_map(expr: &Term) -> Option<&BTreeMap<TermOrdKey, Term>> {
    if let Term::Map(m) = expr {
        return Some(m);
    }
    let items = expr.as_proper_list()?;
    if items.len() == 2
        && matches!(items[0], Term::Symbol(s) if s == "quote")
        && let Term::Map(m) = items[1]
    {
        return Some(m);
    }
    None
}

fn map_str_or_symbol(map: &BTreeMap<TermOrdKey, Term>, key: &str) -> Option<String> {
    match map.get(&TermOrdKey(Term::symbol(key))) {
        Some(Term::Str(s)) => Some(s.clone()),
        Some(Term::Symbol(s)) => Some(s.clone()),
        _ => None,
    }
}

fn extract_module_intent(forms: &[Term]) -> Option<String> {
    for form in forms {
        let Some((name, expr)) = parse_def(form) else {
            continue;
        };
        if name != "::meta" {
            continue;
        }
        let Some(meta_map) = quoted_map(&expr) else {
            continue;
        };
        return map_str_or_symbol(meta_map, ":intent");
    }
    None
}

fn manifest_map(manifest: &Term) -> Result<&BTreeMap<TermOrdKey, Term>, String> {
    match manifest {
        Term::Map(m) => Ok(m),
        _ => Err("selfhost manifest must be a map".to_string()),
    }
}

fn manifest_vec_strings(
    manifest: &BTreeMap<TermOrdKey, Term>,
    key: &str,
) -> Result<Vec<String>, String> {
    let Some(Term::Vector(xs)) = manifest.get(&TermOrdKey(Term::symbol(key))) else {
        return Err(format!("selfhost manifest missing vector key `{key}`"));
    };
    let mut out = Vec::with_capacity(xs.len());
    for x in xs {
        match x {
            Term::Str(s) => out.push(s.clone()),
            Term::Symbol(s) => out.push(s.clone()),
            _ => {
                return Err(format!(
                    "selfhost manifest key `{key}` must contain only strings/symbols"
                ));
            }
        }
    }
    Ok(out)
}

fn read_selfhost_symbol_ownership_index(repo_root: &Path) -> (serde_json::Value, Vec<String>) {
    let mut missing_sources = Vec::new();
    let manifest_fs_path = repo_root.join(SELFHOST_TOOLCHAIN_MANIFEST_PATH);

    let mk_error = |message: String, missing_sources: Vec<String>| {
        (
            serde_json::json!({
                "schema": SELFHOST_SYMBOL_OWNERSHIP_SCHEMA,
                "path": SELFHOST_TOOLCHAIN_MANIFEST_PATH,
                "loaded": false,
                "error": message,
                "module_count": 0,
                "symbol_count": 0,
                "required_symbol_count": 0,
                "unresolved_required_symbols": [],
                "duplicate_symbol_owners": [],
                "symbols": [],
            }),
            missing_sources,
        )
    };

    let manifest_src = match std::fs::read_to_string(&manifest_fs_path) {
        Ok(src) => src,
        Err(e) => {
            missing_sources.push(SELFHOST_TOOLCHAIN_MANIFEST_PATH.to_string());
            return mk_error(
                format!("failed to read selfhost manifest: {e}"),
                missing_sources,
            );
        }
    };
    let manifest = match parse_term(&manifest_src) {
        Ok(v) => v,
        Err(e) => {
            return mk_error(
                format!("failed to parse selfhost manifest: {e}"),
                missing_sources,
            );
        }
    };
    let manifest_map = match manifest_map(&manifest) {
        Ok(m) => m,
        Err(e) => return mk_error(e, missing_sources),
    };
    let module_paths = match manifest_vec_strings(manifest_map, ":module-paths") {
        Ok(v) => v,
        Err(e) => return mk_error(e, missing_sources),
    };
    let required_symbols = match manifest_vec_strings(manifest_map, ":required-symbols") {
        Ok(v) => v,
        Err(e) => return mk_error(e, missing_sources),
    };

    let mut owners: BTreeMap<String, SelfhostSymbolOwner> = BTreeMap::new();
    let mut duplicate_symbol_owners: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for module_path in &module_paths {
        let module_fs_path = repo_root.join(module_path);
        let module_src = match std::fs::read_to_string(&module_fs_path) {
            Ok(src) => src,
            Err(e) => {
                missing_sources.push(module_path.clone());
                return mk_error(
                    format!("failed to read selfhost module `{module_path}`: {e}"),
                    missing_sources,
                );
            }
        };
        let forms = match parse_module(&module_src) {
            Ok(forms) => forms,
            Err(e) => {
                return mk_error(
                    format!("failed to parse selfhost module `{module_path}`: {e}"),
                    missing_sources,
                );
            }
        };
        let module_intent = extract_module_intent(&forms);
        for form in forms {
            let Some((name, _expr)) = parse_def(&form) else {
                continue;
            };
            if name == "::meta" {
                continue;
            }
            match owners.get(&name) {
                Some(prev) if prev.module_path != *module_path => {
                    let mut entries = vec![prev.module_path.clone(), module_path.clone()];
                    entries.sort();
                    entries.dedup();
                    duplicate_symbol_owners.insert(name.clone(), entries);
                }
                Some(_) => {}
                None => {
                    owners.insert(
                        name,
                        SelfhostSymbolOwner {
                            module_path: module_path.clone(),
                            module_intent: module_intent.clone(),
                        },
                    );
                }
            }
        }
    }

    let required_set: BTreeSet<String> = required_symbols.into_iter().collect();
    let unresolved_required_symbols: Vec<String> = required_set
        .iter()
        .filter(|sym| !owners.contains_key(*sym))
        .cloned()
        .collect();

    let symbols: Vec<serde_json::Value> = owners
        .iter()
        .map(|(symbol, owner)| {
            serde_json::json!({
                "symbol": symbol,
                "module_path": owner.module_path,
                "module_intent": owner.module_intent,
                "required": required_set.contains(symbol),
            })
        })
        .collect();

    let duplicate_entries: Vec<serde_json::Value> = duplicate_symbol_owners
        .into_iter()
        .map(|(symbol, modules)| {
            serde_json::json!({
                "symbol": symbol,
                "module_paths": modules,
            })
        })
        .collect();

    (
        serde_json::json!({
            "schema": SELFHOST_SYMBOL_OWNERSHIP_SCHEMA,
            "path": SELFHOST_TOOLCHAIN_MANIFEST_PATH,
            "loaded": true,
            "module_count": module_paths.len(),
            "symbol_count": symbols.len(),
            "required_symbol_count": required_set.len(),
            "unresolved_required_symbols": unresolved_required_symbols,
            "duplicate_symbol_owners": duplicate_entries,
            "symbols": symbols,
        }),
        missing_sources,
    )
}

pub(super) fn embedded_agent_symbol_index() -> Result<serde_json::Value, CliError> {
    let index: serde_json::Value =
        serde_json::from_str(AGENT_SYMBOL_INDEX_JSON).map_err(|error| {
            cli_err(
                EX_INTERNAL,
                "agent-index/symbol-index-invalid",
                format!("embedded GC-AGENT-v0.3 symbol index is invalid: {error}"),
            )
        })?;
    let symbols = index
        .get("symbols")
        .and_then(serde_json::Value::as_array)
        .ok_or_else(|| {
            cli_err(
                EX_INTERNAL,
                "agent-index/symbol-index-invalid",
                "embedded GC-AGENT-v0.3 symbol index has no symbols array",
            )
        })?;
    let declared = index.get("symbolCount").and_then(serde_json::Value::as_u64);
    if declared != u64::try_from(symbols.len()).ok() {
        return Err(cli_err(
            EX_INTERNAL,
            "agent-index/symbol-index-invalid",
            "embedded GC-AGENT-v0.3 symbol count does not match its entries",
        ));
    }
    Ok(index)
}

fn cmd_agent_symbol(cli: &Cli, symbol: &str) -> Result<CmdOut, CliError> {
    if symbol.is_empty() || symbol.trim() != symbol {
        return Err(cli_err(
            EX_PARSE,
            "agent-index/symbol-invalid",
            "--symbol must be a nonempty, unpadded exact GC-AGENT-v0.3 name",
        ));
    }
    let index = embedded_agent_symbol_index()?;
    let symbols = index
        .get("symbols")
        .and_then(serde_json::Value::as_array)
        .ok_or_else(|| {
            cli_err(
                EX_INTERNAL,
                "agent-index/symbol-index-invalid",
                "embedded GC-AGENT-v0.3 symbol index has no symbols array",
            )
        })?;
    let matches: Vec<&serde_json::Value> = symbols
        .iter()
        .filter(|entry| entry.get("symbol").and_then(serde_json::Value::as_str) == Some(symbol))
        .collect();
    if matches.len() != 1 {
        return Err(cli_err(
            EX_PARSE,
            "agent-index/symbol-not-found",
            format!("symbol `{symbol}` is not an exact GC-AGENT-v0.3 name"),
        ));
    }
    let env = JsonEnvelope {
        ok: true,
        kind: "genesis/agent-symbol-v0.3",
        data: Some(serde_json::json!({
            "schema": "genesis/gc-agent-symbol-index-v0.3",
            "profile_id": index["profileId"],
            "profile_identity_sha256": index["profileIdentitySha256"],
            "index_identity_sha256": index["indexIdentitySha256"],
            "lookup": index["lookup"],
            "symbol": matches[0],
        })),
        error: None,
    };
    let json = json_envelope_value(env)?;
    Ok(CmdOut {
        exit_code: EX_OK,
        stdout: if cli.json {
            String::new()
        } else {
            format!("{}\n", json_canonical_string(&json))
        },
        json,
    })
}

fn embedded_agent_diagnostic_catalog() -> Result<&'static serde_json::Value, CliError> {
    diagnostics::embedded_diagnostic_catalog().map_err(|message| {
        cli_err(
            EX_INTERNAL,
            "agent-index/diagnostic-catalog-invalid",
            message,
        )
    })
}

fn cmd_agent_diagnostic(cli: &Cli, code: &str) -> Result<CmdOut, CliError> {
    if code.is_empty() || code.trim() != code {
        return Err(cli_err(
            EX_PARSE,
            "agent-index/diagnostic-invalid",
            "--diagnostic must be a nonempty, unpadded exact diagnostic code",
        ));
    }
    let catalog = embedded_agent_diagnostic_catalog()?;
    let diagnostics = catalog
        .get("diagnostics")
        .and_then(serde_json::Value::as_array)
        .ok_or_else(|| {
            cli_err(
                EX_INTERNAL,
                "agent-index/diagnostic-catalog-invalid",
                "embedded diagnostic catalog has no diagnostics array",
            )
        })?;
    let matches: Vec<&serde_json::Value> = diagnostics
        .iter()
        .filter(|entry| entry.get("code").and_then(serde_json::Value::as_str) == Some(code))
        .collect();
    if matches.len() != 1 {
        return Err(cli_err(
            EX_PARSE,
            "agent-index/diagnostic-not-found",
            format!("diagnostic `{code}` is not an exact catalog code"),
        ));
    }
    let env = JsonEnvelope {
        ok: true,
        kind: "genesis/diagnostic-v0.1",
        data: Some(serde_json::json!({
            "schema": catalog["kind"],
            "catalog_version": catalog["version"],
            "catalog_identity_sha256": catalog["catalogIdentitySha256"],
            "lookup": catalog["lookup"],
            "diagnostic": matches[0],
        })),
        error: None,
    };
    let json = json_envelope_value(env)?;
    Ok(CmdOut {
        exit_code: EX_OK,
        stdout: if cli.json {
            String::new()
        } else {
            format!("{}\n", json_canonical_string(&json))
        },
        json,
    })
}

pub(super) fn cmd_agent_index(
    cli: &Cli,
    symbol: Option<&str>,
    diagnostic: Option<&str>,
    search_symbol: Option<&str>,
    card: Option<AgentCardArg>,
    max_results: u64,
) -> Result<CmdOut, CliError> {
    if let Some(symbol) = symbol {
        return cmd_agent_symbol(cli, symbol);
    }
    if let Some(code) = diagnostic {
        return cmd_agent_diagnostic(cli, code);
    }
    if let Some(query) = search_symbol {
        return cmd_agent_lookup::cmd_agent_symbol_search(cli, query, max_results);
    }
    if let Some(card) = card {
        return cmd_agent_lookup::cmd_agent_card(cli, card);
    }
    let profile = runtime_profile();
    let cli_schema = cli_schema::build_cli_schema(profile);
    let agent_symbol_index = embedded_agent_symbol_index()?;
    let diagnostic_catalog = embedded_agent_diagnostic_catalog()?;
    let cwd = std::env::current_dir().map_err(|e| cli_err(EX_IO, "io/cwd", format!("{e}")))?;
    let repo_root = resolve_repo_root(&cwd);

    let host_abi_path = repo_root.join(HOST_ABI_INDEX_PATH);
    let host_abi_schema_path = repo_root.join(HOST_ABI_SCHEMA_INDEX_PATH);
    let prelude_cap_path = repo_root.join(PRELUDE_CAP_INDEX_PATH);
    let host_abi_index = read_json_file(&host_abi_path);
    let host_abi_schema_index = read_json_file(&host_abi_schema_path);
    let prelude_cap_index = read_json_file(&prelude_cap_path);
    let (selfhost_symbol_index, selfhost_missing_sources) =
        read_selfhost_symbol_ownership_index(&repo_root);
    let workflows = collect_reference_workflows(&repo_root.join("examples"));

    let mut missing_sources: Vec<String> = Vec::new();
    if host_abi_index.is_none() {
        missing_sources.push(HOST_ABI_INDEX_PATH.to_string());
    }
    if host_abi_schema_index.is_none() {
        missing_sources.push(HOST_ABI_SCHEMA_INDEX_PATH.to_string());
    }
    if prelude_cap_index.is_none() {
        missing_sources.push(PRELUDE_CAP_INDEX_PATH.to_string());
    }
    missing_sources.extend(selfhost_missing_sources);

    let env = JsonEnvelope {
        ok: true,
        kind: "genesis/agent-index-v0.1",
        data: Some(serde_json::json!({
            "schema": "genesis/agent-index-v0.1",
            "runtime_profile": cli_schema::runtime_profile_token(profile),
            "cli_schema": {
                "schema": "genesis/cli-schema-v0.1",
                "command": cli_schema,
            },
            "capability_indices": {
                "host_abi": {
                    "path": HOST_ABI_INDEX_PATH,
                    "loaded": host_abi_index.is_some(),
                    "index": host_abi_index,
                },
                "host_abi_schema": {
                    "path": HOST_ABI_SCHEMA_INDEX_PATH,
                    "loaded": host_abi_schema_index.is_some(),
                    "index": host_abi_schema_index,
                },
                "prelude_capabilities": {
                    "path": PRELUDE_CAP_INDEX_PATH,
                    "loaded": prelude_cap_index.is_some(),
                    "index": prelude_cap_index,
                },
            },
            "selfhost_symbol_index": selfhost_symbol_index,
            "language_symbol_index": {
                "path": AGENT_SYMBOL_INDEX_PATH,
                "kind": agent_symbol_index["kind"],
                "profile_id": agent_symbol_index["profileId"],
                "profile_identity_sha256": agent_symbol_index["profileIdentitySha256"],
                "index_identity_sha256": agent_symbol_index["indexIdentitySha256"],
                "symbol_count": agent_symbol_index["symbolCount"],
                "unsupported_behavior_count": agent_symbol_index["unsupportedBehaviorCount"],
                "unsupported_behavior_identity_sha256": agent_symbol_index["unsupportedBehaviorIdentitySha256"],
                "unsupported_classes": agent_symbol_index["unsupportedClasses"],
                "lookup": agent_symbol_index["lookup"],
            },
            "diagnostic_catalog": {
                "path": diagnostics::DIAGNOSTIC_CATALOG_PATH,
                "schema": diagnostic_catalog["kind"],
                "version": diagnostic_catalog["version"],
                "identity_sha256": diagnostic_catalog["catalogIdentitySha256"],
                "diagnostic_count": diagnostic_catalog["diagnosticCount"],
                "lookup": diagnostic_catalog["lookup"],
            },
            "obligation_defaults": [
                "core/obligation::unit-tests",
                "core/obligation::replayable-tests",
                "core/obligation::capabilities-declared",
                "core/obligation::determinism",
            ],
            "reference_workflows": workflows,
            "missing_sources": missing_sources,
            "docs": {
                "cli": "docs/spec/CLI.md",
                "schema_registry": "docs/spec/CLI_JSON_SCHEMAS_v0.1.md",
                "host_abi": "docs/spec/HOST_ABI.md",
                "host_abi_schema": "docs/spec/HOST_ABI.md",
                "foundation_stdlib": "docs/FOUNDATION_STDLIB_v0.2.md",
                "agent_index": "docs/spec/AGENT_INDEX_v0.1.md",
                "agent_plan": "docs/spec/AGENT_INDEX_v0.1.md#agent-plan-v01",
                "agent_authoring_bundle": "docs/spec/AGENT_AUTHORING_BUNDLE_v0.1.md",
                "gc_agent_profile": "docs/spec/GC_AGENT_PROFILE_v0.3.json",
                "gc_agent_core_card": "docs/spec/GC_AGENT_CORE_CARD_v0.3.md",
                "gc_agent_corpus": "docs/spec/GC_AGENT_CORPUS_v0.1.json",
                "gc_canonical_examples": "examples/canonical_language/v0.1/suite.json",
                "gc_agent_task_benchmark": "benchmarks/agent_tasks/v0.1/suite.json",
                "gc_agent_task_cards": "docs/spec/GC_AGENT_TASK_CARDS_v0.3.json",
                "gc_agent_symbol_index": AGENT_SYMBOL_INDEX_PATH,
                "diagnostic_catalog": diagnostics::DIAGNOSTIC_CATALOG_PATH,
                "write_genesiscode_skill_pack": "docs/spec/WRITE_GENESISCODE_SKILL_PACK_v0.1.md",
                "write_genesiscode_skill_distribution": "docs/spec/WRITE_GENESISCODE_SKILL_DISTRIBUTION_v1.md",
                "selfhost_symbol_ownership": "docs/spec/SELFHOST_SYMBOL_OWNERSHIP_INDEX_v0.1.md",
            }
        })),
        error: None,
    };
    let json = json_envelope_value(env)?;
    Ok(CmdOut {
        exit_code: EX_OK,
        stdout: if cli.json {
            String::new()
        } else {
            format!("{}\n", json_canonical_string(&json))
        },
        json,
    })
}
