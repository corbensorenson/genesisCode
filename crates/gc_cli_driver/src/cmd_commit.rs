use super::*;

pub(super) fn cmd_commit(
    cli: &Cli,
    caps: &Path,
    log: Option<&Path>,
    cmd: &CommitCmd,
) -> Result<CmdOut, CliError> {
    let frontend = resolved_coreform_frontend(cli)?;
    let frontend_info = coreform_frontend_json(&frontend);

    let policy = CapsPolicy::load(caps)
        .with_context(|| format!("read {}", caps.display()))
        .map_err(caps_parse_cli_err)?;

    let mut logs = LogAccumulator::default();

    let (value, stdout) = match cmd {
        CommitCmd::New {
            target_kind,
            target_id,
            base,
            patch,
            message,
            why,
            obligations,
            evidence,
            author,
            sign,
            store,
        } => {
            if target_id.trim().is_empty() {
                return Err(cli_err(
                    EX_PARSE,
                    "commit/new",
                    "invalid --target-id: empty value",
                ));
            }
            if message.trim().is_empty() {
                return Err(cli_err(
                    EX_PARSE,
                    "commit/new",
                    "invalid --message: empty value",
                ));
            }
            for o in obligations {
                if o.trim().is_empty() {
                    return Err(cli_err(
                        EX_PARSE,
                        "commit/new",
                        "invalid --obligation: empty value",
                    ));
                }
            }
            for h in evidence {
                gc_vcs::validate_hex_hash(h).map_err(|e| {
                    cli_err(
                        EX_PARSE,
                        "commit/new",
                        format!("invalid --evidence hash `{h}`: {e}"),
                    )
                })?;
            }

            let (base_snapshot, parents) =
                resolve_base_snapshot(cli, &policy, &mut logs, base.as_str())?;
            let patch_hash = resolve_patch_hash(cli, &policy, &mut logs, patch.as_str())?;
            let result_snapshot = apply_patch_for_result(
                cli,
                &policy,
                &mut logs,
                base_snapshot.as_str(),
                patch_hash.as_str(),
            )?;

            let artifact = build_commit_artifact(
                *target_kind,
                target_id,
                &parents,
                &base_snapshot,
                &patch_hash,
                &result_snapshot,
                obligations,
                evidence,
                message,
                why.as_deref(),
                author.as_deref(),
                sign.as_deref(),
            );
            let commit_hash = gc_vcs::bytes32_to_hex(&gc_coreform::hash_term(&artifact));

            if *store {
                let (store_v, store_log) =
                    run_effect_forms(cli, &policy, mk_store_put_program(&artifact))?;
                logs.push(store_log);
                let stored = extract_hash_field(&store_v, ":hash").ok_or_else(|| {
                    cli_err(
                        EX_INTERNAL,
                        "commit/new",
                        "store put returned no :hash for commit artifact",
                    )
                })?;
                if stored != commit_hash {
                    return Err(cli_err(
                        EX_INTERNAL,
                        "commit/new",
                        format!(
                            "stored commit hash mismatch: computed={commit_hash} stored={stored}"
                        ),
                    ));
                }
            }

            let mut out = BTreeMap::new();
            out.insert(TermOrdKey(Term::symbol(":ok")), Term::Bool(true));
            out.insert(TermOrdKey(Term::symbol(":stored")), Term::Bool(*store));
            out.insert(
                TermOrdKey(Term::symbol(":commit")),
                Term::Str(commit_hash.clone()),
            );
            out.insert(
                TermOrdKey(Term::symbol(":base")),
                Term::Str(base_snapshot.clone()),
            );
            out.insert(
                TermOrdKey(Term::symbol(":patch")),
                Term::Str(patch_hash.clone()),
            );
            out.insert(
                TermOrdKey(Term::symbol(":result")),
                Term::Str(result_snapshot.clone()),
            );
            out.insert(TermOrdKey(Term::symbol(":artifact")), artifact);
            (
                Value::Data(Term::Map(out)),
                if cli.json {
                    String::new()
                } else {
                    format!("{commit_hash}\n")
                },
            )
        }
        CommitCmd::Show { hash } => {
            gc_vcs::validate_hex_hash(hash)
                .map_err(|e| cli_err(EX_PARSE, "commit/show", format!("invalid hash: {e}")))?;
            let (resp, step_log) = run_effect_forms(cli, &policy, mk_store_get_program(hash))?;
            logs.push(step_log);
            let artifact = extract_artifact_field(&resp).ok_or_else(|| {
                cli_err(
                    EX_INTERNAL,
                    "commit/show",
                    "store get response missing :artifact",
                )
            })?;
            gc_vcs::Commit::from_term(&artifact)
                .map_err(|e| cli_err(EX_PARSE, "commit/show", format!("invalid commit: {e}")))?;

            let mut out = BTreeMap::new();
            out.insert(TermOrdKey(Term::symbol(":ok")), Term::Bool(true));
            out.insert(TermOrdKey(Term::symbol(":hash")), Term::Str(hash.clone()));
            out.insert(TermOrdKey(Term::symbol(":artifact")), artifact.clone());
            (
                Value::Data(Term::Map(out)),
                if cli.json {
                    String::new()
                } else {
                    format!("{}\n", gc_coreform::print_term(&artifact))
                },
            )
        }
    };

    let log_path = log
        .map(PathBuf::from)
        .unwrap_or_else(|| default_log_path(commit_contract::log_op(cmd)));
    let final_log = logs.into_log();
    std::fs::write(&log_path, final_log.to_string_canonical() + "\n")
        .with_context(|| format!("write {}", log_path.display()))
        .map_err(|e| cli_err(EX_IO, "io/write", format!("{e}")))?;

    let (value_s, value_format) = render_value_for_cli(&mk_ctx(cli), &value);
    let env = JsonEnvelope {
        ok: true,
        kind: commit_contract::kind(cmd),
        data: Some(serde_json::json!({
            "coreform_frontend": frontend_info,
            "caps": caps.display().to_string(),
            "log": log_path.display().to_string(),
            "value": value_s,
            "value_format": value_format,
        })),
        error: None,
    };

    Ok(CmdOut {
        exit_code: EX_OK,
        stdout,
        json: json_envelope_value(env)?,
    })
}

