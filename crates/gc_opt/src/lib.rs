use std::collections::BTreeMap;
use std::fmt;

use egg::{CostFunction, EGraph, Extractor, Id, RecExpr, Rewrite, Runner, Symbol, rewrite};
use gc_coreform::{Term, canonicalize_module, hash_module};
use gc_kernel::{EvalCtx, Value, eval_module, value_hash};
use gc_prelude::build_prelude;
use num_bigint::BigInt;
use num_traits::ToPrimitive;

mod stage2_wasm;
pub use stage2_wasm::{
    Stage2CompileArtifact, Stage2CompileError, Stage2ValidationReport, Stage2ValueKind,
    stage2_compile_module, stage2_validation_report,
};

/// Aggregate statistics from optimizer runs.
#[derive(Debug, Clone, Default)]
pub struct OptimizeStats {
    pub egg_runs: u64,
    pub iterations: u64,
    pub eclasses: u64,
    pub enodes: u64,
    pub rewrites_applied: BTreeMap<String, u64>,
}

#[derive(Debug, Clone, Default)]
pub struct OptimizeReport {
    pub changed: bool,
    pub stats: OptimizeStats,
}

#[derive(Debug, Clone)]
pub struct Stage1GateReport {
    pub obligation: String,
    pub ok: bool,
    pub original_module_hash: [u8; 32],
    pub transformed_module_hash: [u8; 32],
    pub original_value_hash: Option<[u8; 32]>,
    pub transformed_value_hash: Option<[u8; 32]>,
    pub errors: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct Stage1PipelineOutcome {
    pub transformed_forms: Vec<Term>,
    pub optimize_report: OptimizeReport,
    pub gate_report: Stage1GateReport,
}

#[derive(Debug, Clone)]
pub struct OptimizeCommandOutcome {
    pub optimized_forms: Vec<Term>,
    pub stage1: Stage1PipelineOutcome,
    pub stage2: Option<Stage2ValidationReport>,
    pub wasm_artifact: Option<Stage2CompileArtifact>,
    pub original_hash: [u8; 32],
    pub optimized_hash: [u8; 32],
    pub changed: bool,
}

#[derive(Debug, Clone)]
pub enum OptimizeCommandError {
    Stage1Build(String),
    Stage1Gate(Box<Stage1PipelineOutcome>),
    Stage2Gate(Box<Stage2ValidationReport>),
    Stage2Compile(Stage2CompileError),
}

impl fmt::Display for OptimizeCommandError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Stage1Build(msg) => write!(f, "stage1/error: {msg}"),
            Self::Stage1Gate(_) => {
                write!(f, "core/obligation::stage1-validation failed")
            }
            Self::Stage2Gate(_) => write!(
                f,
                "core/obligation::translation-validation (stage2 CoreForm->WASM) failed"
            ),
            Self::Stage2Compile(e) => write!(f, "stage2/error: {e}"),
        }
    }
}

impl std::error::Error for OptimizeCommandError {}

/// Shared optimize command pipeline used by native and WASI CLIs.
///
/// This performs stage1 optimization + optional gate checks + optional stage2 analysis/compile.
pub fn optimize_command_pipeline(
    forms: &[Term],
    stage1_gate: bool,
    stage2_gate: bool,
    emit_wasm: bool,
) -> Result<OptimizeCommandOutcome, OptimizeCommandError> {
    let original_hash = hash_module(forms);
    let stage1 =
        stage1_pipeline(forms).map_err(|e| OptimizeCommandError::Stage1Build(e.to_string()))?;
    if stage1_gate && !stage1.gate_report.ok {
        return Err(OptimizeCommandError::Stage1Gate(Box::new(stage1)));
    }

    let optimized_forms = stage1.transformed_forms.clone();
    let optimized_hash = hash_module(&optimized_forms);
    let changed = optimized_hash != original_hash;

    let stage2 = if stage2_gate || emit_wasm {
        Some(stage2_validation_report(&optimized_forms))
    } else {
        None
    };
    if stage2_gate
        && let Some(s2) = &stage2
        && s2.supported
        && !s2.ok
    {
        return Err(OptimizeCommandError::Stage2Gate(Box::new(s2.clone())));
    }

    let wasm_artifact = if emit_wasm {
        Some(stage2_compile_module(&optimized_forms).map_err(OptimizeCommandError::Stage2Compile)?)
    } else {
        None
    };

    Ok(OptimizeCommandOutcome {
        optimized_forms,
        stage1,
        stage2,
        wasm_artifact,
        original_hash,
        optimized_hash,
        changed,
    })
}

