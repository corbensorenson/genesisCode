use std::collections::BTreeSet;

use gc_coreform::Term;

pub(crate) fn closure_free_vars(param: &str, body: &Term) -> BTreeSet<String> {
    let mut bound = BTreeSet::from([param.to_string()]);
    let mut free = BTreeSet::new();
    collect(body, &mut bound, &mut free);
    free
}

fn collect(term: &Term, bound: &mut BTreeSet<String>, free: &mut BTreeSet<String>) {
    stacker::maybe_grow(32 * 1024, 8 * 1024 * 1024, || {
        collect_inner(term, bound, free)
    });
}

fn collect_inner(term: &Term, bound: &mut BTreeSet<String>, free: &mut BTreeSet<String>) {
    match term {
        Term::Symbol(name) => {
            if !bound.contains(name) {
                free.insert(name.clone());
            }
        }
        Term::Map(entries) => {
            for value in entries.values() {
                collect(value, bound, free);
            }
        }
        Term::Pair(_, _) => collect_list(term, bound, free),
        // Vector elements and map keys are data in CoreForm, not expressions.
        Term::Nil
        | Term::Bool(_)
        | Term::Int(_)
        | Term::Str(_)
        | Term::Bytes(_)
        | Term::Vector(_) => {}
    }
}

fn collect_list(term: &Term, bound: &mut BTreeSet<String>, free: &mut BTreeSet<String>) {
    let Some(items) = term.as_proper_list() else {
        return;
    };
    let Some(Term::Symbol(head)) = items.first().copied() else {
        for item in items {
            collect(item, bound, free);
        }
        return;
    };

    match head.as_str() {
        "quote" | "def" => {}
        "fn" => collect_fn(&items, bound, free),
        "let" => collect_let(&items, bound, free),
        "prim" => {
            for arg in items.iter().skip(2) {
                collect(arg, bound, free);
            }
        }
        "if" | "begin" | "seal" | "unseal" => {
            for expr in items.iter().skip(1) {
                collect(expr, bound, free);
            }
        }
        _ => {
            for item in items {
                collect(item, bound, free);
            }
        }
    }
}

fn collect_fn(items: &[&Term], bound: &mut BTreeSet<String>, free: &mut BTreeSet<String>) {
    let Some(params) = items.get(1).and_then(|term| term.as_proper_list()) else {
        return;
    };
    if params.is_empty() || params.iter().any(|param| !matches!(param, Term::Symbol(_))) {
        return;
    }

    let mut nested_bound = bound.clone();
    for param in params {
        if let Term::Symbol(name) = param {
            nested_bound.insert(name.clone());
        }
    }
    for body in items.iter().skip(2) {
        collect(body, &mut nested_bound, free);
    }
}

fn collect_let(items: &[&Term], bound: &mut BTreeSet<String>, free: &mut BTreeSet<String>) {
    let Some(bindings) = items.get(1).and_then(|term| term.as_proper_list()) else {
        return;
    };
    let mut body_bound = bound.clone();
    for binding in bindings {
        let Some(pair) = binding.as_proper_list() else {
            return;
        };
        if pair.len() != 2 {
            return;
        }
        let Term::Symbol(name) = pair[0] else {
            return;
        };
        collect(pair[1], &mut body_bound, free);
        body_bound.insert(name.clone());
    }
    for body in items.iter().skip(2) {
        collect(body, &mut body_bound, free);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gc_coreform::parse_term;

    fn names(source: &str, param: &str) -> BTreeSet<String> {
        closure_free_vars(param, &parse_term(source).expect("valid test term"))
    }

    #[test]
    fn respects_quote_data_maps_and_sequential_let_shadowing() {
        assert_eq!(
            names(
                "(let ((x outer) (y x)) (prim int/add y z) (quote ignored))",
                "z"
            ),
            BTreeSet::from(["outer".to_string()])
        );
        assert_eq!(names("{:key free}", "x"), BTreeSet::from(["free".into()]));
        assert!(names("[free]", "x").is_empty());
    }

    #[test]
    fn nested_functions_bind_only_their_own_parameters() {
        assert_eq!(
            names("(fn (y z) (prim int/add x (prim int/add y free)))", "x"),
            BTreeSet::from(["free".to_string()])
        );
    }
}
