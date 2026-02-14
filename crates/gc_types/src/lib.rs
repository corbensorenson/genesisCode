use std::collections::{BTreeMap, BTreeSet};

use gc_coreform::{Term, TermOrdKey, print_term};

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
}

#[derive(Debug, Clone)]
pub struct ExportEffectReport {
    pub name: String,
    pub effects: InferredEffects,
}

pub fn infer_effects(forms: &[Term]) -> InferredEffects {
    let mut out = InferredEffects {
        ops: BTreeSet::new(),
        unknown: false,
    };
    for f in forms {
        infer_effects_term(&mut out, f);
    }
    out
}

pub fn infer_effects_in_term(t: &Term) -> InferredEffects {
    let mut out = InferredEffects {
        ops: BTreeSet::new(),
        unknown: false,
    };
    infer_effects_term(&mut out, t);
    out
}

fn infer_effects_term(out: &mut InferredEffects, t: &Term) {
    // Recurse through code-ish forms. We deliberately skip quoted data.
    if let Some(items) = t.as_proper_list() {
        if items.is_empty() {
            return;
        }
        // Special forms with known shapes.
        if matches!(items[0], Term::Symbol(s) if s == "quote") {
            return;
        }
        if matches!(items[0], Term::Symbol(s) if s == "def") {
            if items.len() == 3 {
                infer_effects_term(out, items[2]);
            }
            return;
        }
        if matches!(items[0], Term::Symbol(s) if s == "fn") {
            if items.len() >= 3 {
                for b in items.iter().skip(2) {
                    infer_effects_term(out, b);
                }
            }
            return;
        }
        if matches!(items[0], Term::Symbol(s) if s == "if") {
            if items.len() == 4 {
                infer_effects_term(out, items[1]);
                infer_effects_term(out, items[2]);
                infer_effects_term(out, items[3]);
            }
            return;
        }
        if matches!(items[0], Term::Symbol(s) if s == "begin") {
            for e in items.iter().skip(1) {
                infer_effects_term(out, e);
            }
            return;
        }
        if matches!(items[0], Term::Symbol(s) if s == "let") {
            if items.len() >= 3 {
                // (let ((x e) ...) body...)
                if let Some(binds) = items[1].as_proper_list() {
                    for b in binds {
                        if let Some(pair) = b.as_proper_list()
                            && pair.len() == 2
                        {
                            infer_effects_term(out, pair[1]);
                        }
                    }
                }
                for b in items.iter().skip(2) {
                    infer_effects_term(out, b);
                }
            }
            return;
        }
        if matches!(items[0], Term::Symbol(s) if s == "prim") {
            // Primitive args are expressions.
            for a in items.iter().skip(2) {
                infer_effects_term(out, a);
            }
            return;
        }
        if matches!(items[0], Term::Symbol(s) if s == "seal" || s == "unseal") {
            // Skip; sealing is pure but treated as opaque in optimizer/type world.
            for a in items.iter().skip(1) {
                infer_effects_term(out, a);
            }
            return;
        }

        // General application: canonical form is nested binary, but support n-ary too.
        if let Some((head, args)) = flatten_app(t) {
            if matches!(&head, Term::Symbol(s) if s == "core/effect::perform") {
                // (core/effect::perform op payload k)
                if args.len() == 3 {
                    match literal_op_symbol(&args[0]) {
                        Some(op) => {
                            out.ops.insert(op);
                        }
                        None => out.unknown = true,
                    }
                }
            }
            // Recurse on head/args.
            infer_effects_term(out, &head);
            for a in args {
                infer_effects_term(out, &a);
            }
            return;
        }

        // Fallback: recurse into all items.
        for e in items {
            infer_effects_term(out, e);
        }
        return;
    }

    match t {
        Term::Vector(_) => {
            // Vectors are treated as data in v0.2.
        }
        Term::Map(m) => {
            // Map keys are data; values are code.
            for (_k, v) in m.iter() {
                infer_effects_term(out, v);
            }
        }
        Term::Pair(_, _) => {}
        _ => {}
    }
}

fn flatten_app(t: &Term) -> Option<(Term, Vec<Term>)> {
    let items = t.as_proper_list()?;
    if items.len() == 2 {
        let f = items[0].clone();
        let x = items[1].clone();
        if let Some((head, mut args)) = flatten_app(&f) {
            args.push(x);
            return Some((head, args));
        }
        return Some((f, vec![x]));
    }
    if !items.is_empty() {
        let head = items[0].clone();
        let args = items.into_iter().skip(1).cloned().collect();
        return Some((head, args));
    }
    None
}

