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

impl Default for PoliciesConfig {
    fn default() -> Self {
        Self {
            version: 1,
            default: None,
            aliases: std::collections::BTreeMap::new(),
        }
    }
}

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
    normalize_policies_config(cfg)
        .map_err(|e| policy_error(EX_PARSE, "policy/parse", "config-invalid", "policy/load", e))
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

pub(super) fn resolve_policy_selector(
    query: &str,
    cfg: &PoliciesConfig,
) -> Result<(String, String), String> {
    let q = query.trim();
    if q.is_empty() {
        return Err("policy selector must be non-empty".to_string());
    }
    if q == "default" {
        let Some(def) = cfg.default.as_deref() else {
            return Err("no default policy configured".to_string());
        };
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
