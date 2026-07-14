use std::collections::{BTreeMap, BTreeSet};

use gc_coreform::{Term, TermOrdKey, print_term};

mod diagnostics;
mod effect_inference;
mod infer;
mod ty;

use crate::effect_inference::is_core_task_effect_op;
use crate::infer::{InferSession, infer_module_types};
use crate::ty::{EffRow, RowTail, Ty, parse_type_term};

pub use crate::diagnostics::TypecheckDiagnostic;
use crate::diagnostics::module_diagnostics;
pub use crate::effect_inference::{infer_effects, infer_effects_in_term};

#[derive(Debug, Clone)]
pub struct ModuleForTypecheck {
    pub path: String,
    pub forms: Vec<Term>,
    pub meta: Option<Term>, // expected to be a map datum
}

#[derive(Debug, Clone)]
pub struct InferredEffects {
    pub ops: BTreeSet<String>,
    pub unknown: bool,
}

#[derive(Debug, Clone)]
pub struct TypecheckReport {
    pub ok: bool,
    pub errors: Vec<String>,
    pub warnings: Vec<String>,
    pub diagnostics: Vec<TypecheckDiagnostic>,
    pub modules: Vec<ModuleReport>,
}

#[derive(Debug, Clone)]
pub struct ModuleReport {
    pub path: String,
    pub ok: bool,
    pub errors: Vec<String>,
    pub warnings: Vec<String>,
    pub inferred_effects: InferredEffects,
    pub export_effects: Vec<ExportEffectReport>,
    pub export_types: Vec<ExportTypeReport>,
}

#[derive(Debug, Clone)]
pub struct ExportEffectReport {
    pub name: String,
    pub effects: InferredEffects,
}

#[derive(Debug, Clone)]
pub struct ExportTypeReport {
    pub name: String,
    pub declared: Option<Term>,
    pub inferred: Term,
    pub ok: bool,
    pub errors: Vec<String>,
    pub warnings: Vec<String>,
}

pub fn typecheck_package(mods: &[ModuleForTypecheck]) -> TypecheckReport {
    let mut report = TypecheckReport {
        ok: true,
        errors: Vec::new(),
        warnings: Vec::new(),
        diagnostics: Vec::new(),
        modules: Vec::new(),
    };
    for m in mods {
        let mr = typecheck_one(m);
        report.ok &= mr.ok;
        report.errors.extend(mr.errors.iter().cloned());
        report.warnings.extend(mr.warnings.iter().cloned());
        report
            .diagnostics
            .extend(module_diagnostics(&mr.path, &mr.errors, &mr.warnings));
        report.modules.push(mr);
    }
    report
}

