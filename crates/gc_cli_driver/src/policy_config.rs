use super::*;

fn policy_error(
    exit_code: u8,
    code: &'static str,
    kind: &'static str,
    operation: &'static str,
    message: impl Into<String>,
) -> CliError {
    let message = message.into();
    let context = structured_failures::FailureContext::new("policy", kind, operation)
        .fact("reason", message.clone())
        .into_value();
    cli_err_with_context(exit_code, code, message, context)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct PoliciesConfig {
    #[serde(default = "policy_config_version_one")]
    pub(super) version: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) default: Option<String>,
    #[serde(default)]
    pub(super) aliases: std::collections::BTreeMap<String, String>,
}

fn policy_config_version_one() -> u64 {
    1
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum PolicyAuthorityOperation {
    List,
    Resolve,
    SetDefault,
}

impl PolicyAuthorityOperation {
    fn symbol(self) -> &'static str {
        match self {
            Self::List => ":list",
            Self::Resolve => ":resolve",
            Self::SetDefault => ":set-default",
        }
    }
}

#[derive(Debug, Clone)]
pub(super) struct PolicyAuthorityDecision {
    pub(super) config: PoliciesConfig,
    pub(super) default_resolved: Option<String>,
    pub(super) resolved: Option<String>,
    pub(super) hash: Option<String>,
}

#[cfg(feature = "parity-harness")]
enum RustPolicyAuthorityError {
    Parse(String),
    Resolve(String),
    SetDefault(String),
}

impl Default for PoliciesConfig {
    fn default() -> Self {
        Self {
            version: 1,
            default: None,
            aliases: std::collections::BTreeMap::new(),
        }
    }
}

#[cfg(feature = "parity-harness")]
fn normalize_policies_config(mut cfg: PoliciesConfig) -> Result<PoliciesConfig, String> {
    if cfg.version != 1 {
        return Err(format!(
            "unsupported policies config version {} (expected 1)",
            cfg.version
        ));
    }
    let mut aliases = std::collections::BTreeMap::new();
    for (name_raw, hash_raw) in cfg.aliases {
        let name = name_raw.trim();
        if name.is_empty() {
            return Err("policy alias names must be non-empty".to_string());
        }
        let hash = hash_raw.trim();
        if !is_hex64(hash) {
            return Err(format!("policy alias `{name}` must map to a 64-hex hash"));
        }
        if aliases
            .insert(name.to_string(), hash.to_ascii_lowercase())
            .is_some()
        {
            return Err(format!("duplicate policy alias `{name}`"));
        }
    }
    cfg.aliases = aliases;
    if let Some(default_raw) = cfg.default.take() {
        let d = default_raw.trim();
        if d.is_empty() {
            return Err("default policy selector must be non-empty".to_string());
        }
        cfg.default = Some(if is_hex64(d) {
            d.to_ascii_lowercase()
        } else {
            d.to_string()
        });
    } else {
        cfg.default = None;
    }
    Ok(cfg)
}

pub(super) fn load_policies_config(path: &Path) -> Result<PoliciesConfig, CliError> {
    if !path.exists() {
        return Ok(PoliciesConfig::default());
    }
    let s = std::fs::read_to_string(path)
        .with_context(|| format!("read {}", path.display()))
        .map_err(|e| policy_error(EX_IO, "io/read", "io", "policy/load", format!("{e}")))?;
    let cfg: PoliciesConfig = toml::from_str(&s).map_err(|e| {
        policy_error(
            EX_PARSE,
            "policy/parse",
            "config-parse",
            "policy/load",
            format!("{e}"),
        )
    })?;
    Ok(cfg)
}

pub(super) fn save_policies_config(path: &Path, cfg: &PoliciesConfig) -> Result<(), CliError> {
    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
    {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("create {}", parent.display()))
            .map_err(|e| policy_error(EX_IO, "io/write", "io", "policy/save", format!("{e}")))?;
    }
    let s = toml::to_string_pretty(cfg).map_err(|e| {
        policy_error(
            EX_INTERNAL,
            "policy/serialize",
            "config-serialize",
            "policy/save",
            format!("{e}"),
        )
    })?;
    std::fs::write(path, s)
        .with_context(|| format!("write {}", path.display()))
        .map_err(|e| policy_error(EX_IO, "io/write", "io", "policy/save", format!("{e}")))
}

