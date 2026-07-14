use super::*;
use gc_kernel::Env;

pub(super) fn build_selfhost_vcs_program(
    cli: &Cli,
    cmd: &VcsCmd,
    ctx: &mut EvalCtx,
    env: &mut Env,
) -> Result<(Value, [u8; 32]), CliError> {
    load_selfhost_toolchain(cli, ctx, env)?;

    let (prog, desc) = match cmd {
        VcsCmd::Hash { .. } => {
            return Err(cli_err(
                EX_INTERNAL,
                "vcs/dispatch-drift",
                "vcs hash must be handled before effectful VCS dispatch",
            ));
        }
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
            let prog = f.apply(ctx, Value::data(req)).map_err(|e| {
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
            (prog, desc)
        }
        VcsCmd::Blame {
            snapshot,
            sym,
            path,
        } => {
            gc_vcs::validate_hex_hash(snapshot)
                .map_err(|e| cli_err(EX_PARSE, "vcs/blame", format!("invalid --snapshot: {e}")))?;
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
            let prog = f.apply(ctx, Value::data(req)).map_err(|e| {
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
            (prog, desc)
        }
        VcsCmd::Why { snapshot, sym, op } => {
            gc_vcs::validate_hex_hash(snapshot)
                .map_err(|e| cli_err(EX_PARSE, "vcs/why", format!("invalid --snapshot: {e}")))?;
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
            let prog = f.apply(ctx, Value::data(req)).map_err(|e| {
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
            (prog, desc)
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
            let prog = f.apply(ctx, Value::data(req)).map_err(|e| {
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
            (prog, desc)
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
            let prog = f.apply(ctx, Value::data(req)).map_err(|e| {
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
            (prog, desc)
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
            let prog = f.apply(ctx, Value::data(req)).map_err(|e| {
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
            (prog, desc)
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
                            format!("unsupported --strategy {other} (expected left|right|base)"),
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
                gc_vcs::validate_hex_hash(hv)
                    .map_err(|e| cli_err(EX_PARSE, "vcs/resolve-conflict", e.to_string()))?;
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
            let prog = f.apply(ctx, Value::data(req)).map_err(|e| {
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
            if let Some(Term::Map(rm)) = payload.get(&TermOrdKey(Term::symbol(":resolutions"))) {
                dm.insert(
                    TermOrdKey(Term::symbol(":resolutions")),
                    Term::Map(rm.clone()),
                );
            }
            let desc = Term::Map(dm);
            (prog, desc)
        }
    };
    let program_hash = gc_coreform::hash_term(&desc);
    Ok((prog, program_hash))
}
