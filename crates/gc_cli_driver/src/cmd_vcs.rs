use super::*;

pub(super) fn cmd_vcs(
    cli: &Cli,
    caps: Option<&Path>,
    log: Option<&Path>,
    cmd: &VcsCmd,
) -> Result<CmdOut, CliError> {
    if let VcsCmd::Hash { input, engine } = cmd {
        let engine = resolved_engine(cli, "vcs hash", *engine)?;
        let src = std::fs::read_to_string(input)
            .with_context(|| format!("read {}", input.display()))
            .map_err(|e| cli_err(EX_IO, "io/read", format!("{e}")))?;
        let (hash_hex, hk) = if engine == FmtEngine::Selfhost {
            let mut ctx = EvalCtx::with_step_limit(None);
            ctx.set_mem_limits(resolved_mem_limits(cli));
            let prelude = build_prelude(&mut ctx);
            let mut env = prelude.env;
            load_selfhost_toolchain(cli, &mut ctx, &mut env)?;

            let f = env.get("core/cli::hash-src-with-kind").ok_or_else(|| {
                cli_err(
                    EX_INTERNAL,
                    "selfhost/missing",
                    "missing binding core/cli::hash-src-with-kind",
                )
            })?;

            ctx.steps = 0;
            ctx.step_limit = resolved_step_limit(cli).resolve();
            let r = f
                .apply(&mut ctx, Value::Data(Term::Str(src.clone())))
                .map_err(|e| {
                    cli_err(
                        EX_EVAL,
                        "eval/error",
                        format!("selfhost vcs hash failed: {e}"),
                    )
                })?;
            if let Some((code, message, payload)) = extract_protocol_error(&ctx, &r) {
                return Err(CliError {
                    exit_code: EX_PARSE,
                    json: JsonError {
                        code: "selfhost/error",
                        message: format!("{code}: {message}"),
                        context: payload.map(serde_json::Value::String),
                    },
                });
            }
            let (hash_hex, hk) = match r {
                Value::Data(Term::Map(m)) => {
                    let hash_hex = match m.get(&TermOrdKey(Term::symbol(":hash"))) {
                        Some(Term::Str(s)) => s.clone(),
                        _ => {
                            return Err(cli_err(
                                EX_INTERNAL,
                                "selfhost/bad-return",
                                "selfhost vcs hash return missing :hash string",
                            ));
                        }
                    };
                    let hk = match m.get(&TermOrdKey(Term::symbol(":kind"))) {
                        Some(Term::Str(s)) if s == "term" || s == "module" => s.clone(),
                        _ => {
                            return Err(cli_err(
                                EX_INTERNAL,
                                "selfhost/bad-return",
                                "selfhost vcs hash return missing :kind string",
                            ));
                        }
                    };
                    (hash_hex, hk)
                }
                Value::Map(m) => {
                    let hash_hex = match m.get(&TermOrdKey(Term::symbol(":hash"))) {
                        Some(Value::Data(Term::Str(s))) => s.clone(),
                        _ => {
                            return Err(cli_err(
                                EX_INTERNAL,
                                "selfhost/bad-return",
                                "selfhost vcs hash return missing :hash string",
                            ));
                        }
                    };
                    let hk = match m.get(&TermOrdKey(Term::symbol(":kind"))) {
                        Some(Value::Data(Term::Str(s))) if s == "term" || s == "module" => {
                            s.clone()
                        }
                        _ => {
                            return Err(cli_err(
                                EX_INTERNAL,
                                "selfhost/bad-return",
                                "selfhost vcs hash return missing :kind string",
                            ));
                        }
                    };
                    (hash_hex, hk)
                }
                _ => {
                    return Err(cli_err(
                        EX_INTERNAL,
                        "selfhost/bad-return",
                        format!("selfhost vcs hash returned non-map: {}", r.debug_repr()),
                    ));
                }
            };
            (hash_hex, hk)
        } else {
            let (h, hk) = match parse_term(&src) {
                Ok(t) => (gc_coreform::hash_term(&t), "term"),
                Err(_) => {
                    let forms = parse_module(&src)
                        .map_err(|e| cli_err(EX_PARSE, "parse/coreform", e.to_string()))?;
                    let forms = canonicalize_module(forms)
                        .map_err(|e| cli_err(EX_PARSE, "canon/coreform", e.to_string()))?;
                    (hash_module(&forms), "module")
                }
            };
            (gc_vcs::bytes32_to_hex(&h), hk.to_string())
        };

        let env = JsonEnvelope {
            ok: true,
            kind: "genesis/vcs-hash-v0.2",
            data: Some(serde_json::json!({
                "in": input.display().to_string(),
                // Keep legacy field for backward-compat while standardizing on `in`.
                "input": input.display().to_string(),
                "hash": hash_hex,
                "hash_kind": hk,
                "hash_format": "hex",
                "engine": if engine == FmtEngine::Selfhost { "selfhost" } else { "rust" },
            })),
            error: None,
        };
        return Ok(CmdOut {
            exit_code: EX_OK,
            stdout: if cli.json {
                String::new()
            } else {
                format!("{hash_hex}\n")
            },
            json: serde_json::to_value(env).expect("json"),
        });
    }

    let frontend = resolved_coreform_frontend(cli)?;
    let frontend_info = coreform_frontend_json(&frontend);

    let caps = caps.ok_or_else(|| {
        cli_err(
            EX_PARSE,
            "caps/missing",
            "missing --caps (required for effectful vcs operations)",
        )
    })?;

    let policy = CapsPolicy::load(caps)
        .with_context(|| format!("read {}", caps.display()))
        .map_err(|e| cli_err(EX_PARSE, "caps/parse", format!("{e}")))?;

    let mut ctx = mk_ctx(cli);
    let prelude = build_prelude(&mut ctx);
    let mut env = prelude.env;
    let (prog, kind, log_op, program_hash) = match frontend {
        gc_obligations::CoreformFrontend::Rust => {
            let (forms, kind, log_op) = match cmd {
                VcsCmd::Hash { .. } => unreachable!("handled above"),
                VcsCmd::Diff {
                    base,
                    to,
                    out,
                    no_store,
                } => (
                    mk_vcs_diff_program(base, to, out.as_deref(), !*no_store),
                    "genesis/vcs-diff-v0.1",
                    "vcs-diff",
                ),
                VcsCmd::Apply {
                    base,
                    patch,
                    out,
                    no_store,
                } => (
                    mk_vcs_apply_program(base, patch, out.as_deref(), !*no_store),
                    "genesis/vcs-apply-v0.1",
                    "vcs-apply",
                ),
                VcsCmd::Log { root, max } => (
                    mk_vcs_log_program(root, *max),
                    "genesis/vcs-log-v0.1",
                    "vcs-log",
                ),
                VcsCmd::Blame {
                    snapshot,
                    sym,
                    path,
                } => (
                    mk_vcs_blame_program(snapshot, sym, path.as_deref())?,
                    "genesis/vcs-blame-v0.1",
                    "vcs-blame",
                ),
                VcsCmd::Why { snapshot, sym, op } => (
                    mk_vcs_why_program(snapshot, sym, op.as_deref())?,
                    "genesis/vcs-why-v0.1",
                    "vcs-why",
                ),
                VcsCmd::Merge3 {
                    base,
                    left,
                    right,
                    out,
                } => (
                    mk_vcs_merge3_program(base, left, right, out.as_deref()),
                    "genesis/vcs-merge3-v0.1",
                    "vcs-merge3",
                ),
                VcsCmd::ResolveConflict {
                    conflict,
                    strategy,
                    picks,
                    sets,
                    out,
                } => (
                    mk_vcs_resolve_conflict_program(
                        conflict,
                        strategy.as_deref(),
                        picks,
                        sets,
                        out.as_deref(),
                    )?,
                    "genesis/vcs-resolve-conflict-v0.1",
                    "vcs-resolve-conflict",
                ),
            };

            let forms = canonicalize_module(forms)
                .map_err(|e| cli_err(EX_PARSE, "canon/coreform", e.to_string()))?;
            let program_hash = hash_module(&forms);
            let prog = eval_module(&mut ctx, &mut env, &forms)
                .map_err(|e| cli_err(EX_EVAL, "eval/error", format!("{e}")))?;
            Ok::<_, CliError>((prog, kind, log_op, program_hash))
        }
        gc_obligations::CoreformFrontend::Selfhost(_) => {
            load_selfhost_toolchain(cli, &mut ctx, &mut env)?;

            let (prog, kind, log_op, desc) = match cmd {
                VcsCmd::Hash { .. } => unreachable!("handled above"),
                VcsCmd::Log { root, max } => {
                    let f = env.get("core/cli::vcs-log-program").ok_or_else(|| {
                        cli_err(
                            EX_INTERNAL,
                            "selfhost/missing",
                            "missing binding core/cli::vcs-log-program",
                        )
                    })?;
                    let req = Term::Map(
                        [
                            (
                                TermOrdKey(Term::symbol(":root")),
                                Term::Str(root.to_string()),
                            ),
                            (
                                TermOrdKey(Term::symbol(":max")),
                                Term::Int((*max as i64).into()),
                            ),
                        ]
                        .into_iter()
                        .collect(),
                    );
                    let prog = f.apply(&mut ctx, Value::Data(req)).map_err(|e| {
                        cli_err(
                            EX_EVAL,
                            "eval/error",
                            format!("core/cli vcs-log-program failed: {e}"),
                        )
                    })?;
                    let desc = Term::Map(
                        [
                            (
                                TermOrdKey(Term::symbol(":cmd")),
                                Term::Str("vcs/log".to_string()),
                            ),
                            (
                                TermOrdKey(Term::symbol(":root")),
                                Term::Str(root.to_string()),
                            ),
                            (
                                TermOrdKey(Term::symbol(":max")),
                                Term::Int((*max as i64).into()),
                            ),
                        ]
                        .into_iter()
                        .collect(),
                    );
                    (prog, "genesis/vcs-log-v0.1", "vcs-log", desc)
                }
                VcsCmd::Blame {
                    snapshot,
                    sym,
                    path,
                } => {
                    gc_vcs::validate_hex_hash(snapshot).map_err(|e| {
                        cli_err(EX_PARSE, "vcs/blame", format!("invalid --snapshot: {e}"))
                    })?;
                    if sym.trim().is_empty() {
                        return Err(cli_err(EX_PARSE, "vcs/blame", "invalid --sym: empty value"));
                    }

                    let f = env.get("core/cli::vcs-blame-program").ok_or_else(|| {
                        cli_err(
                            EX_INTERNAL,
                            "selfhost/missing",
                            "missing binding core/cli::vcs-blame-program",
                        )
                    })?;
                    let mut mm = std::collections::BTreeMap::new();
                    mm.insert(
                        TermOrdKey(Term::symbol(":snapshot")),
                        Term::Str(snapshot.to_string()),
                    );
                    mm.insert(TermOrdKey(Term::symbol(":sym")), Term::Str(sym.to_string()));
                    if let Some(p) = path.as_deref() {
                        mm.insert(TermOrdKey(Term::symbol(":path")), Term::Str(p.to_string()));
                    }
                    let req = Term::Map(mm);
                    let prog = f.apply(&mut ctx, Value::Data(req)).map_err(|e| {
                        cli_err(
                            EX_EVAL,
                            "eval/error",
                            format!("core/cli vcs-blame-program failed: {e}"),
                        )
                    })?;

                    let mut dm = std::collections::BTreeMap::new();
                    dm.insert(
                        TermOrdKey(Term::symbol(":cmd")),
                        Term::Str("vcs/blame".to_string()),
                    );
                    dm.insert(
                        TermOrdKey(Term::symbol(":snapshot")),
                        Term::Str(snapshot.to_string()),
                    );
                    dm.insert(TermOrdKey(Term::symbol(":sym")), Term::Str(sym.to_string()));
                    if let Some(p) = path.as_deref() {
                        dm.insert(TermOrdKey(Term::symbol(":path")), Term::Str(p.to_string()));
                    }
                    let desc = Term::Map(dm);
                    (prog, "genesis/vcs-blame-v0.1", "vcs-blame", desc)
                }
                VcsCmd::Why { snapshot, sym, op } => {
                    gc_vcs::validate_hex_hash(snapshot).map_err(|e| {
                        cli_err(EX_PARSE, "vcs/why", format!("invalid --snapshot: {e}"))
                    })?;
                    if sym.trim().is_empty() {
                        return Err(cli_err(EX_PARSE, "vcs/why", "invalid --sym: empty value"));
                    }
                    if let Some(op) = op.as_deref()
                        && op.trim().is_empty()
                    {
                        return Err(cli_err(EX_PARSE, "vcs/why", "invalid --op: empty value"));
                    }

                    let f = env.get("core/cli::vcs-why-program").ok_or_else(|| {
                        cli_err(
                            EX_INTERNAL,
                            "selfhost/missing",
                            "missing binding core/cli::vcs-why-program",
                        )
                    })?;
                    let mut mm = std::collections::BTreeMap::new();
                    mm.insert(
                        TermOrdKey(Term::symbol(":snapshot")),
                        Term::Str(snapshot.to_string()),
                    );
                    mm.insert(TermOrdKey(Term::symbol(":sym")), Term::Str(sym.to_string()));
                    if let Some(o) = op.as_deref() {
                        mm.insert(TermOrdKey(Term::symbol(":op")), Term::Str(o.to_string()));
                    }
                    let req = Term::Map(mm);
                    let prog = f.apply(&mut ctx, Value::Data(req)).map_err(|e| {
                        cli_err(
                            EX_EVAL,
                            "eval/error",
                            format!("core/cli vcs-why-program failed: {e}"),
                        )
                    })?;

                    let mut dm = std::collections::BTreeMap::new();
                    dm.insert(
                        TermOrdKey(Term::symbol(":cmd")),
                        Term::Str("vcs/why".to_string()),
                    );
                    dm.insert(
                        TermOrdKey(Term::symbol(":snapshot")),
                        Term::Str(snapshot.to_string()),
                    );
                    dm.insert(TermOrdKey(Term::symbol(":sym")), Term::Str(sym.to_string()));
                    if let Some(o) = op.as_deref() {
                        dm.insert(TermOrdKey(Term::symbol(":op")), Term::Str(o.to_string()));
                    }
                    let desc = Term::Map(dm);
                    (prog, "genesis/vcs-why-v0.1", "vcs-why", desc)
                }
                VcsCmd::Diff {
                    base,
                    to,
                    out,
                    no_store,
                } => {
                    let f = env.get("core/cli::vcs-diff-program").ok_or_else(|| {
                        cli_err(
                            EX_INTERNAL,
                            "selfhost/missing",
                            "missing binding core/cli::vcs-diff-program",
                        )
                    })?;
                    let mut mm = std::collections::BTreeMap::new();
                    mm.insert(
                        TermOrdKey(Term::symbol(":base")),
                        Term::Str(base.to_string()),
                    );
                    mm.insert(TermOrdKey(Term::symbol(":to")), Term::Str(to.to_string()));
                    mm.insert(TermOrdKey(Term::symbol(":store")), Term::Bool(!*no_store));
                    if let Some(o) = out.as_deref() {
                        mm.insert(
                            TermOrdKey(Term::symbol(":out")),
                            Term::Str(o.display().to_string()),
                        );
                    }
                    let req = Term::Map(mm);
                    let prog = f.apply(&mut ctx, Value::Data(req)).map_err(|e| {
                        cli_err(
                            EX_EVAL,
                            "eval/error",
                            format!("core/cli vcs-diff-program failed: {e}"),
                        )
                    })?;
                    let mut dm = std::collections::BTreeMap::new();
                    dm.insert(
                        TermOrdKey(Term::symbol(":cmd")),
                        Term::Str("vcs/diff".to_string()),
                    );
                    dm.insert(
                        TermOrdKey(Term::symbol(":base")),
                        Term::Str(base.to_string()),
                    );
                    dm.insert(TermOrdKey(Term::symbol(":to")), Term::Str(to.to_string()));
                    dm.insert(TermOrdKey(Term::symbol(":store")), Term::Bool(!*no_store));
                    if let Some(o) = out.as_deref() {
                        dm.insert(
                            TermOrdKey(Term::symbol(":out")),
                            Term::Str(o.display().to_string()),
                        );
                    }
                    let desc = Term::Map(dm);
                    (prog, "genesis/vcs-diff-v0.1", "vcs-diff", desc)
                }
                VcsCmd::Apply {
                    base,
                    patch,
                    out,
                    no_store,
                } => {
                    let f = env.get("core/cli::vcs-apply-program").ok_or_else(|| {
                        cli_err(
                            EX_INTERNAL,
                            "selfhost/missing",
                            "missing binding core/cli::vcs-apply-program",
                        )
                    })?;
                    let mut mm = std::collections::BTreeMap::new();
                    mm.insert(
                        TermOrdKey(Term::symbol(":base")),
                        Term::Str(base.to_string()),
                    );
                    mm.insert(
                        TermOrdKey(Term::symbol(":patch")),
                        Term::Str(patch.to_string()),
                    );
                    mm.insert(TermOrdKey(Term::symbol(":store")), Term::Bool(!*no_store));
                    if let Some(o) = out.as_deref() {
                        mm.insert(
                            TermOrdKey(Term::symbol(":out")),
                            Term::Str(o.display().to_string()),
                        );
                    }
                    let req = Term::Map(mm);
                    let prog = f.apply(&mut ctx, Value::Data(req)).map_err(|e| {
                        cli_err(
                            EX_EVAL,
                            "eval/error",
                            format!("core/cli vcs-apply-program failed: {e}"),
                        )
                    })?;
                    let mut dm = std::collections::BTreeMap::new();
                    dm.insert(
                        TermOrdKey(Term::symbol(":cmd")),
                        Term::Str("vcs/apply".to_string()),
                    );
                    dm.insert(
                        TermOrdKey(Term::symbol(":base")),
                        Term::Str(base.to_string()),
                    );
                    dm.insert(
                        TermOrdKey(Term::symbol(":patch")),
                        Term::Str(patch.to_string()),
                    );
                    dm.insert(TermOrdKey(Term::symbol(":store")), Term::Bool(!*no_store));
                    if let Some(o) = out.as_deref() {
                        dm.insert(
                            TermOrdKey(Term::symbol(":out")),
                            Term::Str(o.display().to_string()),
                        );
                    }
                    let desc = Term::Map(dm);
                    (prog, "genesis/vcs-apply-v0.1", "vcs-apply", desc)
                }
                VcsCmd::Merge3 {
                    base,
                    left,
                    right,
                    out,
                } => {
                    let f = env.get("core/cli::vcs-merge3-program").ok_or_else(|| {
                        cli_err(
                            EX_INTERNAL,
                            "selfhost/missing",
                            "missing binding core/cli::vcs-merge3-program",
                        )
                    })?;
                    let mut mm = std::collections::BTreeMap::new();
                    mm.insert(
                        TermOrdKey(Term::symbol(":base")),
                        Term::Str(base.to_string()),
                    );
                    mm.insert(
                        TermOrdKey(Term::symbol(":left")),
                        Term::Str(left.to_string()),
                    );
                    mm.insert(
                        TermOrdKey(Term::symbol(":right")),
                        Term::Str(right.to_string()),
                    );
                    if let Some(o) = out.as_deref() {
                        mm.insert(
                            TermOrdKey(Term::symbol(":out")),
                            Term::Str(o.display().to_string()),
                        );
                    }
                    let req = Term::Map(mm);
                    let prog = f.apply(&mut ctx, Value::Data(req)).map_err(|e| {
                        cli_err(
                            EX_EVAL,
                            "eval/error",
                            format!("core/cli vcs-merge3-program failed: {e}"),
                        )
                    })?;
                    let mut dm = std::collections::BTreeMap::new();
                    dm.insert(
                        TermOrdKey(Term::symbol(":cmd")),
                        Term::Str("vcs/merge3".to_string()),
                    );
                    dm.insert(
                        TermOrdKey(Term::symbol(":base")),
                        Term::Str(base.to_string()),
                    );
                    dm.insert(
                        TermOrdKey(Term::symbol(":left")),
                        Term::Str(left.to_string()),
                    );
                    dm.insert(
                        TermOrdKey(Term::symbol(":right")),
                        Term::Str(right.to_string()),
                    );
                    if let Some(o) = out.as_deref() {
                        dm.insert(
                            TermOrdKey(Term::symbol(":out")),
                            Term::Str(o.display().to_string()),
                        );
                    }
                    let desc = Term::Map(dm);
                    (prog, "genesis/vcs-merge3-v0.1", "vcs-merge3", desc)
                }
                VcsCmd::ResolveConflict {
                    conflict,
                    strategy,
                    picks,
                    sets,
                    out,
                } => {
                    if strategy.is_none() && picks.is_empty() && sets.is_empty() {
                        return Err(cli_err(
                            EX_PARSE,
                            "vcs/resolve-conflict",
                            "must provide --strategy and/or --pick/--set overrides",
                        ));
                    }

                    let f = env
                        .get("core/cli::vcs-resolve-conflict-program")
                        .ok_or_else(|| {
                            cli_err(
                                EX_INTERNAL,
                                "selfhost/missing",
                                "missing binding core/cli::vcs-resolve-conflict-program",
                            )
                        })?;

                    let mut payload: std::collections::BTreeMap<TermOrdKey, Term> =
                        std::collections::BTreeMap::new();
                    payload.insert(
                        TermOrdKey(Term::symbol(":conflict")),
                        Term::Str(conflict.to_string()),
                    );
                    if let Some(s) = strategy.as_deref() {
                        let s = s.trim();
                        let sym = match s {
                            "left" | ":left" => ":left",
                            "right" | ":right" => ":right",
                            "base" | ":base" => ":base",
                            other => {
                                return Err(cli_err(
                                    EX_PARSE,
                                    "vcs/resolve-conflict",
                                    format!(
                                        "unsupported --strategy {other} (expected left|right|base)"
                                    ),
                                ));
                            }
                        };
                        payload.insert(
                            TermOrdKey(Term::symbol(":strategy")),
                            Term::Str(sym.to_string()),
                        );
                    }
                    if let Some(out) = out.as_deref() {
                        payload.insert(
                            TermOrdKey(Term::symbol(":out")),
                            Term::Str(out.display().to_string()),
                        );
                    }

                    let mut res: std::collections::BTreeMap<String, Term> =
                        std::collections::BTreeMap::new();
                    for p in picks {
                        let (opk, side) = p.split_once('=').ok_or_else(|| {
                            cli_err(
                                EX_PARSE,
                                "vcs/resolve-conflict",
                                format!("bad --pick {p}; expected op=left|right|base"),
                            )
                        })?;
                        let opk = opk.trim();
                        if opk.is_empty() {
                            return Err(cli_err(
                                EX_PARSE,
                                "vcs/resolve-conflict",
                                "bad --pick: empty op",
                            ));
                        }
                        if res.contains_key(opk) {
                            return Err(cli_err(
                                EX_PARSE,
                                "vcs/resolve-conflict",
                                format!("duplicate resolution for op {opk}"),
                            ));
                        }
                        let side = side.trim();
                        let sym = match side {
                            "left" | ":left" => ":left",
                            "right" | ":right" => ":right",
                            "base" | ":base" => ":base",
                            other => {
                                return Err(cli_err(
                                    EX_PARSE,
                                    "vcs/resolve-conflict",
                                    format!("bad --pick {p}; unsupported side {other}"),
                                ));
                            }
                        };
                        res.insert(opk.to_string(), Term::Str(sym.to_string()));
                    }
                    for s in sets {
                        let (opk, hv) = s.split_once('=').ok_or_else(|| {
                            cli_err(
                                EX_PARSE,
                                "vcs/resolve-conflict",
                                format!("bad --set {s}; expected op=<64-hex>"),
                            )
                        })?;
                        let opk = opk.trim();
                        if opk.is_empty() {
                            return Err(cli_err(
                                EX_PARSE,
                                "vcs/resolve-conflict",
                                "bad --set: empty op",
                            ));
                        }
                        if res.contains_key(opk) {
                            return Err(cli_err(
                                EX_PARSE,
                                "vcs/resolve-conflict",
                                format!("duplicate resolution for op {opk}"),
                            ));
                        }
                        let hv = hv.trim();
                        gc_vcs::validate_hex_hash(hv).map_err(|e| {
                            cli_err(EX_PARSE, "vcs/resolve-conflict", e.to_string())
                        })?;
                        res.insert(opk.to_string(), Term::Str(hv.to_string()));
                    }
                    if !res.is_empty() {
                        let mut rm: std::collections::BTreeMap<TermOrdKey, Term> =
                            std::collections::BTreeMap::new();
                        for (k, v) in res {
                            rm.insert(TermOrdKey(Term::Symbol(k)), v);
                        }
                        payload.insert(TermOrdKey(Term::symbol(":resolutions")), Term::Map(rm));
                    }

                    let req = Term::Map(payload.clone());
                    let prog = f.apply(&mut ctx, Value::Data(req)).map_err(|e| {
                        cli_err(
                            EX_EVAL,
                            "eval/error",
                            format!("core/cli vcs-resolve-conflict-program failed: {e}"),
                        )
                    })?;

                    let mut dm: std::collections::BTreeMap<TermOrdKey, Term> =
                        std::collections::BTreeMap::new();
                    dm.insert(
                        TermOrdKey(Term::symbol(":cmd")),
                        Term::Str("vcs/resolve-conflict".to_string()),
                    );
                    dm.insert(
                        TermOrdKey(Term::symbol(":conflict")),
                        Term::Str(conflict.to_string()),
                    );
                    if let Some(s) = strategy.as_deref() {
                        dm.insert(
                            TermOrdKey(Term::symbol(":strategy")),
                            Term::Str(s.to_string()),
                        );
                    }
                    dm.insert(
                        TermOrdKey(Term::symbol(":picks-len")),
                        Term::Int((picks.len() as i64).into()),
                    );
                    dm.insert(
                        TermOrdKey(Term::symbol(":sets-len")),
                        Term::Int((sets.len() as i64).into()),
                    );
                    if let Some(out) = out.as_deref() {
                        dm.insert(
                            TermOrdKey(Term::symbol(":out")),
                            Term::Str(out.display().to_string()),
                        );
                    }
                    if let Some(Term::Map(rm)) =
                        payload.get(&TermOrdKey(Term::symbol(":resolutions")))
                    {
                        dm.insert(
                            TermOrdKey(Term::symbol(":resolutions")),
                            Term::Map(rm.clone()),
                        );
                    }
                    let desc = Term::Map(dm);
                    (
                        prog,
                        "genesis/vcs-resolve-conflict-v0.1",
                        "vcs-resolve-conflict",
                        desc,
                    )
                }
            };
            let program_hash = gc_coreform::hash_term(&desc);
            Ok((prog, kind, log_op, program_hash))
        }
    }?;

    let toolchain = format!("genesis {}", env!("CARGO_PKG_VERSION"));
    let r = gc_effects::run(&mut ctx, &policy, prog, program_hash, toolchain)
        .map_err(|e| cli_err(EX_EVAL, "effects/run", format!("{e}")))?;

    let log_path = log
        .map(PathBuf::from)
        .unwrap_or_else(|| default_log_path(log_op));
    std::fs::write(&log_path, r.log.to_string_canonical() + "\n")
        .with_context(|| format!("write {}", log_path.display()))
        .map_err(|e| cli_err(EX_IO, "io/write", format!("{e}")))?;

    let mut ok = true;
    let mut exit_code = EX_OK;
    if let Some(proto) = ctx.protocol
        && let Value::Sealed { token, payload } = &r.value
        && *token == proto.error
    {
        ok = false;
        exit_code = EX_EVAL;
        if let Value::Data(Term::Map(m)) = payload.as_ref()
            && matches!(
                m.get(&gc_coreform::TermOrdKey(Term::symbol(":error/code"))),
                Some(Term::Str(s)) if s == "core/caps/denied"
            )
        {
            exit_code = EX_CAPS_DENIED;
        }
    }

    let (value, value_format) = render_value_for_cli(&ctx, &r.value);

    let map_is_conflict = |m: &std::collections::BTreeMap<TermOrdKey, Term>| {
        let ok_false = matches!(
            m.get(&gc_coreform::TermOrdKey(Term::symbol(":ok")))
                .or_else(|| m.get(&gc_coreform::TermOrdKey(Term::Str(":ok".to_string())))),
            Some(Term::Bool(false))
        );
        let has_conflict = m.contains_key(&gc_coreform::TermOrdKey(Term::symbol(":conflict")))
            || m.contains_key(&gc_coreform::TermOrdKey(Term::Str(":conflict".to_string())));
        ok_false && has_conflict
    };
    let value_is_conflict = match &r.value {
        Value::Data(Term::Map(m)) => map_is_conflict(m),
        Value::Data(Term::Str(s)) => match gc_coreform::parse_term(s) {
            Ok(Term::Map(m)) => map_is_conflict(&m),
            _ => false,
        },
        _ => false,
    } || match gc_coreform::parse_term(&value) {
        Ok(Term::Map(m)) => map_is_conflict(&m),
        _ => false,
    };
    // Detect conflict artifact and use stable exit semantics for merge.
    if matches!(cmd, VcsCmd::Merge3 { .. } | VcsCmd::ResolveConflict { .. }) && value_is_conflict {
        ok = false;
        exit_code = 3; // conflict
    }

    let stdout = if cli.json {
        String::new()
    } else {
        match cmd {
            VcsCmd::Diff { .. } => extract_vcs_patch_hash(&r.value)
                .map(|h| format!("{h}\n"))
                .unwrap_or_else(|| format!("{value}\n")),
            VcsCmd::Apply { .. } => extract_vcs_snapshot_hash(&r.value)
                .map(|h| format!("{h}\n"))
                .unwrap_or_else(|| format!("{value}\n")),
            VcsCmd::Blame { .. } => extract_vcs_commit_hash(&r.value)
                .map(|h| format!("{h}\n"))
                .unwrap_or_else(|| format!("{value}\n")),
            _ => format!("{value}\n"),
        }
    };

    let env = JsonEnvelope {
        ok,
        kind,
        data: Some(serde_json::json!({
            "coreform_frontend": frontend_info,
            "caps": caps.display().to_string(),
            "log": log_path.display().to_string(),
            "value": value,
            "value_format": value_format,
        })),
        error: if ok {
            None
        } else {
            Some(JsonError {
                code: "vcs/error",
                message: "vcs operation failed".to_string(),
                context: None,
            })
        },
    };

    Ok(CmdOut {
        exit_code,
        stdout,
        json: serde_json::to_value(env).expect("json"),
    })
}