fn typecheck_one(m: &ModuleForTypecheck) -> ModuleReport {
    let inferred = infer_effects(&m.forms);
    let mut ok = true;
    let mut errors = Vec::new();
    let mut warnings = Vec::new();

    let meta = match &m.meta {
        None => {
            ok = false;
            errors.push(format!("{}: missing ::meta", m.path));
            return ModuleReport {
                path: m.path.clone(),
                ok,
                errors,
                warnings,
                inferred_effects: inferred,
                export_effects: Vec::new(),
                export_types: Vec::new(),
            };
        }
        Some(Term::Map(mm)) => Term::Map(mm.clone()),
        Some(other) => {
            ok = false;
            errors.push(format!(
                "{}: ::meta must be a map datum, got {}",
                m.path,
                print_term(other)
            ));
            return ModuleReport {
                path: m.path.clone(),
                ok,
                errors,
                warnings,
                inferred_effects: inferred,
                export_effects: Vec::new(),
                export_types: Vec::new(),
            };
        }
    };

    let exports = meta_exports(&meta).unwrap_or_default();
    if exports.is_empty() {
        warnings.push(format!("{}: ::meta :exports is empty", m.path));
    }
    let caps = match meta_caps(&meta) {
        None => {
            ok = false;
            errors.push(format!("{}: ::meta missing :caps", m.path));
            Vec::new()
        }
        Some(x) => x,
    };
    let strict_effects_meta = match meta_strict_effects(&meta) {
        Ok(v) => v,
        Err(e) => {
            ok = false;
            errors.push(format!("{}: {}", m.path, e));
            false
        }
    };
    let strict_shapes = match meta_strict_shapes(&meta) {
        Ok(v) => v,
        Err(e) => {
            ok = false;
            errors.push(format!("{}: {}", m.path, e));
            false
        }
    };
    let strict_effects = strict_effects_meta || caps.iter().any(|c| is_core_task_effect_op(c));

    let caps_set: BTreeSet<String> = caps.iter().cloned().collect();
    let def_map = defs_map(&m.forms);
    let mut export_effects = Vec::new();
    for e in exports.iter() {
        let Some(expr) = def_map.get(e) else {
            ok = false;
            errors.push(format!(
                "{}: exported symbol {} has no (def {}) in module",
                m.path, e, e
            ));
            continue;
        };
        let eff = infer_effects_in_term(expr);
        if eff.unknown {
            warnings.push(format!(
                "{}: {} effect ops could not be fully inferred (non-literal op passed to core/effect::perform or core/caps::perform, or unknown task wrapper arity)",
                m.path, e
            ));
            if caps.is_empty() {
                ok = false;
                errors.push(format!(
                    "{}: {} declares :caps [] but has unknown effect ops",
                    m.path, e
                ));
            }
            if strict_effects {
                ok = false;
                errors.push(format!(
                    "{}: {} strict effect mode forbids unknown effect ops; use literal op symbols and closed declared rows",
                    m.path, e
                ));
            }
        }
        for op in eff.ops.iter() {
            if !caps_set.contains(op) {
                ok = false;
                errors.push(format!(
                    "{}: {} inferred effect op {} not present in ::meta :caps",
                    m.path, e, op
                ));
            }
        }
        export_effects.push(ExportEffectReport {
            name: e.clone(),
            effects: eff,
        });
    }
    let export_eff_map: BTreeMap<String, InferredEffects> = export_effects
        .iter()
        .map(|x| (x.name.clone(), x.effects.clone()))
        .collect();

    // Parse declared export types early so inference can use them as hints (row tails, dispatch, etc.).
    let types = match meta_types(&meta) {
        None => {
            ok = false;
            errors.push(format!("{}: ::meta missing :types", m.path));
            BTreeMap::new()
        }
        Some(t) => t,
    };
    let mut declared_parsed: BTreeMap<String, Ty> = BTreeMap::new();
    for (name, term) in types.iter() {
        if matches!(term, Term::Symbol(s) if s == "?") {
            continue;
        }
        match parse_type_term(term) {
            Ok(ty) => {
                declared_parsed.insert(name.clone(), ty);
            }
            Err(pe) => {
                ok = false;
                errors.push(format!("{}: type parse error for {}: {}", m.path, name, pe));
            }
        }
    }

    // Infer module types once (sequential def order).
    let mut infer_sess = InferSession::default();
    let (_env, inferred_defs) = infer_module_types(&m.forms, &mut infer_sess, &declared_parsed);
    for e in infer_sess.errors.iter() {
        ok = false;
        errors.push(format!("{}: {e}", m.path));
    }
    for w in infer_sess.warnings.iter() {
        warnings.push(format!("{}: {w}", m.path));
    }

    // Export/type conformance.
    let mut export_types = Vec::new();
    for e in exports {
        if let Some(ty) = types.get(&e) {
            let declared_term = ty.clone();
            if matches!(ty, Term::Symbol(s) if s == "?") {
                warnings.push(format!("{}: export {} has type ?", m.path, e));
            }
            let mut tr_ok = true;
            let mut tr_errors = Vec::new();
            let mut tr_warnings = Vec::new();
            let inferred_ty = inferred_defs.get(&e).cloned().unwrap_or(Ty::Any);

            // Parse declared type and check compatibility when it isn't `?`.
            if !matches!(ty, Term::Symbol(s) if s == "?") {
                let decl_res = if let Some(d) = declared_parsed.get(&e) {
                    Ok(d.clone())
                } else {
                    parse_type_term(ty)
                };
                match decl_res {
                    Ok(decl) => {
                        if !type_compatible(&inferred_ty, &decl, strict_shapes) {
                            tr_ok = false;
                            tr_errors.push(format!(
                                "declared type mismatch for {e}: declared {}, inferred {}",
                                print_term(&decl.to_term()),
                                print_term(&inferred_ty.to_term())
                            ));
                        }
                        // If declared includes an effect row, enforce it against inferred effects.
                        if let Some(decl_eff) = declared_eff_row(&decl) {
                            let eff = export_eff_map.get(&e).cloned().unwrap_or(InferredEffects {
                                ops: BTreeSet::new(),
                                unknown: false,
                            });
                            if strict_effects && decl_eff.tail.is_open() {
                                tr_ok = false;
                                tr_errors.push(format!(
                                    "{e}: strict effect mode requires a closed declared effect row"
                                ));
                            }
                            if eff.unknown && matches!(decl_eff.tail, RowTail::Closed) {
                                tr_ok = false;
                                tr_errors.push(format!(
                                    "{e}: inferred unknown effect ops but declared effect row is closed"
                                ));
                            }
                            for op in eff.ops.iter() {
                                if !decl_eff.ops.contains(op)
                                    && matches!(decl_eff.tail, RowTail::Closed)
                                {
                                    tr_ok = false;
                                    tr_errors.push(format!(
                                        "{e}: inferred effect op {op} not present in declared effect row"
                                    ));
                                }
                            }
                        }
                        if strict_shapes && has_unresolved_contract_ops(&inferred_ty) {
                            tr_ok = false;
                            tr_errors.push(format!(
                                "{e}: strict shape mode forbids unresolved contract op signatures"
                            ));
                        }
                    }
                    Err(pe) => {
                        tr_ok = false;
                        tr_errors.push(format!("type parse error for {e}: {pe}"));
                    }
                }
            } else if matches!(inferred_ty, Ty::Any) {
                tr_warnings.push("inferred type is ?".to_string());
            }

            ok &= tr_ok;
            for msg in tr_errors.iter() {
                errors.push(format!("{}: {}", m.path, msg));
            }
            export_types.push(ExportTypeReport {
                name: e.clone(),
                declared: Some(declared_term),
                inferred: inferred_ty.to_term(),
                ok: tr_ok,
                errors: tr_errors,
                warnings: tr_warnings,
            });
        } else {
            ok = false;
            errors.push(format!(
                "{}: exported symbol {} has no type in ::meta :types",
                m.path, e
            ));
            export_types.push(ExportTypeReport {
                name: e.clone(),
                declared: None,
                inferred: inferred_defs.get(&e).cloned().unwrap_or(Ty::Any).to_term(),
                ok: false,
                errors: vec!["missing declared type".to_string()],
                warnings: Vec::new(),
            });
        }
    }

    ModuleReport {
        path: m.path.clone(),
        ok,
        errors,
        warnings,
        inferred_effects: inferred,
        export_effects,
        export_types,
    }
}