/// Stage-1 compiler pipeline (CoreForm -> CoreForm) with built-in validation gate report.
///
/// Pipeline:
/// 1. Conservative pure-subset optimization (`gc_opt` e-graph + local folds)
/// 2. Canonicalization for stable downstream hashing
/// 3. Validation gate report (`core/obligation::stage1-validation`) by evaluating original and
///    transformed modules and comparing pure result hashes.
///
/// The gate report is always produced. Callers decide whether a failed gate should hard-fail.
pub fn stage1_pipeline(forms: &[Term]) -> Result<Stage1PipelineOutcome, anyhow::Error> {
    let (opt_forms, optimize_report) = optimize_module_with_report(forms);
    let transformed_forms =
        canonicalize_module(opt_forms).map_err(|e| anyhow::anyhow!("stage1 canonicalize: {e}"))?;
    let gate_report = stage1_validation_report(forms, &transformed_forms);
    Ok(Stage1PipelineOutcome {
        transformed_forms,
        optimize_report,
        gate_report,
    })
}

/// Validation gate used by stage-1 transforms.
///
/// `ok=true` only when both modules evaluate to pure values and their value hashes match.
pub fn stage1_validation_report(original: &[Term], transformed: &[Term]) -> Stage1GateReport {
    let original_module_hash = hash_module(original);
    let transformed_module_hash = hash_module(transformed);
    let mut errors = Vec::new();

    let original_eval = eval_pure_hash(original);
    let transformed_eval = eval_pure_hash(transformed);

    let original_value_hash = original_eval.as_ref().ok().copied();
    let transformed_value_hash = transformed_eval.as_ref().ok().copied();

    if let Err(e) = &original_eval {
        errors.push(format!("original module is not gate-valid: {e}"));
    }
    if let Err(e) = &transformed_eval {
        errors.push(format!("transformed module is not gate-valid: {e}"));
    }

    if let (Ok(a), Ok(b)) = (original_eval, transformed_eval)
        && a != b
    {
        errors.push("pure value hash mismatch after stage1 transform".to_string());
    }

    Stage1GateReport {
        obligation: "core/obligation::stage1-validation".to_string(),
        ok: errors.is_empty(),
        original_module_hash,
        transformed_module_hash,
        original_value_hash,
        transformed_value_hash,
        errors,
    }
}

fn eval_pure_hash(forms: &[Term]) -> Result<[u8; 32], String> {
    let mut ctx = EvalCtx::with_step_limit(None);
    let prelude = build_prelude(&mut ctx);
    let mut env = prelude.env;
    let v = eval_module(&mut ctx, &mut env, forms).map_err(|e| format!("{e}"))?;
    if matches!(v, Value::EffectProgram(_)) {
        return Err("effect program produced (not pure)".to_string());
    }
    Ok(value_hash(&v))
}

/// Optimize a CoreForm module by rewriting only conservative pure fragments.
///
/// This optimizer intentionally does *not* cross or rewrite through:
/// - `seal`, `unseal`
/// - `core/effect::*`
/// - `core/contract::*`
///
/// It uses an e-graph optimizer for a small pure subset (integer prim ops + `if`),
/// and falls back to structural recursion elsewhere. Extraction is deterministic.
pub fn optimize_module(forms: &[Term]) -> Vec<Term> {
    optimize_module_with_report(forms).0
}

pub fn optimize_module_with_report(forms: &[Term]) -> (Vec<Term>, OptimizeReport) {
    let mut r = OptimizeReport::default();
    let out: Vec<Term> = forms.iter().map(|t| optimize_topform(t, &mut r)).collect();
    r.changed = out != forms;
    (out, r)
}

