use super::semantic_workspace_misc::refactor_kind_token;
use super::semantic_workspace_types::RefactorConflict;
use super::*;
use std::collections::BTreeMap;

fn term_map_get<'a>(m: &'a BTreeMap<TermOrdKey, Term>, key: &str) -> Option<&'a Term> {
    m.get(&TermOrdKey(Term::symbol(key)))
}

fn decode_refactor_conflicts(
    out_map: &BTreeMap<TermOrdKey, Term>,
    scope: &str,
) -> Result<Vec<RefactorConflict>, CliError> {
    let conflicts_term = term_map_get(out_map, ":conflicts").ok_or_else(|| {
        cli_err(
            EX_INTERNAL,
            "semantic-edit/refactor",
            format!("{scope} missing :conflicts"),
        )
    })?;
    let ok_flag = match term_map_get(out_map, ":ok") {
        Some(Term::Bool(v)) => *v,
        _ => {
            return Err(cli_err(
                EX_INTERNAL,
                "semantic-edit/refactor",
                format!("{scope} missing :ok bool"),
            ));
        }
    };
    let Term::Vector(conflict_entries) = conflicts_term else {
        return Err(cli_err(
            EX_INTERNAL,
            "semantic-edit/refactor",
            format!("{scope} :conflicts must be vector"),
        ));
    };
    let mut conflicts = Vec::with_capacity(conflict_entries.len());
    for entry in conflict_entries {
        let Term::Map(m) = entry else {
            return Err(cli_err(
                EX_INTERNAL,
                "semantic-edit/refactor",
                format!("{scope} conflict entry must be map"),
            ));
        };
        let code = match term_map_get(m, ":code") {
            Some(Term::Str(s)) => s.clone(),
            _ => {
                return Err(cli_err(
                    EX_INTERNAL,
                    "semantic-edit/refactor",
                    format!("{scope} conflict entry missing :code string"),
                ));
            }
        };
        let message = match term_map_get(m, ":message") {
            Some(Term::Str(s)) => s.clone(),
            _ => {
                return Err(cli_err(
                    EX_INTERNAL,
                    "semantic-edit/refactor",
                    format!("{scope} conflict entry missing :message string"),
                ));
            }
        };
        let module_path = match term_map_get(m, ":module-path") {
            Some(Term::Str(s)) => {
                if s.is_empty() {
                    None
                } else {
                    Some(s.clone())
                }
            }
            Some(_) => {
                return Err(cli_err(
                    EX_INTERNAL,
                    "semantic-edit/refactor",
                    format!("{scope} conflict entry :module-path must be string"),
                ));
            }
            None => None,
        };
        let path_repr = match term_map_get(m, ":path-repr") {
            Some(Term::Str(s)) => {
                if s.is_empty() {
                    None
                } else {
                    Some(s.clone())
                }
            }
            Some(_) => {
                return Err(cli_err(
                    EX_INTERNAL,
                    "semantic-edit/refactor",
                    format!("{scope} conflict entry :path-repr must be string"),
                ));
            }
            None => None,
        };
        conflicts.push(RefactorConflict {
            code,
            message,
            module_path,
            path_repr,
        });
    }
    if ok_flag != conflicts.is_empty() {
        return Err(cli_err(
            EX_INTERNAL,
            "semantic-edit/refactor",
            format!("{scope} :ok does not match :conflicts"),
        ));
    }
    Ok(conflicts)
}

pub(super) fn semantic_refactor_plan_conflicts_from_contract(
    cli: &Cli,
    from_symbol: &str,
    to_symbol: &str,
    from_def_modules: &[String],
    to_def_module: Option<&str>,
    to_def_path_repr: Option<&str>,
) -> Result<Vec<RefactorConflict>, CliError> {
    let mut ctx = EvalCtx::with_step_limit(resolved_step_limit(cli).resolve());
    ctx.set_mem_limits(resolved_mem_limits(cli));
    let prelude = build_prelude(&mut ctx);
    let env = prelude.env;
    let payload = Term::Map(
        [
            (
                TermOrdKey(Term::symbol(":from-symbol")),
                Term::Str(from_symbol.to_string()),
            ),
            (
                TermOrdKey(Term::symbol(":to-symbol")),
                Term::Str(to_symbol.to_string()),
            ),
            (
                TermOrdKey(Term::symbol(":from-def-modules")),
                Term::Vector(
                    from_def_modules
                        .iter()
                        .cloned()
                        .map(Term::Str)
                        .collect::<Vec<_>>(),
                ),
            ),
            (
                TermOrdKey(Term::symbol(":to-def-module")),
                Term::Str(to_def_module.unwrap_or_default().to_string()),
            ),
            (
                TermOrdKey(Term::symbol(":to-def-path-repr")),
                Term::Str(to_def_path_repr.unwrap_or_default().to_string()),
            ),
        ]
        .into_iter()
        .collect(),
    );
    let contract = env
        .get("core/cli::semantic-refactor-plan-conflicts")
        .ok_or_else(|| {
            cli_err(
                EX_INTERNAL,
                "semantic-edit/refactor",
                "missing binding core/cli::semantic-refactor-plan-conflicts".to_string(),
            )
        })?;
    let out = contract
        .apply(&mut ctx, Value::Data(payload))
        .map_err(|e| {
            cli_err(
                EX_EVAL,
                "semantic-edit/refactor",
                format!("core/cli::semantic-refactor-plan-conflicts failed: {e}"),
            )
        })?;
    let out_term = out.to_term_for_log(ctx.protocol.map(|p| p.error));
    let Term::Map(out_map) = out_term else {
        return Err(cli_err(
            EX_INTERNAL,
            "semantic-edit/refactor",
            "semantic-refactor-plan-conflicts returned non-map".to_string(),
        ));
    };
    decode_refactor_conflicts(&out_map, "semantic-refactor-plan-conflicts")
}