pub(crate) fn mk_store_put_program(artifact: &Term) -> Vec<Term> {
    // (def prog (core/effect::perform 'core/store::put {:artifact (quote <artifact>)} (fn (r) (core/effect::pure r)))) prog
    let op = Term::list(vec![Term::symbol("quote"), Term::symbol("core/store::put")]);
    let payload = Term::Map(
        [(
            gc_coreform::TermOrdKey(Term::symbol(":artifact")),
            Term::list(vec![Term::symbol("quote"), artifact.clone()]),
        )]
        .into_iter()
        .collect(),
    );
    let k = Term::list(vec![
        Term::symbol("fn"),
        Term::list(vec![Term::symbol("r")]),
        Term::list(vec![Term::symbol("core/effect::pure"), Term::symbol("r")]),
    ]);
    let perform = Term::list(vec![Term::symbol("core/effect::perform"), op, payload, k]);
    vec![
        Term::list(vec![Term::symbol("def"), Term::symbol("prog"), perform]),
        Term::symbol("prog"),
    ]
}

pub(crate) fn mk_store_get_program(hash: &str) -> Vec<Term> {
    let op = Term::list(vec![Term::symbol("quote"), Term::symbol("core/store::get")]);
    let payload = Term::Map(
        [(
            gc_coreform::TermOrdKey(Term::symbol(":hash")),
            Term::Str(hash.to_string()),
        )]
        .into_iter()
        .collect(),
    );
    let k = Term::list(vec![
        Term::symbol("fn"),
        Term::list(vec![Term::symbol("r")]),
        Term::list(vec![Term::symbol("core/effect::pure"), Term::symbol("r")]),
    ]);
    let perform = Term::list(vec![Term::symbol("core/effect::perform"), op, payload, k]);
    vec![
        Term::list(vec![Term::symbol("def"), Term::symbol("prog"), perform]),
        Term::symbol("prog"),
    ]
}