fn optimize_topform(t: &Term, report: &mut OptimizeReport) -> Term {
    let Some(items) = t.as_proper_list() else {
        return optimize_term(t, report);
    };
    if items.len() == 3
        && matches!(items[0], Term::Symbol(s) if s == "def")
        && let Term::Symbol(name) = items[1]
    {
        return Term::list(vec![
            Term::Symbol("def".to_string()),
            Term::Symbol(name.clone()),
            optimize_term(items[2], report),
        ]);
    }
    optimize_term(t, report)
}

fn optimize_term(t: &Term, report: &mut OptimizeReport) -> Term {
    // Fast path: try the e-graph optimizer on pure fragments.
    if let Some(out) = optimize_pure_fragment_egg(t, report) {
        return out;
    }

    // Atoms
    match t {
        Term::Nil
        | Term::Bool(_)
        | Term::Int(_)
        | Term::Str(_)
        | Term::Bytes(_)
        | Term::Symbol(_) => return t.clone(),
        Term::Vector(_) => return t.clone(), // vectors are treated as data
        Term::Map(m) => {
            // map keys are data, map values are code
            let mut out = std::collections::BTreeMap::new();
            for (k, v) in m.iter() {
                out.insert(k.clone(), optimize_term(v, report));
            }
            return Term::Map(out);
        }
        Term::Pair(_, _) => {}
    }

    let Some(items) = t.as_proper_list() else {
        return t.clone();
    };
    if items.is_empty() {
        return Term::Nil;
    }

    // Special forms.
    if let Term::Symbol(h) = items[0] {
        match h.as_str() {
            "quote" => return t.clone(), // don't optimize data
            "fn" => {
                if items.len() >= 3 {
                    let mut xs = Vec::new();
                    xs.push(Term::Symbol("fn".to_string()));
                    xs.push(items[1].clone()); // params list is data-ish
                    for b in items.iter().skip(2) {
                        xs.push(optimize_term(b, report));
                    }
                    return Term::list(xs);
                }
                return t.clone();
            }
            "if" => {
                if items.len() == 4 {
                    let c = optimize_term(items[1], report);
                    let tt = optimize_term(items[2], report);
                    let ee = optimize_term(items[3], report);
                    if is_falsey(&c) {
                        return ee;
                    }
                    if is_truthy_literal(&c) {
                        return tt;
                    }
                    return Term::list(vec![Term::Symbol("if".to_string()), c, tt, ee]);
                }
                return t.clone();
            }
            "begin" => {
                if items.len() == 2 {
                    return optimize_term(items[1], report);
                }
                let mut xs = Vec::new();
                xs.push(Term::Symbol("begin".to_string()));
                for e in items.iter().skip(1) {
                    xs.push(optimize_term(e, report));
                }
                return Term::list(xs);
            }
            "let" => {
                // Keep bindings, optimize RHS and body.
                if items.len() >= 3 {
                    let binds = items[1].clone();
                    let binds_opt = optimize_let_binds(&binds, report);
                    let mut xs = Vec::new();
                    xs.push(Term::Symbol("let".to_string()));
                    xs.push(binds_opt);
                    for b in items.iter().skip(2) {
                        xs.push(optimize_term(b, report));
                    }
                    return Term::list(xs);
                }
                return t.clone();
            }
            "prim" => {
                return optimize_prim(items, report);
            }
            "seal" | "unseal" => return t.clone(), // opaque
            _ => {}
        }
    }

    // Treat `core/effect::*` and `core/contract::*` as opaque calls.
    if let Some((head, _args)) = flatten_app(t)
        && matches!(
            head,
            Term::Symbol(ref s)
                if s.starts_with("core/effect::") || s.starts_with("core/contract::")
        )
    {
        return t.clone();
    }

    // General application: optimize children.
    let mut xs = Vec::new();
    for it in items {
        xs.push(optimize_term(it, report));
    }
    Term::list(xs)
}