fn declared_eff_row(ty: &Ty) -> Option<&EffRow> {
    match ty {
        Ty::Fn { eff, .. } => Some(eff),
        Ty::Prog { eff, .. } => Some(eff),
        _ => None,
    }
}

fn has_unresolved_contract_ops(ty: &Ty) -> bool {
    match ty {
        Ty::Msg { op, payload } => op.is_none() || has_unresolved_contract_ops(payload),
        Ty::Fn { param, ret, .. } => {
            has_unresolved_contract_ops(param) || has_unresolved_contract_ops(ret)
        }
        Ty::Prog { ret, .. } => has_unresolved_contract_ops(ret),
        Ty::Rec { fields, .. } => fields.iter().any(|(_, v)| has_unresolved_contract_ops(v)),
        Ty::Contract { methods, .. } => methods.iter().any(|(_, v)| has_unresolved_contract_ops(v)),
        _ => false,
    }
}

fn type_compatible(inferred: &Ty, declared: &Ty, strict_shapes: bool) -> bool {
    // `?` in the declared position accepts anything.
    if matches!(declared, Ty::Any) {
        return true;
    }
    match (inferred, declared) {
        (Ty::Any, _) => false,
        (Ty::Int, Ty::Int)
        | (Ty::Bool, Ty::Bool)
        | (Ty::Nil, Ty::Nil)
        | (Ty::Str, Ty::Str)
        | (Ty::Bytes, Ty::Bytes)
        | (Ty::Symbol, Ty::Symbol) => true,
        (
            Ty::Msg {
                op: iop,
                payload: ip,
            },
            Ty::Msg {
                op: dop,
                payload: dp,
            },
        ) => {
            if let Some(d) = dop
                && iop.as_deref() != Some(d.as_str())
            {
                return false;
            }
            type_compatible(ip, dp, strict_shapes)
        }
        (
            Ty::Fn {
                param: ip,
                ret: ir,
                eff: ie,
            },
            Ty::Fn {
                param: dp,
                ret: dr,
                eff: de,
            },
        ) => {
            if !type_compatible(ip, dp, strict_shapes) {
                return false;
            }
            if !type_compatible(ir, dr, strict_shapes) {
                return false;
            }
            eff_row_compatible(ie, de)
        }
        (Ty::Prog { ret: ir, eff: ie }, Ty::Prog { ret: dr, eff: de }) => {
            type_compatible(ir, dr, strict_shapes) && eff_row_compatible(ie, de)
        }
        (
            Ty::Rec {
                fields: ifs,
                tail: i_tail,
            },
            Ty::Rec {
                fields: dfs,
                tail: d_tail,
            },
        ) => {
            if !dfs.iter().all(|(k, dt)| {
                ifs.get(k)
                    .is_some_and(|it| type_compatible(it, dt, strict_shapes))
            }) {
                return false;
            }
            if strict_shapes && matches!(d_tail, RowTail::Closed) {
                if !matches!(i_tail, RowTail::Closed) {
                    return false;
                }
                if ifs.len() != dfs.len() {
                    return false;
                }
            }
            true
        }
        (
            Ty::Contract {
                methods: ims,
                tail: i_tail,
            },
            Ty::Contract {
                methods: dms,
                tail: d_tail,
            },
        ) => {
            if !dms.iter().all(|(k, dt)| {
                ims.get(k)
                    .is_some_and(|it| type_compatible(it, dt, strict_shapes))
            }) {
                return false;
            }
            if strict_shapes && matches!(d_tail, RowTail::Closed) {
                if !matches!(i_tail, RowTail::Closed) {
                    return false;
                }
                if ims.len() != dms.len() {
                    return false;
                }
            }
            true
        }
        _ => false,
    }
}

