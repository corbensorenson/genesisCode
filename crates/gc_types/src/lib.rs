use std::collections::{BTreeMap, BTreeSet};

use gc_coreform::{Term, TermOrdKey, print_term};

mod infer;
mod ty;

use crate::infer::{InferSession, infer_module_types};
use crate::ty::{EffRow, RowTail, Ty, parse_type_term};

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
            if let Term::Symbol(sym) = &head {
                match sym.as_str() {
                    "core/effect::perform" => {
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
                    "core/caps::perform" => {
                        // (core/caps::perform op payload)
                        if args.len() == 2 {
                            match literal_op_symbol(&args[0]) {
                                Some(op) => {
                                    out.ops.insert(op);
                                }
                                None => out.unknown = true,
                            }
                        }
                    }
                    _ => {
                        if let Some(ops) = direct_effect_ops(sym, args.len()) {
                            for op in ops {
                                out.ops.insert((*op).to_string());
                            }
                        } else if sym.starts_with("core/task::")
                            || sym.starts_with("core/editor/task::")
                        {
                            // Unknown task wrapper/combinator shape: remain conservative.
                            out.unknown = true;
                        }
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

fn direct_effect_ops(head: &str, arity: usize) -> Option<&'static [&'static str]> {
    match head {
        // Base deterministic task ABI.
        "core/task::spawn" if arity >= 3 => Some(&["core/task::spawn"]),
        "core/task::await" if arity >= 1 => Some(&["core/task::await"]),
        "core/task::cancel" if arity >= 1 => Some(&["core/task::cancel"]),
        "core/task::status" if arity >= 1 => Some(&["core/task::status"]),
        "core/task::scope" if arity >= 1 => Some(&["core/task::scope"]),

        // AI-facing task combinators in prelude, mapped to base ABI effects.
        "core/task::await-all" if arity >= 1 => Some(&["core/task::await"]),
        "core/task::await-all-loop" if arity >= 3 => Some(&["core/task::await"]),
        "core/task::all" if arity >= 1 => Some(&["core/task::await"]),
        "core/task::race" if arity >= 1 => Some(&["core/task::await", "core/task::cancel"]),
        "core/task::race-cancel-rest" if arity >= 3 => Some(&["core/task::cancel"]),
        "core/task::spawn-batch" if arity >= 6 => Some(&["core/task::spawn"]),
        "core/task::spawn-batch-loop" if arity >= 7 => Some(&["core/task::spawn"]),
        "core/task::map-bounded" if arity >= 5 => Some(&["core/task::spawn", "core/task::await"]),
        "core/task::map-bounded-loop" if arity >= 7 => {
            Some(&["core/task::spawn", "core/task::await"])
        }
        "core/task::parallel-map-bounded" if arity >= 5 => {
            Some(&["core/task::spawn", "core/task::await"])
        }

        // Editor task wrappers lower to host editor task capabilities.
        "core/editor/task::spawn" if arity >= 3 => Some(&["editor/task::spawn"]),
        "core/editor/task::poll" if arity >= 1 => Some(&["editor/task::poll"]),
        "core/editor/task::cancel" if arity >= 1 => Some(&["editor/task::cancel"]),
        _ => None,
    }
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
                        if !type_compatible(&inferred_ty, &decl) {
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
                            if strict_effects
                                && has_core_task_effect_ops(&eff)
                                && decl_eff.tail.is_open()
                            {
                                tr_ok = false;
                                tr_errors.push(format!(
                                    "{e}: strict effect mode requires a closed declared effect row for concurrent task exports"
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

fn type_compatible(inferred: &Ty, declared: &Ty) -> bool {
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
            type_compatible(ip, dp)
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
            if !type_compatible(ip, dp) {
                return false;
            }
            if !type_compatible(ir, dr) {
                return false;
            }
            eff_row_compatible(ie, de)
        }
        (Ty::Prog { ret: ir, eff: ie }, Ty::Prog { ret: dr, eff: de }) => {
            type_compatible(ir, dr) && eff_row_compatible(ie, de)
        }
        (
            Ty::Rec {
                fields: ifs,
                tail: _,
            },
            Ty::Rec {
                fields: dfs,
                tail: _,
            },
        ) => dfs
            .iter()
            .all(|(k, dt)| ifs.get(k).is_some_and(|it| type_compatible(it, dt))),
        (
            Ty::Contract {
                methods: ims,
                tail: _,
            },
            Ty::Contract {
                methods: dms,
                tail: _,
            },
        ) => dms
            .iter()
            .all(|(k, dt)| ims.get(k).is_some_and(|it| type_compatible(it, dt))),
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

fn is_core_task_effect_op(op: &str) -> bool {
    matches!(
        op,
        "core/task::spawn"
            | "core/task::await"
            | "core/task::cancel"
            | "core/task::status"
            | "core/task::scope"
    )
}

fn has_core_task_effect_ops(eff: &InferredEffects) -> bool {
    eff.ops.iter().any(|op| is_core_task_effect_op(op))
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
mod tests {
    use gc_coreform::{Term, canonicalize_module, parse_module};

    use super::*;
    use crate::infer::infer_module_types;
    use crate::ty::RowTail;

    fn extract_meta(forms: &[Term]) -> Option<Term> {
        forms.iter().find_map(|t| {
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
        })
    }

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
    fn infers_caps_perform_literal_ops() {
        let src = r#"
            (def ::meta '{:exports [] :caps [editor/task::poll] :types {}})
            (def x ((core/caps::perform 'editor/task::poll) {:task-id "task-1"}))
            x
        "#;
        let forms = canonicalize_module(parse_module(src).unwrap()).unwrap();
        let inf = infer_effects(&forms);
        assert!(inf.ops.contains("editor/task::poll"));
        assert!(!inf.unknown);
    }

    #[test]
    fn infers_task_wrapper_ops_without_inlining() {
        let src = r#"
            (def ::meta '{:exports [] :caps [core/task::await] :types {}})
            (def x (core/task::await "task-1"))
            x
        "#;
        let forms = canonicalize_module(parse_module(src).unwrap()).unwrap();
        let inf = infer_effects(&forms);
        assert!(inf.ops.contains("core/task::await"));
        assert!(!inf.unknown);
    }

    #[test]
    fn typecheck_requires_types_for_exports() {
        let src = r#"
            (def ::meta '{:exports [m::x] :caps [] :types {}})
            (def m::x 1)
            m::x
        "#;
        let forms = canonicalize_module(parse_module(src).unwrap()).unwrap();
        let meta = extract_meta(&forms);
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

    #[test]
    fn contract_row_typing_accepts_declared_method() {
        let src = r#"
          (def ::meta
            '{
              :exports [pkg/t::c]
              :caps []
              :types {
                pkg/t::c
                  (Contract
                    [[foo/bar::x (Fn (Msg ?) Int (Eff [] nil))]]
                    nil)}})

          (def pkg/t::c
            (core/contract::extend
              core/contract::genesis
              {foo/bar::x (fn (m) 10)}
              {}))

          pkg/t::c
        "#;
        let forms = canonicalize_module(parse_module(src).unwrap()).unwrap();
        let meta = extract_meta(&forms);
        let m = ModuleForTypecheck {
            path: "t.gc".to_string(),
            forms,
            meta,
        };
        let r = typecheck_package(&[m]);
        assert!(r.ok, "expected ok, errors: {:?}", r.errors);
    }

    #[test]
    fn contract_row_typing_rejects_missing_declared_method() {
        let src = r#"
          (def ::meta
            '{
              :exports [pkg/t::c]
              :caps []
              :types {
                pkg/t::c
                  (Contract
                    [[foo/bar::y (Fn (Msg ?) Int (Eff [] nil))]]
                    nil)}})

          (def pkg/t::c
            (core/contract::extend
              core/contract::genesis
              {foo/bar::x (fn (m) 10)}
              {}))

          pkg/t::c
        "#;
        let forms = canonicalize_module(parse_module(src).unwrap()).unwrap();
        let meta = extract_meta(&forms);
        let m = ModuleForTypecheck {
            path: "t.gc".to_string(),
            forms,
            meta,
        };
        let r = typecheck_package(&[m]);
        assert!(!r.ok);
        assert!(
            r.errors
                .iter()
                .any(|e| e.contains("declared type mismatch")),
            "expected declared type mismatch error, got {:?}",
            r.errors
        );
    }

    #[test]
    fn infer_perform_returns_prog_of_continuation_prog() {
        let src = r#"
            (def ::meta '{:exports [] :caps [sys/time::now] :types {}})
            (def m::p
              (core/effect::perform
                'sys/time::now
                nil
                (fn (t) (core/effect::pure 1))))
            m::p
        "#;
        let forms = canonicalize_module(parse_module(src).unwrap()).unwrap();
        let mut sess = InferSession::default();
        let (_env, defs) = infer_module_types(&forms, &mut sess, &BTreeMap::new());
        assert!(
            sess.errors.is_empty(),
            "unexpected infer errors: {:?}",
            sess.errors
        );
        let ty = defs.get("m::p").cloned().unwrap_or(Ty::Any);
        match ty {
            Ty::Prog { ret, eff } => {
                assert_eq!(*ret, Ty::Int);
                assert!(eff.ops.contains("sys/time::now"));
                assert!(matches!(eff.tail, RowTail::Closed));
            }
            other => panic!("expected Prog, got {}", print_term(&other.to_term())),
        }
    }

    #[test]
    fn infer_contract_extend_preserves_row_tail_var() {
        let src = r#"
          (def ::meta '{:exports [] :caps [] :types {}})
          (def m::c
            (core/contract::extend
              core/contract::genesis
              {foo/bar::x (fn (m) 10)}
              {}))
          m::c
        "#;
        let forms = canonicalize_module(parse_module(src).unwrap()).unwrap();
        let mut sess = InferSession::default();
        let (_env, defs) = infer_module_types(&forms, &mut sess, &BTreeMap::new());
        assert!(
            sess.errors.is_empty(),
            "unexpected infer errors: {:?}",
            sess.errors
        );
        let ty = defs.get("m::c").cloned().unwrap_or(Ty::Any);
        match ty {
            Ty::Contract { tail, methods } => {
                assert!(matches!(tail, RowTail::Var(ref s) if s == "r"));
                assert!(methods.contains_key("foo/bar::x"));
            }
            other => panic!("expected Contract, got {}", print_term(&other.to_term())),
        }
    }

    #[test]
    fn strict_effects_reject_unknown_effect_ops() {
        let src = r#"
          (def ::meta
            '{
              :exports [m::x]
              :caps [core/task::spawn]
              :strict-effects true
              :types {m::x ?}})
          (def m::op 'core/task::spawn)
          (def m::x
            (core/effect::perform m::op {:payload 1} (fn (resp) (core/effect::pure resp))))
          m::x
        "#;
        let forms = canonicalize_module(parse_module(src).unwrap()).unwrap();
        let m = ModuleForTypecheck {
            path: "strict.gc".to_string(),
            meta: extract_meta(&forms),
            forms,
        };
        let r = typecheck_package(&[m]);
        assert!(!r.ok);
        assert!(
            r.errors
                .iter()
                .any(|e| e.contains("strict effect mode forbids unknown effect ops")),
            "expected strict unknown-op error, got {:?}",
            r.errors
        );
    }

    #[test]
    fn strict_effects_require_closed_declared_row_for_task_exports() {
        let src = r#"
          (def ::meta
            '{
              :exports [m::x]
              :caps [core/task::await]
              :strict-effects true
              :types {m::x (Prog ? (Eff [core/task::await] ?))}})
          (def m::x
            (core/effect::perform
              'core/task::await
              {:task-id "task-1"}
              (fn (resp) (core/effect::pure resp))))
          m::x
        "#;
        let forms = canonicalize_module(parse_module(src).unwrap()).unwrap();
        let m = ModuleForTypecheck {
            path: "strict-row.gc".to_string(),
            meta: extract_meta(&forms),
            forms,
        };
        let r = typecheck_package(&[m]);
        assert!(!r.ok);
        assert!(
            r.errors.iter().any(|e| e.contains(
                "strict effect mode requires a closed declared effect row for concurrent task exports"
            )),
            "expected strict closed-row error, got {:?}",
            r.errors
        );
    }
}