pub(crate) fn mk_store_has_program(hash: &str) -> Vec<Term> {
    let op = Term::list(vec![Term::symbol("quote"), Term::symbol("core/store::has")]);
    let payload = Term::Map(
        [(
            gc_coreform::TermOrdKey(Term::symbol(":hash")),
            Term::Str(hash.to_string()),
        )]
        .into_iter()
        .collect(),
    );
    let k = Term::list(vec![
        Term::symbol("fn"),
        Term::list(vec![Term::symbol("r")]),
        Term::list(vec![Term::symbol("core/effect::pure"), Term::symbol("r")]),
    ]);
    let perform = Term::list(vec![Term::symbol("core/effect::perform"), op, payload, k]);
    vec![
        Term::list(vec![Term::symbol("def"), Term::symbol("prog"), perform]),
        Term::symbol("prog"),
    ]
}

pub(crate) fn extract_store_put_hash(v: &Value) -> Option<String> {
    let t = v.to_term_for_log(None);
    let Term::Map(m) = t else { return None };
    match m.get(&gc_coreform::TermOrdKey(Term::symbol(":hash"))) {
        Some(Term::Str(s)) => Some(s.clone()),
        _ => None,
    }
}

pub(crate) fn extract_store_has_present(v: &Value) -> Option<bool> {
    let t = v.to_term_for_log(None);
    let Term::Map(m) = t else { return None };
    match m.get(&gc_coreform::TermOrdKey(Term::symbol(":present"))) {
        Some(Term::Bool(b)) => Some(*b),
        _ => None,
    }
}