fn eff_row_compatible(inferred: &EffRow, declared: &EffRow) -> bool {
    if declared.tail.is_open() {
        return true;
    }
    inferred.ops.is_subset(&declared.ops) && matches!(inferred.tail, RowTail::Closed)
}

fn meta_exports(meta: &Term) -> Option<Vec<String>> {
    let Term::Map(m) = meta else { return None };
    let v = m.get(&TermOrdKey(Term::Symbol(":exports".to_string())))?;
    let Term::Vector(xs) = v else { return None };
    let mut out = Vec::new();
    for x in xs {
        if let Term::Symbol(s) = x {
            out.push(s.clone());
        }
    }
    Some(out)
}

fn meta_caps(meta: &Term) -> Option<Vec<String>> {
    let Term::Map(m) = meta else { return None };
    let v = m.get(&TermOrdKey(Term::Symbol(":caps".to_string())))?;
    let Term::Vector(xs) = v else { return None };
    let mut out = Vec::new();
    for x in xs {
        if let Term::Symbol(s) = x {
            out.push(s.clone());
        }
    }
    Some(out)
}

fn meta_types(meta: &Term) -> Option<BTreeMap<String, Term>> {
    let Term::Map(m) = meta else { return None };
    let v = m.get(&TermOrdKey(Term::Symbol(":types".to_string())))?;
    let Term::Map(tm) = v else { return None };
    let mut out = BTreeMap::new();
    for (k, v) in tm {
        let Term::Symbol(name) = &k.0 else { continue };
        out.insert(name.clone(), v.clone());
    }
    Some(out)
}

fn meta_strict_effects(meta: &Term) -> Result<bool, String> {
    let Term::Map(m) = meta else {
        return Ok(false);
    };
    let Some(v) = m.get(&TermOrdKey(Term::symbol(":strict-effects"))) else {
        return Ok(false);
    };
    match v {
        Term::Bool(b) => Ok(*b),
        other => Err(format!(
            "::meta :strict-effects must be bool, got {}",
            print_term(other)
        )),
    }
}

