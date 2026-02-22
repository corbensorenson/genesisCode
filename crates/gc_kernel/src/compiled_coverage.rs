use std::collections::BTreeSet;
use std::sync::Arc;

use super::{CExpr, CompiledForm, CompiledModule, CoverageSiteManifest};

pub(super) fn compiled_module_coverage_manifest_from_compiled(
    compiled: &CompiledModule,
) -> CoverageSiteManifest {
    let mut manifest = CoverageSiteManifest::default();
    for form in &compiled.forms {
        match form {
            CompiledForm::Def(_, expr) | CompiledForm::Expr(expr) => {
                collect_coverage_manifest_from_expr(expr, &mut manifest);
            }
        }
    }
    manifest
}

fn collect_coverage_manifest_from_expr(expr: &Arc<CExpr>, manifest: &mut CoverageSiteManifest) {
    match expr.as_ref() {
        CExpr::Atom(_) | CExpr::Vector(_) | CExpr::Quote(_) | CExpr::SealNew => {}
        CExpr::Var { site_id, .. } => {
            manifest.statement_sites.insert(site_id.clone());
        }
        CExpr::Map(entries) => {
            for (_, v) in entries {
                collect_coverage_manifest_from_expr(v, manifest);
            }
        }
        CExpr::If {
            cond,
            then_expr,
            else_expr,
            site_id,
        } => {
            manifest.decision_sites.insert(site_id.clone());
            let mut vars = BTreeSet::new();
            collect_condition_symbols_from_expr(cond, &mut vars);
            manifest
                .decision_conditions
                .entry(site_id.clone())
                .or_default()
                .extend(vars);
            collect_coverage_manifest_from_expr(cond, manifest);
            collect_coverage_manifest_from_expr(then_expr, manifest);
            collect_coverage_manifest_from_expr(else_expr, manifest);
        }
        CExpr::Begin(xs) => {
            for x in xs {
                collect_coverage_manifest_from_expr(x, manifest);
            }
        }
        CExpr::Let(bindings, body) => {
            for (_, rhs) in bindings {
                collect_coverage_manifest_from_expr(rhs, manifest);
            }
            collect_coverage_manifest_from_expr(body, manifest);
        }
        CExpr::FnUnary { body, .. } => {
            collect_coverage_manifest_from_expr(body, manifest);
        }
        CExpr::Prim { args, .. } => {
            for a in args {
                collect_coverage_manifest_from_expr(a, manifest);
            }
        }
        CExpr::Seal(v, tok) | CExpr::Unseal(v, tok) | CExpr::App(v, tok) => {
            collect_coverage_manifest_from_expr(v, manifest);
            collect_coverage_manifest_from_expr(tok, manifest);
        }
    }
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
        CExpr::Prim { args, .. } => {
            for a in args {
                collect_condition_symbols_from_expr(a, out);
            }
        }
        CExpr::Seal(v, tok) | CExpr::Unseal(v, tok) | CExpr::App(v, tok) => {
            collect_condition_symbols_from_expr(v, out);
            collect_condition_symbols_from_expr(tok, out);
        }
    }
}