#[derive(Default)]
struct LogAccumulator {
    initialized: bool,
    version: u64,
    program_hash: [u8; 32],
    toolchain: String,
    entries: Vec<gc_effects::EffectLogEntry>,
}

impl LogAccumulator {
    fn push(&mut self, log: gc_effects::EffectLog) {
        if !self.initialized {
            self.initialized = true;
            self.version = log.version;
            self.program_hash = log.program_hash;
            self.toolchain = log.toolchain.clone();
        }
        for mut entry in log.entries {
            entry.i = self.entries.len() as u64;
            self.entries.push(entry);
        }
    }

    fn into_log(self) -> gc_effects::EffectLog {
        if self.initialized {
            gc_effects::EffectLog {
                version: self.version,
                program_hash: self.program_hash,
                toolchain: self.toolchain,
                entries: self.entries,
            }
        } else {
            gc_effects::EffectLog {
                version: 3,
                program_hash: [0u8; 32],
                toolchain: format!("genesis {}", env!("CARGO_PKG_VERSION")),
                entries: Vec::new(),
            }
        }
    }
}

fn resolve_base_snapshot(
    cli: &Cli,
    policy: &CapsPolicy,
    logs: &mut LogAccumulator,
    base: &str,
) -> Result<(String, Vec<String>), CliError> {
    if base.starts_with("refs/") {
        let (refs_v, refs_log) = run_effect_forms(cli, policy, mk_refs_get_program(base))?;
        logs.push(refs_log);
        let parent_hash = extract_hash_field(&refs_v, ":hash").ok_or_else(|| {
            cli_err(
                EX_INTERNAL,
                "commit/new",
                "refs get returned no :hash for base ref",
            )
        })?;
        if parent_hash == "nil" {
            return Err(cli_err(
                EX_PARSE,
                "commit/new",
                format!("base ref `{base}` is unset"),
            ));
        }
        gc_vcs::validate_hex_hash(&parent_hash).map_err(|e| {
            cli_err(
                EX_PARSE,
                "commit/new",
                format!("base ref `{base}` resolved to invalid hash: {e}"),
            )
        })?;

        let (store_v, store_log) =
            run_effect_forms(cli, policy, mk_store_get_program(&parent_hash))?;
        logs.push(store_log);
        let artifact = extract_artifact_field(&store_v).ok_or_else(|| {
            cli_err(
                EX_INTERNAL,
                "commit/new",
                "store get returned no :artifact for base ref commit",
            )
        })?;
        let commit = gc_vcs::Commit::from_term(&artifact).map_err(|e| {
            cli_err(
                EX_PARSE,
                "commit/new",
                format!("base ref commit artifact is invalid: {e}"),
            )
        })?;
        Ok((commit.result, vec![parent_hash]))
    } else {
        gc_vcs::validate_hex_hash(base)
            .map_err(|e| cli_err(EX_PARSE, "commit/new", format!("invalid --base: {e}")))?;
        Ok((base.to_ascii_lowercase(), Vec::new()))
    }
}