fn optimize_let_binds(binds: &Term, report: &mut OptimizeReport) -> Term {
    let Some(items) = binds.as_proper_list() else {
        return binds.clone();
    };
    let mut out = Vec::new();
    for b in items {
        let Some(pair) = b.as_proper_list() else {
            out.push(b.clone());
            continue;
        };
        if pair.len() != 2 {
            out.push(b.clone());
            continue;
        }
        let name = pair[0].clone();
        let rhs = optimize_term(pair[1], report);
        out.push(Term::list(vec![name, rhs]));
    }
    Term::list(out)
}

fn optimize_prim(items: Vec<&Term>, report: &mut OptimizeReport) -> Term {
    if items.len() < 2 {
        return Term::list(items.into_iter().cloned().collect());
    }
    let Term::Symbol(op) = items[1] else {
        // malformed; still optimize args
        let mut xs = Vec::new();
        xs.push(Term::Symbol("prim".to_string()));
        for a in items.iter().skip(1) {
            xs.push(optimize_term(a, report));
        }
        return Term::list(xs);
    };
    let mut args: Vec<Term> = items
        .iter()
        .skip(2)
        .map(|a| optimize_term(a, report))
        .collect();

    // Local constant folding: only int/* and only when args are literal ints.
    match (op.as_str(), args.as_slice()) {
        ("int/add", [a, b]) => {
            if let (Some(x), Some(y)) = (as_int(a), as_int(b)) {
                return Term::Int(x + y);
            }
            if is_int_zero(a) {
                return b.clone();
            }
            if is_int_zero(b) {
                return a.clone();
            }
        }
        ("int/sub", [a, b]) => {
            if let (Some(x), Some(y)) = (as_int(a), as_int(b)) {
                return Term::Int(x - y);
            }
            if is_int_zero(b) {
                return a.clone();
            }
        }
        ("int/mul", [a, b]) => {
            if let (Some(x), Some(y)) = (as_int(a), as_int(b)) {
                return Term::Int(x * y);
            }
            if is_int_one(a) {
                return b.clone();
            }
            if is_int_one(b) {
                return a.clone();
            }
            if is_int_zero(a) || is_int_zero(b) {
                return Term::Int(BigInt::from(0));
            }
        }
        ("int/eq?", [a, b]) => {
            if let (Some(x), Some(y)) = (as_int(a), as_int(b)) {
                return Term::Bool(x == y);
            }
        }
        ("int/lt?", [a, b]) => {
            if let (Some(x), Some(y)) = (as_int(a), as_int(b)) {
                return Term::Bool(x < y);
            }
        }
        _ => {}
    }

    let mut out = Vec::new();
    out.push(Term::Symbol("prim".to_string()));
    out.push(Term::Symbol(op.clone()));
    out.append(&mut args);
    Term::list(out)
}

fn optimize_pure_fragment_egg(t: &Term, report: &mut OptimizeReport) -> Option<Term> {
    let expr = term_to_pure_expr(t)?;
    let rules = pure_rules();
    let runner = Runner::default()
        .with_expr(&expr)
        .with_iter_limit(8)
        .with_node_limit(50_000);
    let runner = runner.run(&rules);

    let root = runner.roots[0];
    let egraph = runner.egraph;
    let (best_cost, best_expr) = Extractor::new(&egraph, DetCostFn).find_best(root);
    let _ = best_cost;

    let out = pure_expr_to_term(&best_expr)?;

    // Stats (deterministic aggregates).
    report.stats.egg_runs = report.stats.egg_runs.saturating_add(1);
    report.stats.iterations = report
        .stats
        .iterations
        .saturating_add(runner.iterations.len() as u64);
    report.stats.eclasses = report
        .stats
        .eclasses
        .saturating_add(egraph.number_of_classes() as u64);
    report.stats.enodes = report
        .stats
        .enodes
        .saturating_add(egraph.total_size() as u64);
    for it in &runner.iterations {
        for (name, n) in &it.applied {
            *report
                .stats
                .rewrites_applied
                .entry(name.to_string())
                .or_insert(0) += *n as u64;
        }
    }

    Some(out)
}