pub(super) fn semantic_refactor_target_conflicts_from_contract(
    cli: &Cli,
    kind: RefactorKind,
    target_module_path: Option<&str>,
    target_module_valid: bool,
    target_module_exists: bool,
) -> Result<Vec<RefactorConflict>, CliError> {
    let mut ctx = EvalCtx::with_step_limit(resolved_step_limit(cli).resolve());
    ctx.set_mem_limits(resolved_mem_limits(cli));
    let prelude = build_prelude(&mut ctx);
    let env = prelude.env;
    let payload = Term::Map(
        [
            (
                TermOrdKey(Term::symbol(":kind")),
                Term::Str(refactor_kind_token(kind).to_string()),
            ),
            (
                TermOrdKey(Term::symbol(":target-module-path")),
                Term::Str(target_module_path.unwrap_or_default().to_string()),
            ),
            (
                TermOrdKey(Term::symbol(":target-module-valid")),
                Term::Bool(target_module_valid),
            ),
            (
                TermOrdKey(Term::symbol(":target-module-exists")),
                Term::Bool(target_module_exists),
            ),
        ]
        .into_iter()
        .collect(),
    );
    let contract = env
        .get("core/cli::semantic-refactor-target-conflicts")
        .ok_or_else(|| {
            cli_err(
                EX_INTERNAL,
                "semantic-edit/refactor",
                "missing binding core/cli::semantic-refactor-target-conflicts".to_string(),
            )
        })?;
    let out = contract
        .apply(&mut ctx, Value::Data(payload))
        .map_err(|e| {
            cli_err(
                EX_EVAL,
                "semantic-edit/refactor",
                format!("core/cli::semantic-refactor-target-conflicts failed: {e}"),
            )
        })?;
    let out_term = out.to_term_for_log(ctx.protocol.map(|p| p.error));
    let Term::Map(out_map) = out_term else {
        return Err(cli_err(
            EX_INTERNAL,
            "semantic-edit/refactor",
            "semantic-refactor-target-conflicts returned non-map".to_string(),
        ));
    };
    decode_refactor_conflicts(&out_map, "semantic-refactor-target-conflicts")
}

pub(super) fn semantic_refactor_validate_from_contract(
    cli: &Cli,
    kind: RefactorKind,
    from_symbol: &str,
    to_symbol: &str,
) -> Result<Vec<RefactorConflict>, CliError> {
    let mut ctx = EvalCtx::with_step_limit(resolved_step_limit(cli).resolve());
    ctx.set_mem_limits(resolved_mem_limits(cli));
    let prelude = build_prelude(&mut ctx);
    let env = prelude.env;
    let payload = Term::Map(
        [
            (
                TermOrdKey(Term::symbol(":kind")),
                Term::Str(refactor_kind_token(kind).to_string()),
            ),
            (
                TermOrdKey(Term::symbol(":from-symbol")),
                Term::Str(from_symbol.to_string()),
            ),
            (
                TermOrdKey(Term::symbol(":to-symbol")),
                Term::Str(to_symbol.to_string()),
            ),
        ]
        .into_iter()
        .collect(),
    );
    let contract = env
        .get("core/cli::semantic-refactor-validate")
        .ok_or_else(|| {
            cli_err(
                EX_INTERNAL,
                "semantic-edit/refactor",
                "missing binding core/cli::semantic-refactor-validate".to_string(),
            )
        })?;
    let out = contract
        .apply(&mut ctx, Value::Data(payload))
        .map_err(|e| {
            cli_err(
                EX_EVAL,
                "semantic-edit/refactor",
                format!("core/cli::semantic-refactor-validate failed: {e}"),
            )
        })?;
    let out_term = out.to_term_for_log(ctx.protocol.map(|p| p.error));
    let Term::Map(out_map) = out_term else {
        return Err(cli_err(
            EX_INTERNAL,
            "semantic-edit/refactor",
            "semantic-refactor-validate returned non-map".to_string(),
        ));
    };
    decode_refactor_conflicts(&out_map, "semantic-refactor-validate")
}
