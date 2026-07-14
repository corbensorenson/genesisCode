use std::collections::BTreeSet;
use std::sync::Arc;

use super::{CExpr, CompiledForm, CompiledModule, CoverageSiteManifest};
use crate::error::{KernelError, KernelErrorKind};

pub(super) fn compiled_module_coverage_manifest_from_compiled(
    compiled: &CompiledModule,
) -> CoverageSiteManifest {
    compiled.coverage_sites.manifest()
}

pub(super) fn collect_decision_conditions_and_validate(
    forms: &[CompiledForm],
    statement_site_count: usize,
    decision_site_count: usize,
) -> Result<Vec<BTreeSet<String>>, KernelError> {
    let mut decision_conditions = vec![BTreeSet::new(); decision_site_count];
    for form in forms {
        match form {
            CompiledForm::Def { expr, .. } | CompiledForm::Expr(expr) => {
                collect_decision_conditions_from_expr(
                    expr,
                    statement_site_count,
                    &mut decision_conditions,
                )?;
            }
        }
    }
    Ok(decision_conditions)
}

fn collect_decision_conditions_from_expr(
    expr: &Arc<CExpr>,
    statement_site_count: usize,
    decision_conditions: &mut [BTreeSet<String>],
) -> Result<(), KernelError> {
    match expr.as_ref() {
        CExpr::Atom(_) | CExpr::Vector(_) | CExpr::Quote(_) | CExpr::SealNew => {}
        CExpr::Var { statement_site, .. } => {
            let site_index = usize::try_from(*statement_site).map_err(|_| {
                KernelError::new(
                    KernelErrorKind::Internal,
                    "compiled coverage statement site index exceeds usize range",
                )
            })?;
            if site_index >= statement_site_count {
                return Err(KernelError::new(
                    KernelErrorKind::Internal,
                    format!("compiled coverage statement site index out of range: {site_index}"),
                ));
            }
        }
        CExpr::Map(entries) => {
            for (_, v) in entries {
                collect_decision_conditions_from_expr(
                    v,
                    statement_site_count,
                    decision_conditions,
                )?;
            }
        }
        CExpr::If {
            cond,
            then_expr,
            else_expr,
            decision_site,
        } => {
            let site_index = usize::try_from(*decision_site).map_err(|_| {
                KernelError::new(
                    KernelErrorKind::Internal,
                    "compiled coverage decision site index exceeds usize range",
                )
            })?;
            let Some(conditions) = decision_conditions.get_mut(site_index) else {
                return Err(KernelError::new(
                    KernelErrorKind::Internal,
                    format!("compiled coverage decision site index out of range: {site_index}"),
                ));
            };
            collect_condition_symbols_from_expr(cond, conditions);
            collect_decision_conditions_from_expr(cond, statement_site_count, decision_conditions)?;
            collect_decision_conditions_from_expr(
                then_expr,
                statement_site_count,
                decision_conditions,
            )?;
            collect_decision_conditions_from_expr(
                else_expr,
                statement_site_count,
                decision_conditions,
            )?;
        }
        CExpr::Begin(xs) => {
            for x in xs {
                collect_decision_conditions_from_expr(
                    x,
                    statement_site_count,
                    decision_conditions,
                )?;
            }
        }
        CExpr::Let(bindings, body) => {
            for (_, rhs) in bindings {
                collect_decision_conditions_from_expr(
                    rhs,
                    statement_site_count,
                    decision_conditions,
                )?;
            }
            collect_decision_conditions_from_expr(body, statement_site_count, decision_conditions)?;
        }
        CExpr::FnUnary { body, .. } => {
            collect_decision_conditions_from_expr(body, statement_site_count, decision_conditions)?;
        }
        CExpr::Prim { args, .. } | CExpr::PrimUnknown { args, .. } => {
            for a in args {
                collect_decision_conditions_from_expr(
                    a,
                    statement_site_count,
                    decision_conditions,
                )?;
            }
        }
        CExpr::Seal(v, tok) | CExpr::Unseal(v, tok) | CExpr::App(v, tok) => {
            collect_decision_conditions_from_expr(v, statement_site_count, decision_conditions)?;
            collect_decision_conditions_from_expr(tok, statement_site_count, decision_conditions)?;
        }
        CExpr::AppN { callee, args, .. } => {
            collect_decision_conditions_from_expr(
                callee,
                statement_site_count,
                decision_conditions,
            )?;
            for arg in args.iter() {
                collect_decision_conditions_from_expr(
                    arg,
                    statement_site_count,
                    decision_conditions,
                )?;
            }
        }
    }
    Ok(())
}

fn collect_condition_symbols_from_expr(expr: &Arc<CExpr>, out: &mut BTreeSet<String>) {
    match expr.as_ref() {
        CExpr::Var { name, .. } => {
            out.insert(name.clone());
        }
        CExpr::Atom(_) | CExpr::Vector(_) | CExpr::Quote(_) | CExpr::SealNew => {}
        CExpr::Map(entries) => {
            for (_, v) in entries {
                collect_condition_symbols_from_expr(v, out);
            }
        }
        CExpr::If {
            cond,
            then_expr,
            else_expr,
            ..
        } => {
            collect_condition_symbols_from_expr(cond, out);
            collect_condition_symbols_from_expr(then_expr, out);
            collect_condition_symbols_from_expr(else_expr, out);
        }
        CExpr::Begin(xs) => {
            for x in xs {
                collect_condition_symbols_from_expr(x, out);
            }
        }
        CExpr::Let(bindings, body) => {
            for (_, rhs) in bindings {
                collect_condition_symbols_from_expr(rhs, out);
            }
            collect_condition_symbols_from_expr(body, out);
        }
        CExpr::FnUnary { body, .. } => {
            collect_condition_symbols_from_expr(body, out);
        }
        CExpr::Prim { args, .. } | CExpr::PrimUnknown { args, .. } => {
            for a in args {
                collect_condition_symbols_from_expr(a, out);
            }
        }
        CExpr::Seal(v, tok) | CExpr::Unseal(v, tok) | CExpr::App(v, tok) => {
            collect_condition_symbols_from_expr(v, out);
            collect_condition_symbols_from_expr(tok, out);
        }
        CExpr::AppN { callee, args, .. } => {
            collect_condition_symbols_from_expr(callee, out);
            for arg in args.iter() {
                collect_condition_symbols_from_expr(arg, out);
            }
        }
    }
}