egg::define_language! {
    enum PureLang {
        Num(BigInt),
        "true" = True,
        "false" = False,
        Var(Symbol),
        "+" = Add([Id; 2]),
        "-" = Sub([Id; 2]),
        "*" = Mul([Id; 2]),
        "==" = Eq([Id; 2]),
        "<" = Lt([Id; 2]),
        "if" = If([Id; 3]),
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum ConstVal {
    Int(BigInt),
    Bool(bool),
}

#[derive(Default)]
struct ConstAnalysis;

impl egg::Analysis<PureLang> for ConstAnalysis {
    type Data = Option<ConstVal>;

    fn make(egraph: &mut EGraph<PureLang, Self>, enode: &PureLang) -> Self::Data {
        match enode {
            PureLang::Num(n) => Some(ConstVal::Int(n.clone())),
            PureLang::True => Some(ConstVal::Bool(true)),
            PureLang::False => Some(ConstVal::Bool(false)),
            PureLang::Var(_) => None,
            PureLang::Add([a, b]) => match (&egraph[*a].data, &egraph[*b].data) {
                (Some(ConstVal::Int(x)), Some(ConstVal::Int(y))) => Some(ConstVal::Int(x + y)),
                _ => None,
            },
            PureLang::Sub([a, b]) => match (&egraph[*a].data, &egraph[*b].data) {
                (Some(ConstVal::Int(x)), Some(ConstVal::Int(y))) => Some(ConstVal::Int(x - y)),
                _ => None,
            },
            PureLang::Mul([a, b]) => match (&egraph[*a].data, &egraph[*b].data) {
                (Some(ConstVal::Int(x)), Some(ConstVal::Int(y))) => Some(ConstVal::Int(x * y)),
                _ => None,
            },
            PureLang::Eq([a, b]) => match (&egraph[*a].data, &egraph[*b].data) {
                (Some(ConstVal::Int(x)), Some(ConstVal::Int(y))) => Some(ConstVal::Bool(x == y)),
                _ => None,
            },
            PureLang::Lt([a, b]) => match (&egraph[*a].data, &egraph[*b].data) {
                (Some(ConstVal::Int(x)), Some(ConstVal::Int(y))) => Some(ConstVal::Bool(x < y)),
                _ => None,
            },
            PureLang::If([c, t, e]) => match &egraph[*c].data {
                Some(ConstVal::Bool(true)) => egraph[*t].data.clone(),
                Some(ConstVal::Bool(false)) => egraph[*e].data.clone(),
                _ => None,
            },
        }
    }

    fn merge(&mut self, to: &mut Self::Data, from: Self::Data) -> egg::DidMerge {
        let before = to.clone();
        *to = match (&before, from) {
            (None, x) => x,
            (Some(x), None) => Some(x.clone()),
            (Some(a), Some(b)) if a == &b => Some(a.clone()),
            (Some(_), Some(_)) => None,
        };
        egg::DidMerge(before != *to, false)
    }

    fn modify(egraph: &mut EGraph<PureLang, Self>, id: Id) {
        let Some(c) = egraph[id].data.clone() else {
            return;
        };
        match c {
            ConstVal::Int(n) => {
                let cid = egraph.add(PureLang::Num(n));
                egraph.union(id, cid);
            }
            ConstVal::Bool(true) => {
                let cid = egraph.add(PureLang::True);
                egraph.union(id, cid);
            }
            ConstVal::Bool(false) => {
                let cid = egraph.add(PureLang::False);
                egraph.union(id, cid);
            }
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct DetCost {
    nodes: usize,
    repr: String,
}

impl Ord for DetCost {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.nodes
            .cmp(&other.nodes)
            .then_with(|| self.repr.cmp(&other.repr))
    }
}

impl PartialOrd for DetCost {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

struct DetCostFn;

impl CostFunction<PureLang> for DetCostFn {
    type Cost = DetCost;

    fn cost<C>(&mut self, enode: &PureLang, mut costs: C) -> Self::Cost
    where
        C: FnMut(Id) -> Self::Cost,
    {
        let mut nodes = 1usize;
        let repr = match enode {
            PureLang::Num(n) => n.to_string(),
            PureLang::True => "true".to_string(),
            PureLang::False => "false".to_string(),
            PureLang::Var(s) => s.to_string(),
            PureLang::Add([a, b]) => {
                let ca = costs(*a);
                let cb = costs(*b);
                nodes += ca.nodes + cb.nodes;
                format!("(+ {} {})", ca.repr, cb.repr)
            }
            PureLang::Sub([a, b]) => {
                let ca = costs(*a);
                let cb = costs(*b);
                nodes += ca.nodes + cb.nodes;
                format!("(- {} {})", ca.repr, cb.repr)
            }
            PureLang::Mul([a, b]) => {
                let ca = costs(*a);
                let cb = costs(*b);
                nodes += ca.nodes + cb.nodes;
                format!("(* {} {})", ca.repr, cb.repr)
            }
            PureLang::Eq([a, b]) => {
                let ca = costs(*a);
                let cb = costs(*b);
                nodes += ca.nodes + cb.nodes;
                format!("(== {} {})", ca.repr, cb.repr)
            }
            PureLang::Lt([a, b]) => {
                let ca = costs(*a);
                let cb = costs(*b);
                nodes += ca.nodes + cb.nodes;
                format!("(< {} {})", ca.repr, cb.repr)
            }
            PureLang::If([c, t, e]) => {
                let cc = costs(*c);
                let ct = costs(*t);
                let ce = costs(*e);
                nodes += cc.nodes + ct.nodes + ce.nodes;
                format!("(if {} {} {})", cc.repr, ct.repr, ce.repr)
            }
        };
        DetCost { nodes, repr }
    }
}

fn pure_rules() -> Vec<Rewrite<PureLang, ConstAnalysis>> {
    vec![
        rewrite!("add-comm"; "(+ ?a ?b)" => "(+ ?b ?a)"),
        rewrite!("mul-comm"; "(* ?a ?b)" => "(* ?b ?a)"),
        rewrite!("add-zero-l"; "(+ 0 ?a)" => "?a"),
        rewrite!("add-zero-r"; "(+ ?a 0)" => "?a"),
        rewrite!("mul-one-l"; "(* 1 ?a)" => "?a"),
        rewrite!("mul-one-r"; "(* ?a 1)" => "?a"),
        rewrite!("mul-zero-l"; "(* 0 ?a)" => "0"),
        rewrite!("mul-zero-r"; "(* ?a 0)" => "0"),
        rewrite!("sub-zero"; "(- ?a 0)" => "?a"),
        rewrite!("sub-self"; "(- ?a ?a)" => "0"),
        rewrite!("if-true"; "(if true ?t ?e)" => "?t"),
        rewrite!("if-false"; "(if false ?t ?e)" => "?e"),
        rewrite!("eq-self"; "(== ?a ?a)" => "true"),
        rewrite!("lt-self"; "(< ?a ?a)" => "false"),
    ]
}

fn term_to_pure_expr(t: &Term) -> Option<RecExpr<PureLang>> {
    let mut expr = RecExpr::<PureLang>::default();
    let _root = build_pure(t, &mut expr)?;
    Some(expr)
}

fn build_pure(t: &Term, expr: &mut RecExpr<PureLang>) -> Option<Id> {
    match t {
        Term::Int(i) => Some(expr.add(PureLang::Num(i.clone()))),
        Term::Bool(true) => Some(expr.add(PureLang::True)),
        Term::Bool(false) => Some(expr.add(PureLang::False)),
        Term::Symbol(s) => Some(expr.add(PureLang::Var(Symbol::from(s.as_str())))),
        Term::Nil | Term::Str(_) | Term::Bytes(_) | Term::Vector(_) | Term::Map(_) => None,
        Term::Pair(_, _) => {
            let items = t.as_proper_list()?;
            if items.is_empty() {
                return None;
            }
            if let Term::Symbol(h) = items[0] {
                match h.as_str() {
                    "if" if items.len() == 4 => {
                        let c = build_pure(items[1], expr)?;
                        let tt = build_pure(items[2], expr)?;
                        let ee = build_pure(items[3], expr)?;
                        return Some(expr.add(PureLang::If([c, tt, ee])));
                    }
                    "prim" if items.len() == 4 => {
                        let Term::Symbol(op) = items[1] else {
                            return None;
                        };
                        let a = build_pure(items[2], expr)?;
                        let b = build_pure(items[3], expr)?;
                        let n = match op.as_str() {
                            "int/add" => PureLang::Add([a, b]),
                            "int/sub" => PureLang::Sub([a, b]),
                            "int/mul" => PureLang::Mul([a, b]),
                            "int/eq?" => PureLang::Eq([a, b]),
                            "int/lt?" => PureLang::Lt([a, b]),
                            _ => return None,
                        };
                        return Some(expr.add(n));
                    }
                    _ => {}
                }
            }
            None
        }
    }
}

fn pure_expr_to_term(expr: &RecExpr<PureLang>) -> Option<Term> {
    // RecExpr stores nodes in post-order; the last node is the root.
    let mut terms: Vec<Term> = Vec::with_capacity(expr.as_ref().len());
    for n in expr.as_ref() {
        let t = match n {
            PureLang::Num(x) => Term::Int(x.clone()),
            PureLang::True => Term::Bool(true),
            PureLang::False => Term::Bool(false),
            PureLang::Var(s) => Term::Symbol(s.to_string()),
            PureLang::Add([a, b]) => Term::list(vec![
                Term::Symbol("prim".to_string()),
                Term::Symbol("int/add".to_string()),
                terms[usize::from(*a)].clone(),
                terms[usize::from(*b)].clone(),
            ]),
            PureLang::Sub([a, b]) => Term::list(vec![
                Term::Symbol("prim".to_string()),
                Term::Symbol("int/sub".to_string()),
                terms[usize::from(*a)].clone(),
                terms[usize::from(*b)].clone(),
            ]),
            PureLang::Mul([a, b]) => Term::list(vec![
                Term::Symbol("prim".to_string()),
                Term::Symbol("int/mul".to_string()),
                terms[usize::from(*a)].clone(),
                terms[usize::from(*b)].clone(),
            ]),
            PureLang::Eq([a, b]) => Term::list(vec![
                Term::Symbol("prim".to_string()),
                Term::Symbol("int/eq?".to_string()),
                terms[usize::from(*a)].clone(),
                terms[usize::from(*b)].clone(),
            ]),
            PureLang::Lt([a, b]) => Term::list(vec![
                Term::Symbol("prim".to_string()),
                Term::Symbol("int/lt?".to_string()),
                terms[usize::from(*a)].clone(),
                terms[usize::from(*b)].clone(),
            ]),
            PureLang::If([c, t, e]) => Term::list(vec![
                Term::Symbol("if".to_string()),
                terms[usize::from(*c)].clone(),
                terms[usize::from(*t)].clone(),
                terms[usize::from(*e)].clone(),
            ]),
        };
        terms.push(t);
    }
    terms.pop()
}

fn as_int(t: &Term) -> Option<BigInt> {
    match t {
        Term::Int(i) => Some(i.clone()),
        _ => None,
    }
}

fn is_int_zero(t: &Term) -> bool {
    matches!(t, Term::Int(i) if i.to_i64() == Some(0))
}

fn is_int_one(t: &Term) -> bool {
    matches!(t, Term::Int(i) if i.to_i64() == Some(1))
}

fn is_falsey(t: &Term) -> bool {
    matches!(t, Term::Nil | Term::Bool(false))
}

fn is_truthy_literal(t: &Term) -> bool {
    match t {
        Term::Nil | Term::Bool(false) => false,
        Term::Bool(true) => true,
        Term::Int(_) | Term::Str(_) | Term::Bytes(_) | Term::Symbol(_) => true,
        _ => false,
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

#[cfg(test)]
mod tests {
    use gc_coreform::{Term, canonicalize_module, parse_module, print_module};

    use super::{
        optimize_module, optimize_module_with_report, stage1_pipeline, stage1_validation_report,
    };

    #[test]
    fn folds_int_prim_constants() {
        let src = r#"
            (def x (prim int/add 1 2))
            x
        "#;
        let forms = canonicalize_module(parse_module(src).unwrap()).unwrap();
        let opt = optimize_module(&forms);
        let opt = canonicalize_module(opt).unwrap();

        // Find (def x <expr>) and check it became 3.
        let def = opt
            .iter()
            .find(|t| {
                t.as_proper_list().is_some_and(|xs| {
                    xs.len() == 3
                        && matches!(xs[0], Term::Symbol(s) if s == "def")
                        && matches!(xs[1], Term::Symbol(s) if s == "x")
                })
            })
            .expect("def x");
        let xs = def.as_proper_list().unwrap();
        assert!(matches!(xs[2], Term::Int(i) if i == &3.into()));
    }

    #[test]
    fn does_not_optimize_inside_quote() {
        let src = r#"
            (def x '(prim int/add 1 2))
            x
        "#;
        let forms = canonicalize_module(parse_module(src).unwrap()).unwrap();
        let opt = optimize_module(&forms);
        let opt = canonicalize_module(opt).unwrap();

        let def = opt
            .iter()
            .find(|t| {
                t.as_proper_list().is_some_and(|xs| {
                    xs.len() == 3
                        && matches!(xs[0], Term::Symbol(s) if s == "def")
                        && matches!(xs[1], Term::Symbol(s) if s == "x")
                })
            })
            .expect("def x");
        let xs = def.as_proper_list().unwrap();
        // Still a (quote ...) term, not folded to 3.
        assert!(
            matches!(xs[2].as_proper_list(), Some(q) if q.len() == 2 && matches!(q[0], Term::Symbol(s) if s == "quote"))
        );
    }

    #[test]
    fn egg_optimizer_eliminates_identities_deterministically() {
        let src = r#"
          (def x (prim int/add 0 (prim int/add y 0)))
          x
        "#;
        let forms = canonicalize_module(parse_module(src).unwrap()).unwrap();
        let (opt1, r1) = optimize_module_with_report(&forms);
        let (opt2, r2) = optimize_module_with_report(&forms);
        assert_eq!(
            print_module(&canonicalize_module(opt1.clone()).unwrap()),
            print_module(&canonicalize_module(opt2.clone()).unwrap())
        );
        assert!(r1.stats.egg_runs > 0);
        assert_eq!(r1.stats.egg_runs, r2.stats.egg_runs);

        let opt = canonicalize_module(opt1).unwrap();
        let def = opt
            .iter()
            .find(|t| {
                t.as_proper_list().is_some_and(|xs| {
                    xs.len() == 3
                        && matches!(xs[0], Term::Symbol(s) if s == "def")
                        && matches!(xs[1], Term::Symbol(s) if s == "x")
                })
            })
            .expect("def x");
        let xs = def.as_proper_list().unwrap();
        assert!(matches!(xs[2], Term::Symbol(s) if s == "y"));
    }

    #[test]
    fn stage1_validation_reports_ok_for_pure_equivalent_module() {
        let src = r#"
          (def x (prim int/add 0 (prim int/add 41 1)))
          x
        "#;
        let forms = canonicalize_module(parse_module(src).unwrap()).unwrap();
        let out = stage1_pipeline(&forms).expect("stage1 pipeline");
        assert!(
            out.gate_report.ok,
            "expected gate ok: {:?}",
            out.gate_report
        );
        assert!(out.gate_report.original_value_hash.is_some());
        assert!(out.gate_report.transformed_value_hash.is_some());
    }

    #[test]
    fn stage1_validation_fails_for_effectful_module() {
        let src = r#"
          (core/effect::perform
            'sys/time::now
            nil
            (fn (t) (core/effect::pure t)))
        "#;
        let forms = canonicalize_module(parse_module(src).unwrap()).unwrap();
        let gate = stage1_validation_report(&forms, &forms);
        assert!(!gate.ok);
        assert!(
            gate.errors
                .iter()
                .any(|e| e.contains("effect program produced")),
            "expected effect-related gate error, got {:?}",
            gate.errors
        );
    }
}