pub(crate) fn extract_store_get_artifact(v: &Value) -> Option<Term> {
    let t = v.to_term_for_log(None);
    let Term::Map(m) = t else { return None };
    m.get(&gc_coreform::TermOrdKey(Term::symbol(":artifact")))
        .cloned()
}

pub(crate) fn extract_refs_get_hash(v: &Value) -> Option<String> {
    let t = v.to_term_for_log(None);
    let Term::Map(m) = t else { return None };
    match m.get(&gc_coreform::TermOrdKey(Term::symbol(":hash"))) {
        Some(Term::Str(s)) => Some(s.clone()),
        Some(Term::Nil) => Some("nil".to_string()),
        _ => None,
    }
}

pub(crate) fn extract_refs_set_hash(v: &Value) -> Option<String> {
    let t = v.to_term_for_log(None);
    let Term::Map(m) = t else { return None };
    match m.get(&gc_coreform::TermOrdKey(Term::symbol(":hash"))) {
        Some(Term::Str(s)) => Some(s.clone()),
        Some(Term::Nil) => Some("nil".to_string()),
        _ => None,
    }
}

pub(crate) fn extract_refs_list_pairs(v: &Value) -> Option<Vec<(String, String)>> {
    let t = v.to_term_for_log(None);
    let Term::Map(m) = t else { return None };
    let Term::Vector(xs) = m.get(&gc_coreform::TermOrdKey(Term::symbol(":refs")))? else {
        return None;
    };
    let mut out = Vec::new();
    for x in xs {
        let Term::Map(em) = x else { return None };
        let name = match em.get(&gc_coreform::TermOrdKey(Term::symbol(":name"))) {
            Some(Term::Str(s)) => s.clone(),
            _ => return None,
        };
        let hash = match em.get(&gc_coreform::TermOrdKey(Term::symbol(":hash"))) {
            Some(Term::Str(s)) => s.clone(),
            Some(Term::Nil) => "nil".to_string(),
            _ => return None,
        };
        out.push((name, hash));
    }
    Some(out)
}

