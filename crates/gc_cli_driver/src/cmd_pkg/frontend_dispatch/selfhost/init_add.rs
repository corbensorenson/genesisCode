use super::*;
use gc_kernel::Env;

type ProgramBuild = (Value, &'static str, &'static str, Term);

pub(super) struct PkgAddProgramRequest<'a> {
    pub(super) spec: &'a str,
    pub(super) lock: &'a std::path::Path,
    pub(super) update_policy: &'a str,
    pub(super) registry: &'a Option<String>,
    pub(super) strategy: &'a Option<String>,
    pub(super) tag_policy: &'a Option<String>,
}

pub(super) fn build_pkg_init(
    ctx: &mut EvalCtx,
    env: &mut Env,
    workspace: &str,
    lock: &std::path::Path,
    policy: &str,
    registry_default: &Option<String>,
) -> Result<ProgramBuild, CliError> {
    let f = env.get("core/cli::pkg-init-program").ok_or_else(|| {
        cli_err(
            EX_INTERNAL,
            "selfhost/missing",
            "missing binding core/cli::pkg-init-program",
        )
    })?;
    let mut mm = std::collections::BTreeMap::new();
    mm.insert(
        TermOrdKey(Term::symbol(":workspace")),
        Term::Str(workspace.to_string()),
    );
    mm.insert(
        TermOrdKey(Term::symbol(":lock")),
        Term::Str(lock.display().to_string()),
    );
    mm.insert(
        TermOrdKey(Term::symbol(":policy")),
        Term::Str(policy.to_string()),
    );
    mm.insert(
        TermOrdKey(Term::symbol(":registry-default")),
        registry_default
            .as_deref()
            .map(|s| Term::Str(s.to_string()))
            .unwrap_or(Term::Nil),
    );
    let req = Term::Map(mm);
    let prog = f.apply(ctx, Value::Data(req)).map_err(|e| {
        cli_err(
            EX_EVAL,
            "eval/error",
            format!("core/cli pkg-init-program failed: {e}"),
        )
    })?;
    let desc = Term::Map(
        [
            (
                TermOrdKey(Term::symbol(":cmd")),
                Term::Str("pkg/init".to_string()),
            ),
            (
                TermOrdKey(Term::symbol(":workspace")),
                Term::Str(workspace.to_string()),
            ),
            (
                TermOrdKey(Term::symbol(":lock")),
                Term::Str(lock.display().to_string()),
            ),
            (
                TermOrdKey(Term::symbol(":policy")),
                Term::Str(policy.to_string()),
            ),
        ]
        .into_iter()
        .collect(),
    );
    Ok((prog, "genesis/pkg-init-v0.1", "pkg-init", desc))
}

pub(super) fn build_pkg_add(
    ctx: &mut EvalCtx,
    env: &mut Env,
    req: &PkgAddProgramRequest<'_>,
) -> Result<ProgramBuild, CliError> {
    let spec = req.spec;
    let lock = req.lock;
    let update_policy = req.update_policy;
    let registry = req.registry;
    let strategy = req.strategy;
    let tag_policy = req.tag_policy;
    let (name, selector) =
        parse_pkg_spec(spec).map_err(|e| cli_err(EX_PARSE, "pkg/spec", e.to_string()))?;
    let (strategy_norm, tag_policy_norm) =
        normalize_pkg_add_strategy(&selector, strategy.as_deref(), tag_policy.as_deref())?;
    let f = env.get("core/cli::pkg-add-program").ok_or_else(|| {
        cli_err(
            EX_INTERNAL,
            "selfhost/missing",
            "missing binding core/cli::pkg-add-program",
        )
    })?;
    let mut mm = std::collections::BTreeMap::new();
    mm.insert(
        TermOrdKey(Term::symbol(":lock")),
        Term::Str(lock.display().to_string()),
    );
    mm.insert(TermOrdKey(Term::symbol(":name")), Term::Str(name.clone()));
    mm.insert(
        TermOrdKey(Term::symbol(":selector")),
        Term::Str(selector.clone()),
    );
    mm.insert(
        TermOrdKey(Term::symbol(":update-policy")),
        Term::Str(update_policy.to_string()),
    );
    mm.insert(
        TermOrdKey(Term::symbol(":registry")),
        registry
            .as_deref()
            .map(|s| Term::Str(s.to_string()))
            .unwrap_or(Term::Nil),
    );
    mm.insert(
        TermOrdKey(Term::symbol(":strategy")),
        strategy_norm
            .as_deref()
            .map(|s| Term::Str(s.to_string()))
            .unwrap_or(Term::Nil),
    );
    mm.insert(
        TermOrdKey(Term::symbol(":tag-policy")),
        tag_policy_norm
            .as_deref()
            .map(|s| Term::Str(s.to_string()))
            .unwrap_or(Term::Nil),
    );
    let req = Term::Map(mm);
    let prog = f.apply(ctx, Value::Data(req)).map_err(|e| {
        cli_err(
            EX_EVAL,
            "eval/error",
            format!("core/cli pkg-add-program failed: {e}"),
        )
    })?;
    let desc = Term::Map(
        [
            (
                TermOrdKey(Term::symbol(":cmd")),
                Term::Str("pkg/add".to_string()),
            ),
            (
                TermOrdKey(Term::symbol(":lock")),
                Term::Str(lock.display().to_string()),
            ),
            (TermOrdKey(Term::symbol(":name")), Term::Str(name)),
            (TermOrdKey(Term::symbol(":selector")), Term::Str(selector)),
            (
                TermOrdKey(Term::symbol(":update-policy")),
                Term::Str(update_policy.to_string()),
            ),
            (
                TermOrdKey(Term::symbol(":strategy")),
                strategy_norm
                    .as_deref()
                    .map(|s| Term::Str(s.to_string()))
                    .unwrap_or(Term::Nil),
            ),
            (
                TermOrdKey(Term::symbol(":tag-policy")),
                tag_policy_norm
                    .as_deref()
                    .map(|s| Term::Str(s.to_string()))
                    .unwrap_or(Term::Nil),
            ),
        ]
        .into_iter()
        .collect(),
    );
    Ok((prog, "genesis/pkg-add-v0.1", "pkg-add", desc))
}