#[cfg(feature = "parity-harness")]
fn resolve_policy_selector(query: &str, cfg: &PoliciesConfig) -> Result<(String, String), String> {
    let q = query.trim();
    if q.is_empty() {
        return Err("policy selector must be non-empty".to_string());
    }
    if q == "default" {
        let Some(def) = cfg.default.as_deref() else {
            return Err("no default policy configured".to_string());
        };
        if def.trim() == "default" {
            return Err("default policy selector cannot resolve to itself".to_string());
        }
        return resolve_policy_selector(def, cfg);
    }
    if is_hex64(q) {
        let h = q.to_ascii_lowercase();
        return Ok((h.clone(), h));
    }
    let h = cfg
        .aliases
        .get(q)
        .ok_or_else(|| format!("unknown policy alias `{q}`"))?;
    Ok((q.to_string(), h.clone()))
}

fn policy_config_term(config: &PoliciesConfig) -> Term {
    let aliases = config
        .aliases
        .iter()
        .map(|(name, hash)| {
            Term::Map(
                [
                    (TermOrdKey(Term::symbol(":hash")), Term::Str(hash.clone())),
                    (TermOrdKey(Term::symbol(":name")), Term::Str(name.clone())),
                ]
                .into_iter()
                .collect(),
            )
        })
        .collect();
    Term::Map(
        [
            (TermOrdKey(Term::symbol(":aliases")), Term::Vector(aliases)),
            (
                TermOrdKey(Term::symbol(":default")),
                config
                    .default
                    .as_ref()
                    .map(|value| Term::Str(value.clone()))
                    .unwrap_or(Term::Nil),
            ),
            (
                TermOrdKey(Term::symbol(":version")),
                Term::Int(config.version.into()),
            ),
        ]
        .into_iter()
        .collect(),
    )
}

fn policy_authority_request(
    operation: PolicyAuthorityOperation,
    config: &PoliciesConfig,
    selector: Option<&str>,
) -> Term {
    Term::Map(
        [
            (
                TermOrdKey(Term::symbol(":config")),
                policy_config_term(config),
            ),
            (
                TermOrdKey(Term::symbol(":kind")),
                Term::Str("genesis/policy-authority-request-v0.1".to_string()),
            ),
            (
                TermOrdKey(Term::symbol(":operation")),
                Term::symbol(operation.symbol()),
            ),
            (
                TermOrdKey(Term::symbol(":selector")),
                selector
                    .map(|value| Term::Str(value.to_string()))
                    .unwrap_or(Term::Nil),
            ),
            (TermOrdKey(Term::symbol(":v")), Term::Int(1.into())),
        ]
        .into_iter()
        .collect(),
    )
}

fn authority_optional_string(
    report: &std::collections::BTreeMap<TermOrdKey, Term>,
    field: &str,
) -> Result<Option<String>, CliError> {
    match report.get(&TermOrdKey(Term::symbol(field))) {
        Some(Term::Str(value)) => Ok(Some(value.clone())),
        Some(Term::Nil) => Ok(None),
        _ => Err(policy_error(
            EX_PARSE,
            "policy/authority",
            "authority-report",
            "policy/decode",
            format!("policy authority report has invalid {field}"),
        )),
    }
}

fn authority_hash(value: Option<String>, field: &str) -> Result<Option<String>, CliError> {
    if let Some(hash) = value.as_deref()
        && (!is_hex64(hash) || hash != hash.to_ascii_lowercase())
    {
        return Err(policy_error(
            EX_PARSE,
            "policy/authority",
            "authority-report",
            "policy/decode",
            format!("policy authority report has noncanonical {field}"),
        ));
    }
    Ok(value)
}

fn decode_policy_aliases(
    term: &Term,
) -> Result<std::collections::BTreeMap<String, String>, CliError> {
    let Term::Vector(entries) = term else {
        return Err(policy_error(
            EX_PARSE,
            "policy/authority",
            "authority-report",
            "policy/decode",
            "policy authority :aliases must be a vector",
        ));
    };
    let mut aliases = std::collections::BTreeMap::new();
    for entry in entries {
        let Term::Map(entry) = entry else {
            return Err(policy_error(
                EX_PARSE,
                "policy/authority",
                "authority-report",
                "policy/decode",
                "policy authority alias entry must be a map",
            ));
        };
        if entry.len() != 2 {
            return Err(policy_error(
                EX_PARSE,
                "policy/authority",
                "authority-report",
                "policy/decode",
                "policy authority alias entry must contain exactly two fields",
            ));
        }
        let name = match entry.get(&TermOrdKey(Term::symbol(":name"))) {
            Some(Term::Str(value)) if !value.is_empty() => value.clone(),
            _ => {
                return Err(policy_error(
                    EX_PARSE,
                    "policy/authority",
                    "authority-report",
                    "policy/decode",
                    "policy authority alias name must be non-empty",
                ));
            }
        };
        let hash = match entry.get(&TermOrdKey(Term::symbol(":hash"))) {
            Some(Term::Str(value)) if is_hex64(value) && value == &value.to_ascii_lowercase() => {
                value.clone()
            }
            _ => {
                return Err(policy_error(
                    EX_PARSE,
                    "policy/authority",
                    "authority-report",
                    "policy/decode",
                    "policy authority alias hash must be canonical 64-hex",
                ));
            }
        };
        if aliases.insert(name, hash).is_some() {
            return Err(policy_error(
                EX_PARSE,
                "policy/authority",
                "authority-report",
                "policy/decode",
                "policy authority report contains duplicate aliases",
            ));
        }
    }
    Ok(aliases)
}