pub(crate) fn parse_pkg_spec(spec: &str) -> Result<(String, String), String> {
    let (name, sel) = spec
        .split_once('@')
        .ok_or_else(|| "spec must be <name>@<selector>".to_string())?;
    let name = name.trim();
    let sel = sel.trim();
    if name.is_empty() || sel.is_empty() {
        return Err("spec must be <name>@<selector> (both non-empty)".to_string());
    }
    Ok((name.to_string(), sel.to_string()))
}

pub(crate) fn normalize_pkg_add_strategy(
    selector: &str,
    strategy: Option<&str>,
    tag_policy: Option<&str>,
) -> Result<(Option<String>, Option<String>), CliError> {
    let strategy = match strategy {
        Some(raw) => gc_pkg::ResolutionStrategy::from_str(raw).ok_or_else(|| {
            cli_err(
                EX_PARSE,
                "pkg/spec",
                format!("invalid --strategy `{raw}` (expected pinned|track-ref|tag-policy)"),
            )
        })?,
        None => gc_pkg::infer_strategy(selector),
    };

    if matches!(strategy, gc_pkg::ResolutionStrategy::TagPolicy)
        && !matches!(
            gc_pkg::classify_selector(selector),
            Some(gc_pkg::SelectorKind::TagRef)
        )
    {
        return Err(cli_err(
            EX_PARSE,
            "pkg/spec",
            "tag-policy strategy requires selector under refs/tags/*".to_string(),
        ));
    }
    if !matches!(strategy, gc_pkg::ResolutionStrategy::TagPolicy) && tag_policy.is_some() {
        return Err(cli_err(
            EX_PARSE,
            "pkg/spec",
            "--tag-policy can only be used with --strategy tag-policy".to_string(),
        ));
    }

    let strategy_s = Some(strategy.as_str().to_string());
    let tag_policy_s = if matches!(strategy, gc_pkg::ResolutionStrategy::TagPolicy) {
        Some(tag_policy.unwrap_or("exact").to_string())
    } else {
        None
    };
    Ok((strategy_s, tag_policy_s))
}