fn resolve_patch_hash(
    cli: &Cli,
    policy: &CapsPolicy,
    logs: &mut LogAccumulator,
    patch: &str,
) -> Result<String, CliError> {
    if gc_vcs::validate_hex_hash(patch).is_ok() {
        return Ok(patch.to_ascii_lowercase());
    }

    let patch_path = PathBuf::from(patch);
    let src = std::fs::read_to_string(&patch_path)
        .with_context(|| format!("read {}", patch_path.display()))
        .map_err(|e| cli_err(EX_IO, "io/read", format!("{e}")))?;
    let patch_term =
        parse_term(&src).map_err(|e| cli_err(EX_PARSE, "parse/term", e.to_string()))?;

    let (put_v, put_log) = run_effect_forms(cli, policy, mk_store_put_program(&patch_term))?;
    logs.push(put_log);
    extract_hash_field(&put_v, ":hash").ok_or_else(|| {
        cli_err(
            EX_INTERNAL,
            "commit/new",
            "store put returned no :hash for patch artifact",
        )
    })
}

fn apply_patch_for_result(
    cli: &Cli,
    policy: &CapsPolicy,
    logs: &mut LogAccumulator,
    base_snapshot: &str,
    patch_hash: &str,
) -> Result<String, CliError> {
    let (apply_v, apply_log) = run_effect_forms(
        cli,
        policy,
        mk_vcs_apply_program(base_snapshot, patch_hash, None, true),
    )?;
    logs.push(apply_log);
    extract_hash_field(&apply_v, ":snapshot").ok_or_else(|| {
        cli_err(
            EX_INTERNAL,
            "commit/new",
            "vcs apply returned no :snapshot hash",
        )
    })
}

#[expect(
    clippy::too_many_arguments,
    reason = "commit artifact assembly requires explicit fields"
)]
fn build_commit_artifact(
    target_kind: CommitTargetKind,
    target_id: &str,
    parents: &[String],
    base_snapshot: &str,
    patch_hash: &str,
    result_snapshot: &str,
    obligations: &[String],
    evidence: &[String],
    message: &str,
    why: Option<&str>,
    author: Option<&str>,
    sign: Option<&str>,
) -> Term {
    let mut target = BTreeMap::new();
    target.insert(
        TermOrdKey(Term::symbol(":kind")),
        Term::symbol(match target_kind {
            CommitTargetKind::Package => ":package",
            CommitTargetKind::Module => ":module",
            CommitTargetKind::Contract => ":contract",
            CommitTargetKind::Workspace => ":workspace",
        }),
    );
    target.insert(
        TermOrdKey(Term::symbol(":name")),
        Term::Str(target_id.to_string()),
    );

    let mut m = BTreeMap::new();
    m.insert(
        TermOrdKey(Term::symbol(":type")),
        Term::symbol(":vcs/commit"),
    );
    m.insert(TermOrdKey(Term::symbol(":v")), Term::Int(1.into()));
    m.insert(
        TermOrdKey(Term::symbol(":parents")),
        Term::Vector(parents.iter().cloned().map(Term::Str).collect()),
    );
    m.insert(TermOrdKey(Term::symbol(":target")), Term::Map(target));
    m.insert(
        TermOrdKey(Term::symbol(":base")),
        Term::Str(base_snapshot.to_string()),
    );
    m.insert(
        TermOrdKey(Term::symbol(":patch")),
        Term::Str(patch_hash.to_string()),
    );
    m.insert(
        TermOrdKey(Term::symbol(":result")),
        Term::Str(result_snapshot.to_string()),
    );
    m.insert(
        TermOrdKey(Term::symbol(":obligations")),
        Term::Vector(obligations.iter().cloned().map(Term::Str).collect()),
    );
    m.insert(
        TermOrdKey(Term::symbol(":evidence")),
        Term::Vector(evidence.iter().cloned().map(Term::Str).collect()),
    );
    m.insert(
        TermOrdKey(Term::symbol(":attestations")),
        Term::Vector(Vec::new()),
    );
    m.insert(
        TermOrdKey(Term::symbol(":message")),
        Term::Str(message.to_string()),
    );
    if let Some(why) = why {
        m.insert(TermOrdKey(Term::symbol(":why")), Term::Str(why.to_string()));
    }
    if author.is_some() || sign.is_some() {
        let mut am = BTreeMap::new();
        if let Some(name) = author {
            am.insert(
                TermOrdKey(Term::symbol(":name")),
                Term::Str(name.to_string()),
            );
        }
        if let Some(id) = sign {
            am.insert(TermOrdKey(Term::symbol(":id")), Term::Str(id.to_string()));
        }
        m.insert(TermOrdKey(Term::symbol(":author")), Term::Map(am));
    }
    Term::Map(m)
}