fn decode_policy_authority(
    report: std::collections::BTreeMap<TermOrdKey, Term>,
    operation: PolicyAuthorityOperation,
) -> Result<PolicyAuthorityDecision, CliError> {
    let identity_matches = matches!(report.get(&TermOrdKey(Term::symbol(":kind"))), Some(Term::Str(kind)) if kind == "genesis/policy-authority-result-v0.1")
        && matches!(report.get(&TermOrdKey(Term::symbol(":v"))), Some(Term::Int(version)) if version == &1.into())
        && matches!(report.get(&TermOrdKey(Term::symbol(":operation"))), Some(Term::Symbol(actual)) if actual == operation.symbol());
    if !identity_matches {
        return Err(policy_error(
            EX_PARSE,
            "policy/authority",
            "authority-report",
            "policy/decode",
            "policy authority result identity or operation mismatch",
        ));
    }
    match report.get(&TermOrdKey(Term::symbol(":ok"))) {
        Some(Term::Bool(false)) => return decode_policy_authority_failure(&report),
        Some(Term::Bool(true)) => {}
        _ => {
            return Err(policy_error(
                EX_PARSE,
                "policy/authority",
                "authority-report",
                "policy/decode",
                "policy authority result has invalid :ok",
            ));
        }
    }
    let expected_len = match operation {
        PolicyAuthorityOperation::List | PolicyAuthorityOperation::SetDefault => 7,
        PolicyAuthorityOperation::Resolve => 8,
    };
    if report.len() != expected_len {
        return Err(policy_error(
            EX_PARSE,
            "policy/authority",
            "authority-report",
            "policy/decode",
            "policy authority success field set mismatch",
        ));
    }
    let aliases = report
        .get(&TermOrdKey(Term::symbol(":aliases")))
        .ok_or_else(|| {
            policy_error(
                EX_PARSE,
                "policy/authority",
                "authority-report",
                "policy/decode",
                "policy authority result is missing :aliases",
            )
        })
        .and_then(decode_policy_aliases)?;
    let default = authority_optional_string(&report, ":default")?;
    let default_resolved = if operation == PolicyAuthorityOperation::Resolve {
        None
    } else {
        authority_hash(
            authority_optional_string(&report, ":default-resolved")?,
            ":default-resolved",
        )?
    };
    let resolved = if operation == PolicyAuthorityOperation::Resolve {
        authority_optional_string(&report, ":resolved")?
    } else {
        None
    };
    let hash = if operation == PolicyAuthorityOperation::Resolve {
        authority_hash(authority_optional_string(&report, ":hash")?, ":hash")?
    } else {
        None
    };
    Ok(PolicyAuthorityDecision {
        config: PoliciesConfig {
            version: 1,
            default,
            aliases,
        },
        default_resolved,
        resolved,
        hash,
    })
}

fn decode_policy_authority_failure(
    report: &std::collections::BTreeMap<TermOrdKey, Term>,
) -> Result<PolicyAuthorityDecision, CliError> {
    if report.len() != 6 {
        return Err(policy_error(
            EX_PARSE,
            "policy/authority",
            "authority-report",
            "policy/decode",
            "policy authority failure field set mismatch",
        ));
    }
    let message = match report.get(&TermOrdKey(Term::symbol(":error-message"))) {
        Some(Term::Str(message)) if !message.is_empty() => message.clone(),
        _ => {
            return Err(policy_error(
                EX_PARSE,
                "policy/authority",
                "authority-report",
                "policy/decode",
                "policy authority failure has invalid :error-message",
            ));
        }
    };
    let (exit_code, code, kind) = match report.get(&TermOrdKey(Term::symbol(":error-code"))) {
        Some(Term::Str(code)) if code == "policy/parse" => {
            (EX_PARSE, "policy/parse", "config-invalid")
        }
        Some(Term::Str(code)) if code == "policy/resolve" => {
            (EX_VERIFY, "policy/resolve", "selection-denied")
        }
        Some(Term::Str(code)) if code == "policy/set-default" => {
            (EX_VERIFY, "policy/set-default", "selection-denied")
        }
        _ => {
            return Err(policy_error(
                EX_PARSE,
                "policy/authority",
                "authority-report",
                "policy/decode",
                "policy authority failure has unknown :error-code",
            ));
        }
    };
    Err(policy_error(
        exit_code,
        code,
        kind,
        "policy/decision",
        message,
    ))
}

