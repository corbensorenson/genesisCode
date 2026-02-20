use super::*;

fn planned_policy_list_args(cli: &Cli, policies: &Path) -> Result<PathBuf, CliError> {
    let frontend = resolved_coreform_frontend(cli)?;
    if frontend_is_rust(&frontend) {
        Ok(policies.to_path_buf())
    } else {
        let req = Term::Map(
            [(
                TermOrdKey(Term::symbol(":policies")),
                Term::Str(policies.display().to_string()),
            )]
            .into_iter()
            .collect(),
        );
        let planned =
            selfhost_plan_request_map(cli, "core/cli::policy-list-request", req, "policy list")?;
        Ok(PathBuf::from(planned_required_str(
            &planned,
            ":policies",
            "policy list",
        )?))
    }
}

fn planned_policy_show_args(
    cli: &Cli,
    name_or_hash: &str,
    policies: &Path,
    store: &Path,
) -> Result<(String, PathBuf, PathBuf), CliError> {
    let frontend = resolved_coreform_frontend(cli)?;
    if frontend_is_rust(&frontend) {
        Ok((
            name_or_hash.to_string(),
            policies.to_path_buf(),
            store.to_path_buf(),
        ))
    } else {
        let req = Term::Map(
            [
                (
                    TermOrdKey(Term::symbol(":name-or-hash")),
                    Term::Str(name_or_hash.to_string()),
                ),
                (
                    TermOrdKey(Term::symbol(":policies")),
                    Term::Str(policies.display().to_string()),
                ),
                (
                    TermOrdKey(Term::symbol(":store")),
                    Term::Str(store.display().to_string()),
                ),
            ]
            .into_iter()
            .collect(),
        );
        let planned =
            selfhost_plan_request_map(cli, "core/cli::policy-show-request", req, "policy show")?;
        Ok((
            planned_required_str(&planned, ":name-or-hash", "policy show")?,
            PathBuf::from(planned_required_str(&planned, ":policies", "policy show")?),
            PathBuf::from(planned_required_str(&planned, ":store", "policy show")?),
        ))
    }
}

fn planned_policy_set_default_args(
    cli: &Cli,
    name_or_hash: &str,
    policies: &Path,
) -> Result<(String, PathBuf), CliError> {
    let frontend = resolved_coreform_frontend(cli)?;
    if frontend_is_rust(&frontend) {
        Ok((name_or_hash.to_string(), policies.to_path_buf()))
    } else {
        let req = Term::Map(
            [
                (
                    TermOrdKey(Term::symbol(":name-or-hash")),
                    Term::Str(name_or_hash.to_string()),
                ),
                (
                    TermOrdKey(Term::symbol(":policies")),
                    Term::Str(policies.display().to_string()),
                ),
            ]
            .into_iter()
            .collect(),
        );
        let planned = selfhost_plan_request_map(
            cli,
            "core/cli::policy-set-default-request",
            req,
            "policy set-default",
        )?;
        Ok((
            planned_required_str(&planned, ":name-or-hash", "policy set-default")?,
            PathBuf::from(planned_required_str(
                &planned,
                ":policies",
                "policy set-default",
            )?),
        ))
    }
}