#[derive(Debug, Clone)]
pub(crate) struct SetRefSpec {
    pub(crate) name: String,
    pub(crate) hash: String,
    pub(crate) policy: String,
    pub(crate) expected_old: Option<String>,
}

pub(crate) fn parse_set_ref_spec(spec: &str) -> Result<SetRefSpec, CliError> {
    let (base, expected_old_raw) = match spec.split_once('@') {
        None => (spec, None),
        Some((lhs, rhs)) => (lhs, Some(rhs)),
    };

    let mut it = base.rsplitn(3, ':');
    let Some(policy) = it.next() else {
        return Err(cli_err(
            EX_PARSE,
            "sync/set-ref",
            "set-ref must be <refname>:<commit-hash>:<policy-hash>[@<expected-old-hash|nil>]"
                .to_string(),
        ));
    };
    let Some(hash) = it.next() else {
        return Err(cli_err(
            EX_PARSE,
            "sync/set-ref",
            "set-ref must be <refname>:<commit-hash>:<policy-hash>[@<expected-old-hash|nil>]"
                .to_string(),
        ));
    };
    let Some(name) = it.next() else {
        return Err(cli_err(
            EX_PARSE,
            "sync/set-ref",
            "set-ref must be <refname>:<commit-hash>:<policy-hash>[@<expected-old-hash|nil>]"
                .to_string(),
        ));
    };
    let name = name.trim();
    let hash = hash.trim();
    let policy = policy.trim();

    if name.is_empty() || hash.is_empty() || policy.is_empty() {
        return Err(cli_err(
            EX_PARSE,
            "sync/set-ref",
            "set-ref fields must be non-empty".to_string(),
        ));
    }
    if !is_hex64(hash) {
        return Err(cli_err(
            EX_PARSE,
            "sync/set-ref",
            "set-ref commit hash must be 64-hex".to_string(),
        ));
    }
    if !is_hex64(policy) {
        return Err(cli_err(
            EX_PARSE,
            "sync/set-ref",
            "set-ref policy hash must be 64-hex".to_string(),
        ));
    }
    let expected_old = match expected_old_raw.map(str::trim) {
        None => None,
        Some("") => {
            return Err(cli_err(
                EX_PARSE,
                "sync/set-ref",
                "set-ref expected-old must be non-empty when provided".to_string(),
            ));
        }
        Some(s) => {
            if s != "nil" && !is_hex64(s) {
                return Err(cli_err(
                    EX_PARSE,
                    "sync/set-ref",
                    "set-ref expected-old must be 64-hex or `nil`".to_string(),
                ));
            }
            Some(if s == "nil" {
                "nil".to_string()
            } else {
                s.to_ascii_lowercase()
            })
        }
    };

    Ok(SetRefSpec {
        name: name.to_string(),
        hash: hash.to_ascii_lowercase(),
        policy: policy.to_ascii_lowercase(),
        expected_old,
    })
}