fn run_effect_forms(
    cli: &Cli,
    policy: &CapsPolicy,
    forms: Vec<Term>,
) -> Result<(Value, gc_effects::EffectLog), CliError> {
    let mut ctx = mk_ctx(cli);
    let prelude = build_prelude(&mut ctx);
    let mut env = prelude.env;

    let forms = canonicalize_module(forms)
        .map_err(|e| cli_err(EX_PARSE, "canon/coreform", e.to_string()))?;
    let program_hash = hash_module(&forms);
    let prog = eval_module(&mut ctx, &mut env, &forms)
        .map_err(|e| cli_err(EX_EVAL, "eval/error", format!("{e}")))?;

    let toolchain = format!("genesis {}", env!("CARGO_PKG_VERSION"));
    let run = gc_effects::run(&mut ctx, policy, prog, program_hash, toolchain)
        .map_err(|e| cli_err(EX_EVAL, "effects/run", format!("{e}")))?;
    enforce_no_legacy_semantic_fallback_in_selfhost_only(cli, "commit", &run.log)?;
    if let Some((code, message, payload)) = extract_protocol_error(&ctx, &run.value) {
        let exit = if code == "core/caps/denied" {
            EX_CAPS_DENIED
        } else if code == "core/vcs/conflict" {
            3
        } else {
            EX_EVAL
        };
        return Err(CliError {
            exit_code: exit,
            json: JsonError {
                code: commit_error_json_code(&code),
                message: format!("{code}: {message}"),
                context: payload.map(serde_json::Value::String),
            },
        });
    }
    Ok((run.value, run.log))
}

fn extract_hash_field(v: &Value, key: &str) -> Option<String> {
    let t = v.to_term_for_log(None);
    let Term::Map(m) = t else { return None };
    match m.get(&TermOrdKey(Term::symbol(key))) {
        Some(Term::Str(s)) => Some(s.clone()),
        _ => None,
    }
}

fn extract_artifact_field(v: &Value) -> Option<Term> {
    let t = v.to_term_for_log(None);
    let Term::Map(m) = t else { return None };
    m.get(&TermOrdKey(Term::symbol(":artifact"))).cloned()
}

fn commit_error_json_code(code: &str) -> &'static str {
    match code {
        "core/caps/denied" => "core/caps/denied",
        "core/store/not-found" => "core/store/not-found",
        "core/store/bad-hash" => "core/store/bad-hash",
        "core/store/corruption" => "core/store/corruption",
        "core/store/io-error" => "core/store/io-error",
        "core/vcs/conflict" => "core/vcs/conflict",
        "core/vcs/bad-payload" => "core/vcs/bad-payload",
        "core/vcs/bad-commit" => "core/vcs/bad-commit",
        _ => "commit/error",
    }
}
