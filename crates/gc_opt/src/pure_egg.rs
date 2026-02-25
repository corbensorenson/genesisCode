use egg::{CostFunction, EGraph, Extractor, Id, RecExpr, Rewrite, Runner, Symbol, rewrite};
use gc_coreform::Term;
use num_bigint::BigInt;

use crate::OptimizeReport;

pub(crate) fn optimize_pure_fragment_egg(t: &Term, report: &mut OptimizeReport) -> Option<Term> {
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