#[cfg(feature = "parity-harness")]
fn rust_policy_authority(
    operation: PolicyAuthorityOperation,
    config: PoliciesConfig,
    selector: Option<&str>,
) -> Result<PolicyAuthorityDecision, RustPolicyAuthorityError> {
    let mut config = normalize_policies_config(config).map_err(RustPolicyAuthorityError::Parse)?;
    match operation {
        PolicyAuthorityOperation::List => {
            let default_resolved = config.default.as_deref().and_then(|value| {
                resolve_policy_selector(value, &config)
                    .ok()
                    .map(|(_, hash)| hash)
            });
            Ok(PolicyAuthorityDecision {
                config,
                default_resolved,
                resolved: None,
                hash: None,
            })
        }
        PolicyAuthorityOperation::Resolve => {
            let (resolved, hash) = resolve_policy_selector(selector.unwrap_or(""), &config)
                .map_err(RustPolicyAuthorityError::Resolve)?;
            Ok(PolicyAuthorityDecision {
                config,
                default_resolved: None,
                resolved: Some(resolved),
                hash: Some(hash),
            })
        }
        PolicyAuthorityOperation::SetDefault => {
            let selector = selector.unwrap_or("").trim();
            if selector.is_empty() {
                return Err(RustPolicyAuthorityError::Parse(
                    "policy selector must be non-empty".to_string(),
                ));
            }
            if selector == "default" {
                return Err(RustPolicyAuthorityError::SetDefault(
                    "default policy selector cannot resolve to itself".to_string(),
                ));
            }
            if is_hex64(selector) {
                config.default = Some(selector.to_ascii_lowercase());
            } else {
                if selector.is_empty() || !config.aliases.contains_key(selector) {
                    return Err(RustPolicyAuthorityError::SetDefault(format!(
                        "unknown policy alias `{selector}`"
                    )));
                }
                config.default = Some(selector.to_string());
            }
            let (_, default_resolved) =
                resolve_policy_selector(config.default.as_deref().unwrap_or(""), &config)
                    .map_err(RustPolicyAuthorityError::SetDefault)?;
            Ok(PolicyAuthorityDecision {
                config,
                default_resolved: Some(default_resolved),
                resolved: None,
                hash: None,
            })
        }
    }
}

pub(super) fn authoritative_policy_decision(
    cli: &Cli,
    operation: PolicyAuthorityOperation,
    config: PoliciesConfig,
    selector: Option<&str>,
) -> Result<PolicyAuthorityDecision, CliError> {
    let frontend = resolved_coreform_frontend(cli)?;
    if frontend_is_rust(&frontend) {
        #[cfg(not(feature = "parity-harness"))]
        return Err(policy_error(
            EX_INTERNAL,
            "policy/authority",
            "authority-unavailable",
            "policy/dispatch",
            "Rust policy oracle is not compiled into production",
        ));
        #[cfg(feature = "parity-harness")]
        return rust_policy_authority(operation, config, selector).map_err(|error| {
            let (exit, code, message) = match error {
                RustPolicyAuthorityError::Parse(message) => (EX_PARSE, "policy/parse", message),
                RustPolicyAuthorityError::Resolve(message) => {
                    (EX_VERIFY, "policy/resolve", message)
                }
                RustPolicyAuthorityError::SetDefault(message) => {
                    (EX_VERIFY, "policy/set-default", message)
                }
            };
            policy_error(exit, code, "parity-oracle", "policy/dispatch", message)
        });
    }
    let report = selfhost_plan_request_map(
        cli,
        "core/cli::policy-authority",
        policy_authority_request(operation, &config, selector),
        "policy authority",
    )?;
    decode_policy_authority(report, operation)
}

#[cfg(test)]
#[path = "tests_policy_config.rs"]
mod tests;