fn literal_op_symbol(t: &Term) -> Option<String> {
    let items = t.as_proper_list()?;
    if items.len() == 2
        && matches!(items[0], Term::Symbol(s) if s == "quote")
        && let Term::Symbol(s) = items[1]
    {
        return Some(s.clone());
    }
    None
}

pub fn typecheck_package(mods: &[ModuleForTypecheck]) -> TypecheckReport {
    let mut report = TypecheckReport {
        ok: true,
        errors: Vec::new(),
        warnings: Vec::new(),
        modules: Vec::new(),
    };
    for m in mods {
        let mr = typecheck_one(m);
        report.ok &= mr.ok;
        report.errors.extend(mr.errors.iter().cloned());
        report.warnings.extend(mr.warnings.iter().cloned());
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
                "{}: {} effect ops could not be fully inferred (non-literal op passed to core/effect::perform)",
                m.path, e
            ));
            if caps.is_empty() {
                ok = false;
                errors.push(format!(
                    "{}: {} declares :caps [] but has unknown effect ops",
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

    // Export/type conformance.
    let types = match meta_types(&meta) {
        None => {
            ok = false;
            errors.push(format!("{}: ::meta missing :types", m.path));
            BTreeMap::new()
        }
        Some(t) => t,
    };
    for e in exports {
        if let Some(ty) = types.get(&e) {
            if matches!(ty, Term::Symbol(s) if s == "?") {
                warnings.push(format!("{}: export {} has type ?", m.path, e));
            }
        } else {
            ok = false;
            errors.push(format!(
                "{}: exported symbol {} has no type in ::meta :types",
                m.path, e
            ));
        }
    }

    ModuleReport {
        path: m.path.clone(),
        ok,
        errors,
        warnings,
        inferred_effects: inferred,
        export_effects,
    }
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

impl TypecheckReport {
    pub fn to_term(&self) -> Term {
        let mut m = BTreeMap::new();
        m.insert(
            TermOrdKey(Term::symbol(":kind")),
            Term::Str("genesis/typecheck-v0.2".to_string()),
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
        let mods: Vec<Term> = self.modules.iter().map(|x| x.to_term()).collect();
        m.insert(TermOrdKey(Term::symbol(":modules")), Term::Vector(mods));
        Term::Map(m)
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
mod tests {
    use gc_coreform::{Term, canonicalize_module, parse_module};

    use super::*;

    #[test]
    fn infers_literal_effect_ops() {
        let src = r#"
            (def ::meta '{:exports [] :caps [sys/time::now] :types {}})
            (def x
              (core/effect::perform 'sys/time::now nil (fn (t) (core/effect::pure t))))
            x
        "#;
        let forms = canonicalize_module(parse_module(src).unwrap()).unwrap();
        let inf = infer_effects(&forms);
        assert!(inf.ops.contains("sys/time::now"));
        assert!(!inf.unknown);
    }

    #[test]
    fn marks_unknown_when_op_is_not_literal() {
        let src = r#"
            (def ::meta '{:exports [] :caps [?] :types {}})
            (def op 'sys/time::now)
            (def x (core/effect::perform op nil (fn (t) (core/effect::pure t))))
            x
        "#;
        let forms = canonicalize_module(parse_module(src).unwrap()).unwrap();
        let inf = infer_effects(&forms);
        assert!(inf.ops.is_empty());
        assert!(inf.unknown);
    }

    #[test]
    fn typecheck_requires_types_for_exports() {
        let src = r#"
            (def ::meta '{:exports [m::x] :caps [] :types {}})
            (def m::x 1)
            m::x
        "#;
        let forms = canonicalize_module(parse_module(src).unwrap()).unwrap();
        let meta = forms.iter().find_map(|t| {
            let items = t.as_proper_list()?;
            if items.len() == 3
                && matches!(items[0], Term::Symbol(s) if s == "def")
                && matches!(items[1], Term::Symbol(s) if s == "::meta")
            {
                let q = items[2].as_proper_list()?;
                if q.len() == 2 && matches!(q[0], Term::Symbol(s) if s == "quote") {
                    return Some(q[1].clone());
                }
            }
            None
        });
        let m = ModuleForTypecheck {
            path: "x.gc".to_string(),
            forms,
            meta,
        };
        let r = typecheck_package(&[m]);
        assert!(!r.ok);
        assert!(
            r.errors
                .iter()
                .any(|e| e.contains("exported symbol m::x has no type"))
        );
    }
}