pub(super) fn cmd_policy(cli: &Cli, cmd: &PolicyCmd) -> Result<CmdOut, CliError> {
    match cmd {
        PolicyCmd::List { policies } => {
            let policies_buf = planned_policy_list_args(cli, policies)?;
            let policies = policies_buf.as_path();
            let cfg = load_policies_config(policies)?;
            let default_resolved = cfg
                .default
                .as_deref()
                .and_then(|d| resolve_policy_selector(d, &cfg).ok().map(|(_, h)| h));
            let stdout = if cli.json {
                String::new()
            } else {
                let mut s = String::new();
                s.push_str("default ");
                match cfg.default.as_deref() {
                    Some(d) => s.push_str(d),
                    None => s.push_str("nil"),
                }
                s.push('\n');
                if let Some(h) = &default_resolved {
                    s.push_str("default-resolved ");
                    s.push_str(h);
                    s.push('\n');
                }
                for (name, hash) in &cfg.aliases {
                    s.push_str(name);
                    s.push(' ');
                    s.push_str(hash);
                    s.push('\n');
                }
                s
            };
            let env = JsonEnvelope {
                ok: true,
                kind: "genesis/policy-list-v0.1",
                data: Some(serde_json::json!({
                    "policies": policies.display().to_string(),
                    "default": cfg.default,
                    "default_resolved": default_resolved,
                    "aliases": cfg.aliases.iter().map(|(k, v)| serde_json::json!({"name": k, "hash": v})).collect::<Vec<_>>(),
                })),
                error: None,
            };
            Ok(CmdOut {
                exit_code: EX_OK,
                stdout,
                json: json_envelope_value(env)?,
            })
        }
        PolicyCmd::Show {
            name_or_hash,
            policies,
            store,
        } => {
            let (name_or_hash, policies_buf, store_buf) =
                planned_policy_show_args(cli, name_or_hash, policies, store)?;
            let policies = policies_buf.as_path();
            let store = store_buf.as_path();
            let cfg = load_policies_config(policies)?;
            let (resolved, hash) = resolve_policy_selector(&name_or_hash, &cfg)
                .map_err(|e| cli_err(EX_VERIFY, "policy/resolve", e))?;
            let p = store.join(&hash);
            let bytes = std::fs::read(&p)
                .with_context(|| format!("read {}", p.display()))
                .map_err(|e| cli_err(EX_IO, "io/read", format!("{e}")))?;
            let src = String::from_utf8(bytes).map_err(|e| {
                cli_err(
                    EX_PARSE,
                    "policy/parse",
                    format!("policy artifact {} is not utf-8: {e}", p.display()),
                )
            })?;
            let t =
                parse_term(&src).map_err(|e| cli_err(EX_PARSE, "policy/parse", format!("{e}")))?;
            let pol = gc_vcs::Policy::from_term(&t)
                .map_err(|e| cli_err(EX_PARSE, "policy/schema", format!("{e}")))?;
            let printed = print_term(&t);
            let stdout = if cli.json {
                String::new()
            } else {
                format!("{printed}\n")
            };
            let env = JsonEnvelope {
                ok: true,
                kind: "genesis/policy-show-v0.1",
                data: Some(serde_json::json!({
                    "query": name_or_hash,
                    "resolved": resolved,
                    "hash": hash,
                    "store": store.display().to_string(),
                    "term": printed,
                    "name": pol.name,
                    "frozen_prefixes": pol.frozen_prefixes,
                    "classes": {
                        "dev": pol.dev.is_some(),
                        "main": pol.main.is_some(),
                        "tags": pol.tags.is_some(),
                    }
                })),
                error: None,
            };
            Ok(CmdOut {
                exit_code: EX_OK,
                stdout,
                json: json_envelope_value(env)?,
            })
        }
        PolicyCmd::SetDefault {
            name_or_hash,
            policies,
        } => {
            let (name_or_hash, policies_buf) =
                planned_policy_set_default_args(cli, name_or_hash, policies)?;
            let policies = policies_buf.as_path();
            let mut cfg = load_policies_config(policies)?;
            if is_hex64(&name_or_hash) {
                cfg.default = Some(name_or_hash.to_ascii_lowercase());
            } else {
                let alias = name_or_hash.trim();
                if alias.is_empty() {
                    return Err(cli_err(
                        EX_PARSE,
                        "policy/set-default",
                        "policy selector must be non-empty",
                    ));
                }
                if !cfg.aliases.contains_key(alias) {
                    return Err(cli_err(
                        EX_VERIFY,
                        "policy/set-default",
                        format!("unknown policy alias `{alias}`"),
                    ));
                }
                cfg.default = Some(alias.to_string());
            }
            save_policies_config(policies, &cfg)?;
            let (_, resolved_hash) =
                resolve_policy_selector(cfg.default.as_deref().unwrap_or(""), &cfg)
                    .map_err(|e| cli_err(EX_VERIFY, "policy/set-default", e))?;
            let stdout = if cli.json {
                String::new()
            } else {
                format!(
                    "default {}\ndefault-resolved {}\n",
                    cfg.default.as_deref().unwrap_or("nil"),
                    resolved_hash
                )
            };
            let env = JsonEnvelope {
                ok: true,
                kind: "genesis/policy-set-default-v0.1",
                data: Some(serde_json::json!({
                    "policies": policies.display().to_string(),
                    "default": cfg.default,
                    "default_resolved": resolved_hash,
                })),
                error: None,
            };
            Ok(CmdOut {
                exit_code: EX_OK,
                stdout,
                json: json_envelope_value(env)?,
            })
        }
    }
}