pub(crate) fn parse_sync_set_refs(specs: &[String]) -> Result<Vec<SetRefSpec>, CliError> {
    let mut seen: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
    let mut out = Vec::with_capacity(specs.len());
    for spec in specs {
        let parsed = parse_set_ref_spec(spec)?;
        if !seen.insert(parsed.name.clone()) {
            return Err(cli_err(
                EX_PARSE,
                "sync/set-ref",
                format!("duplicate set-ref target: {}", parsed.name),
            ));
        }
        out.push(parsed);
    }
    Ok(out)
}

pub(crate) fn is_hex64(s: &str) -> bool {
    if s.len() != 64 {
        return false;
    }
    s.as_bytes().iter().all(|b| b.is_ascii_hexdigit())
}

pub(crate) fn parse_local_set_refs(
    specs: &[String],
    policy: Option<&str>,
) -> Result<Vec<SetRefSpec>, CliError> {
    if specs.is_empty() {
        return Ok(Vec::new());
    }
    let Some(pol) = policy else {
        return Err(cli_err(
            EX_PARSE,
            "pkg/import",
            "--set-ref requires --policy <policy-hash>".to_string(),
        ));
    };
    if !is_hex64(pol) {
        return Err(cli_err(
            EX_PARSE,
            "pkg/import",
            "--policy must be 64-hex".to_string(),
        ));
    }

    let mut seen: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
    let mut out = Vec::new();
    for s in specs {
        let (name, rhs) = s.split_once('=').ok_or_else(|| {
            cli_err(
                EX_PARSE,
                "pkg/import",
                "set-ref must be <refname>=<commit-hash|nil>[@<expected-old-hash|nil>]".to_string(),
            )
        })?;
        let name = name.trim();
        let rhs = rhs.trim();
        if name.is_empty() || rhs.is_empty() {
            return Err(cli_err(
                EX_PARSE,
                "pkg/import",
                "set-ref fields must be non-empty".to_string(),
            ));
        }
        if !seen.insert(name.to_string()) {
            return Err(cli_err(
                EX_PARSE,
                "pkg/import",
                format!("duplicate set-ref target: {name}"),
            ));
        }
        let (hash, expected_old) = match rhs.split_once('@') {
            None => (rhs, None),
            Some((h, eo)) => {
                let eo = eo.trim();
                if eo.is_empty() {
                    return Err(cli_err(
                        EX_PARSE,
                        "pkg/import",
                        "set-ref expected-old must be non-empty when @ is used".to_string(),
                    ));
                }
                (h.trim(), Some(eo))
            }
        };
        if hash != "nil" && !is_hex64(hash) {
            return Err(cli_err(
                EX_PARSE,
                "pkg/import",
                "set-ref hash must be 64-hex or `nil`".to_string(),
            ));
        }
        let expected_old = match expected_old {
            None => None,
            Some(eo) => {
                if eo != "nil" && !is_hex64(eo) {
                    return Err(cli_err(
                        EX_PARSE,
                        "pkg/import",
                        "set-ref expected-old must be 64-hex or `nil`".to_string(),
                    ));
                }
                Some(eo.to_string())
            }
        };
        out.push(SetRefSpec {
            name: name.to_string(),
            hash: hash.to_string(),
            policy: pol.to_string(),
            expected_old,
        });
    }
    Ok(out)
}

