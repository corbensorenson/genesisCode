use gc_coreform::Term;
use num_bigint::BigInt;
use num_traits::ToPrimitive;

/// Optimize a CoreForm module by rewriting only conservative pure fragments.
///
/// This optimizer intentionally does *not* cross or rewrite through:
/// - `seal`, `unseal`
/// - `core/effect::*`
/// - `core/contract::*`
///
/// It performs constant folding and a few algebraic identities for `prim int/*`
/// and constant-folds `if` when the condition is a literal.
pub fn optimize_module(forms: &[Term]) -> Vec<Term> {
    forms.iter().map(optimize_topform).collect()
}

fn optimize_topform(t: &Term) -> Term {
    let Some(items) = t.as_proper_list() else {
        return optimize_term(t);
    };
    if items.len() == 3
        && matches!(items[0], Term::Symbol(s) if s == "def")
        && let Term::Symbol(name) = items[1]
    {
        return Term::list(vec![
            Term::Symbol("def".to_string()),
            Term::Symbol(name.clone()),
            optimize_term(items[2]),
        ]);
    }
    optimize_term(t)
}

fn optimize_term(t: &Term) -> Term {
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
                out.insert(k.clone(), optimize_term(v));
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
                        xs.push(optimize_term(b));
                    }
                    return Term::list(xs);
                }
                return t.clone();
            }
            "if" => {
                if items.len() == 4 {
                    let c = optimize_term(items[1]);
                    let tt = optimize_term(items[2]);
                    let ee = optimize_term(items[3]);
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
                    return optimize_term(items[1]);
                }
                let mut xs = Vec::new();
                xs.push(Term::Symbol("begin".to_string()));
                for e in items.iter().skip(1) {
                    xs.push(optimize_term(e));
                }
                return Term::list(xs);
            }
            "let" => {
                // Keep bindings, optimize RHS and body.
                if items.len() >= 3 {
                    let binds = items[1].clone();
                    let binds_opt = optimize_let_binds(&binds);
                    let mut xs = Vec::new();
                    xs.push(Term::Symbol("let".to_string()));
                    xs.push(binds_opt);
                    for b in items.iter().skip(2) {
                        xs.push(optimize_term(b));
                    }
                    return Term::list(xs);
                }
                return t.clone();
            }
            "prim" => {
                return optimize_prim(items);
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
        xs.push(optimize_term(it));
    }
    Term::list(xs)
}

fn optimize_let_binds(binds: &Term) -> Term {
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
        let rhs = optimize_term(pair[1]);
        out.push(Term::list(vec![name, rhs]));
    }
    Term::list(out)
}

fn optimize_prim(items: Vec<&Term>) -> Term {
    if items.len() < 2 {
        return Term::list(items.into_iter().cloned().collect());
    }
    let Term::Symbol(op) = items[1] else {
        // malformed; still optimize args
        let mut xs = Vec::new();
        xs.push(Term::Symbol("prim".to_string()));
        for a in items.iter().skip(1) {
            xs.push(optimize_term(a));
        }
        return Term::list(xs);
    };
    let mut args: Vec<Term> = items.iter().skip(2).map(|a| optimize_term(a)).collect();

    // Constant folding: only int/* and only when args are literal ints.
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
    use gc_coreform::{Term, canonicalize_module, parse_module};

    use super::optimize_module;

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
}