fn meta_strict_shapes(meta: &Term) -> Result<bool, String> {
    let Term::Map(m) = meta else {
        return Ok(false);
    };
    let Some(v) = m.get(&TermOrdKey(Term::symbol(":strict-shapes"))) else {
        return Ok(false);
    };
    match v {
        Term::Bool(b) => Ok(*b),
        other => Err(format!(
            "::meta :strict-shapes must be bool, got {}",
            print_term(other)
        )),
    }
}

impl ModuleReport {
    pub fn to_term(&self) -> Term {
        let mut m = BTreeMap::new();
        m.insert(
            TermOrdKey(Term::symbol(":path")),
            Term::Str(self.path.clone()),
        );
        m.insert(TermOrdKey(Term::symbol(":ok")), Term::Bool(self.ok));
        m.insert(
            TermOrdKey(Term::symbol(":errors")),
            Term::Vector(self.errors.iter().cloned().map(Term::Str).collect()),
        );
        m.insert(
            TermOrdKey(Term::symbol(":warnings")),
            Term::Vector(self.warnings.iter().cloned().map(Term::Str).collect()),
        );
        m.insert(
            TermOrdKey(Term::symbol(":inferred-ops")),
            Term::Vector(
                self.inferred_effects
                    .ops
                    .iter()
                    .cloned()
                    .map(Term::Symbol)
                    .collect(),
            ),
        );
        m.insert(
            TermOrdKey(Term::symbol(":unknown-ops")),
            Term::Bool(self.inferred_effects.unknown),
        );
        m.insert(
            TermOrdKey(Term::symbol(":exports")),
            Term::Vector(self.export_effects.iter().map(|e| e.to_term()).collect()),
        );
        m.insert(
            TermOrdKey(Term::symbol(":types")),
            Term::Vector(self.export_types.iter().map(|e| e.to_term()).collect()),
        );
        Term::Map(m)
    }
}

impl ExportEffectReport {
    pub fn to_term(&self) -> Term {
        let mut m = BTreeMap::new();
        m.insert(
            TermOrdKey(Term::symbol(":name")),
            Term::Symbol(self.name.clone()),
        );
        m.insert(
            TermOrdKey(Term::symbol(":ops")),
            Term::Vector(self.effects.ops.iter().cloned().map(Term::Symbol).collect()),
        );
        m.insert(
            TermOrdKey(Term::symbol(":unknown")),
            Term::Bool(self.effects.unknown),
        );
        Term::Map(m)
    }
}

impl ExportTypeReport {
    pub fn to_term(&self) -> Term {
        let mut m = BTreeMap::new();
        m.insert(
            TermOrdKey(Term::symbol(":name")),
            Term::Symbol(self.name.clone()),
        );
        m.insert(TermOrdKey(Term::symbol(":ok")), Term::Bool(self.ok));
        m.insert(
            TermOrdKey(Term::symbol(":declared")),
            self.declared.clone().unwrap_or(Term::Nil),
        );
        m.insert(TermOrdKey(Term::symbol(":inferred")), self.inferred.clone());
        m.insert(
            TermOrdKey(Term::symbol(":errors")),
            Term::Vector(self.errors.iter().cloned().map(Term::Str).collect()),
        );
        m.insert(
            TermOrdKey(Term::symbol(":warnings")),
            Term::Vector(self.warnings.iter().cloned().map(Term::Str).collect()),
        );
        Term::Map(m)
    }
}

fn defs_map(forms: &[Term]) -> BTreeMap<String, Term> {
    let mut out = BTreeMap::new();
    for f in forms {
        if let Some((name, expr)) = parse_def(f) {
            out.insert(name, expr);
        }
    }
    out
}

fn parse_def(t: &Term) -> Option<(String, Term)> {
    let items = t.as_proper_list()?;
    if items.len() != 3 {
        return None;
    }
    if !matches!(items[0], Term::Symbol(s) if s == "def") {
        return None;
    }
    let Term::Symbol(name) = items[1] else {
        return None;
    };
    Some((name.clone(), items[2].clone()))
}

#[cfg(test)]
#[path = "lib_tests.rs"]
mod tests;