pub(crate) fn extract_vcs_patch_hash(v: &Value) -> Option<String> {
    let t = v.to_term_for_log(None);
    let Term::Map(m) = t else { return None };
    match m.get(&gc_coreform::TermOrdKey(Term::symbol(":patch"))) {
        Some(Term::Str(s)) => Some(s.clone()),
        _ => None,
    }
}

pub(crate) fn extract_vcs_snapshot_hash(v: &Value) -> Option<String> {
    let t = v.to_term_for_log(None);
    let Term::Map(m) = t else { return None };
    match m.get(&gc_coreform::TermOrdKey(Term::symbol(":snapshot"))) {
        Some(Term::Str(s)) => Some(s.clone()),
        _ => None,
    }
}

pub(crate) fn extract_vcs_commit_hash(v: &Value) -> Option<String> {
    let t = v.to_term_for_log(None);
    let Term::Map(m) = t else { return None };
    match m.get(&gc_coreform::TermOrdKey(Term::symbol(":commit"))) {
        Some(Term::Str(s)) => Some(s.clone()),
        _ => None,
    }
}

pub(crate) fn extract_pkg_snapshot_hash(v: &Value) -> Option<String> {
    let t = v.to_term_for_log(None);
    let Term::Map(m) = t else { return None };
    match m.get(&gc_coreform::TermOrdKey(Term::symbol(":snapshot"))) {
        Some(Term::Str(s)) => Some(s.clone()),
        _ => None,
    }
}

pub(crate) fn extract_pkg_export_bundle_hash(v: &Value) -> Option<String> {
    let t = v.to_term_for_log(None);
    let Term::Map(m) = t else { return None };
    match m.get(&gc_coreform::TermOrdKey(Term::symbol(":bundle-h"))) {
        Some(Term::Str(s)) => Some(s.clone()),
        _ => None,
    }
}

pub(crate) fn extract_pkg_import_root(v: &Value) -> Option<String> {
    let t = v.to_term_for_log(None);
    let Term::Map(m) = t else { return None };
    match m.get(&gc_coreform::TermOrdKey(Term::symbol(":root"))) {
        Some(Term::Str(s)) => Some(s.clone()),
        _ => None,
    }
}

pub(crate) fn extract_pkg_publish_commit(v: &Value) -> Option<String> {
    let t = v.to_term_for_log(None);
    let Term::Map(m) = t else { return None };
    match m.get(&gc_coreform::TermOrdKey(Term::symbol(":commit"))) {
        Some(Term::Str(s)) => Some(s.clone()),
        _ => None,
    }
}

pub(crate) fn extract_pkg_lock_hash(v: &Value) -> Option<String> {
    let t = v.to_term_for_log(None);
    let Term::Map(m) = t else { return None };
    match m.get(&gc_coreform::TermOrdKey(Term::symbol(":lock-h"))) {
        Some(Term::Str(s)) => Some(s.clone()),
        _ => None,
    }
}

pub(crate) fn extract_pkg_ok_bool(v: &Value) -> Option<bool> {
    let t = v.to_term_for_log(None);
    let Term::Map(m) = t else { return None };
    match m.get(&gc_coreform::TermOrdKey(Term::symbol(":ok"))) {
        Some(Term::Bool(b)) => Some(*b),
        _ => None,
    }
}
