use gc_coreform::Term;

use crate::InferredEffects;

pub fn infer_effects(forms: &[Term]) -> InferredEffects {
    let mut out = InferredEffects {
        ops: std::collections::BTreeSet::new(),
        unknown: false,
    };
    for f in forms {
        infer_effects_term(&mut out, f);
    }
    out
}

pub fn infer_effects_in_term(t: &Term) -> InferredEffects {
    let mut out = InferredEffects {
        ops: std::collections::BTreeSet::new(),
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
        // Pure task DSL constructors.
        "core/task::reduce-seq" if arity >= 1 => Some(&[]),
        "core/task::program" if arity >= 1 => Some(&[]),
        "core/task::program-with-initial" if arity >= 1 => Some(&[]),
        "core/task::step/sleep-ms" if arity >= 1 => Some(&[]),
        "core/task::step/set" if arity >= 1 => Some(&[]),
        "core/task::step/int-add" if arity >= 1 => Some(&[]),
        "core/task::step/int-mul" if arity >= 1 => Some(&[]),
        "core/task::step/str-append" if arity >= 1 => Some(&[]),
        "core/task::step/vec-push" if arity >= 1 => Some(&[]),
        "core/task::step/map-put" if arity >= 2 => Some(&[]),
        "core/task::step/fail" if arity >= 1 => Some(&[]),
        "core/task::step/return" if arity >= 1 => Some(&[]),

        // Base deterministic task ABI.
        "core/task::spawn" if arity >= 3 => Some(&["core/task::spawn"]),
        "core/task::await" if arity >= 1 => Some(&["core/task::await"]),
        "core/task::cancel" if arity >= 1 => Some(&["core/task::cancel"]),
        "core/task::status" if arity >= 1 => Some(&["core/task::status"]),
        "core/task::scope" if arity >= 1 => Some(&["core/task::scope"]),
        "core/task::channel-open" if arity >= 1 => Some(&["core/task::channel-open"]),
        "core/task::channel-send" if arity >= 2 => Some(&["core/task::channel-send"]),
        "core/task::channel-recv" if arity >= 1 => Some(&["core/task::channel-recv"]),
        "core/task::channel-close" if arity >= 1 => Some(&["core/task::channel-close"]),
        "core/task::channel-status" if arity >= 1 => Some(&["core/task::channel-status"]),

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
        "core/task::task-group" if arity >= 4 => Some(&["core/task::spawn"]),
        "core/task::task-group-await" if arity >= 1 => Some(&["core/task::await"]),
        "core/task::spawn-program" if arity >= 3 => Some(&["core/task::spawn"]),
        "core/task::spawn-eval" if arity >= 3 => Some(&["core/task::spawn"]),
        "core/task::spawn-eval1" if arity >= 4 => Some(&["core/task::spawn"]),
        "core/task::spawn-evaln" if arity >= 4 => Some(&["core/task::spawn"]),
        "core/task::parallel-reduce-bounded" if arity >= 7 => {
            Some(&["core/task::spawn", "core/task::await"])
        }
        "core/task::parallel-reduce" if arity >= 7 => {
            Some(&["core/task::spawn", "core/task::await"])
        }

        // Editor task wrappers lower to host editor task capabilities.
        "core/editor/task::spawn" if arity >= 3 => Some(&["editor/task::spawn"]),
        "core/editor/task::poll" if arity >= 1 => Some(&["editor/task::poll"]),
        "core/editor/task::cancel" if arity >= 1 => Some(&["editor/task::cancel"]),
        _ => None,
    }
}

pub fn is_core_task_effect_op(op: &str) -> bool {
    matches!(
        op,
        "core/task::spawn"
            | "core/task::await"
            | "core/task::cancel"
            | "core/task::status"
            | "core/task::scope"
            | "core/task::channel-open"
            | "core/task::channel-send"
            | "core/task::channel-recv"
            | "core/task::channel-close"
            | "core/task::channel-status"
    )
}

pub fn has_core_task_effect_ops(eff: &InferredEffects) -> bool {
    eff.ops.iter().any(|op| is_core_task_effect_op(op))
}
