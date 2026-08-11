use std::collections::{BTreeMap, BTreeSet};

use gc_coreform::{SpecialForm, Term};

use crate::{
    InferredEffects,
    ty::{EffRow, EffectRowBindings, RowTail, Ty},
};

use super::{InferSession, TypeEnv, arg_type_match, flatten_app, infer_term};

pub fn infer_effects_in_terms_with_env(forms: &[Term], env: &TypeEnv) -> InferredEffects {
    let mut out = InferredEffects {
        ops: std::collections::BTreeSet::new(),
        unknown: false,
    };
    for form in forms {
        merge_inferred_effects(&mut out, crate::infer_effects_in_term(form));
        collect_typed_call_effects(&mut out, form, env, None);
    }
    out
}

pub fn infer_effects_in_term_with_env(t: &Term, env: &TypeEnv) -> InferredEffects {
    infer_effects_in_term_with_expected(t, env, None)
}

pub fn infer_effects_in_term_with_expected(
    t: &Term,
    env: &TypeEnv,
    expected: Option<&Ty>,
) -> InferredEffects {
    let mut out = crate::infer_effects_in_term(t);
    collect_typed_call_effects(&mut out, t, env, expected);
    out
}

pub fn unknown_effect_signature_symbols_in_term(
    t: &Term,
    package_declarations: &BTreeMap<String, Ty>,
) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    collect_unknown_effect_signatures(&mut out, t, package_declarations, &BTreeSet::new());
    out
}

fn collect_unknown_effect_signatures(
    out: &mut BTreeSet<String>,
    t: &Term,
    package_declarations: &BTreeMap<String, Ty>,
    bound: &BTreeSet<String>,
) {
    if let Some(items) = t.as_proper_list() {
        if items.is_empty() {
            return;
        }
        if let Term::Symbol(head) = items[0]
            && let Some(form) = SpecialForm::from_symbol(head)
        {
            match form {
                SpecialForm::Quote => return,
                SpecialForm::Def => {
                    if items.len() == 3 {
                        collect_unknown_effect_signatures(
                            out,
                            items[2],
                            package_declarations,
                            bound,
                        );
                    }
                }
                SpecialForm::Fn => {
                    let mut body_bound = bound.clone();
                    if let Some(params) = items.get(1).and_then(|term| term.as_proper_list()) {
                        for param in params {
                            if let Term::Symbol(name) = param {
                                body_bound.insert(name.clone());
                            }
                        }
                    }
                    for body in items.iter().skip(2) {
                        collect_unknown_effect_signatures(
                            out,
                            body,
                            package_declarations,
                            &body_bound,
                        );
                    }
                }
                SpecialForm::If | SpecialForm::Begin => {
                    for expression in items.iter().skip(1) {
                        collect_unknown_effect_signatures(
                            out,
                            expression,
                            package_declarations,
                            bound,
                        );
                    }
                }
                SpecialForm::Let => {
                    let mut body_bound = bound.clone();
                    if let Some(bindings) = items.get(1).and_then(|term| term.as_proper_list()) {
                        for binding in bindings {
                            if let Some(pair) = binding.as_proper_list()
                                && pair.len() == 2
                            {
                                collect_unknown_effect_signatures(
                                    out,
                                    pair[1],
                                    package_declarations,
                                    &body_bound,
                                );
                                if let Term::Symbol(name) = pair[0] {
                                    body_bound.insert(name.clone());
                                }
                            }
                        }
                    }
                    for body in items.iter().skip(2) {
                        collect_unknown_effect_signatures(
                            out,
                            body,
                            package_declarations,
                            &body_bound,
                        );
                    }
                }
                SpecialForm::Prim => {
                    for argument in items.iter().skip(2) {
                        collect_unknown_effect_signatures(
                            out,
                            argument,
                            package_declarations,
                            bound,
                        );
                    }
                }
                SpecialForm::Seal | SpecialForm::Unseal => {
                    for argument in items.iter().skip(1) {
                        collect_unknown_effect_signatures(
                            out,
                            argument,
                            package_declarations,
                            bound,
                        );
                    }
                }
            }
            return;
        }

        if let Some((head, args)) = flatten_app(t) {
            if let Term::Symbol(symbol) = &head
                && !bound.contains(symbol)
                && matches!(package_declarations.get(symbol), Some(Ty::Any))
            {
                out.insert(symbol.clone());
            }
            collect_unknown_effect_signatures(out, &head, package_declarations, bound);
            for argument in args {
                collect_unknown_effect_signatures(out, &argument, package_declarations, bound);
            }
            return;
        }

        for item in items {
            collect_unknown_effect_signatures(out, item, package_declarations, bound);
        }
        return;
    }

    if let Term::Map(entries) = t {
        for value in entries.values() {
            collect_unknown_effect_signatures(out, value, package_declarations, bound);
        }
    }
}

fn collect_typed_call_effects(
    out: &mut InferredEffects,
    t: &Term,
    env: &TypeEnv,
    expected: Option<&Ty>,
) {
    if let Some(items) = t.as_proper_list() {
        if items.is_empty() {
            return;
        }
        if let Term::Symbol(head) = items[0]
            && let Some(form) = SpecialForm::from_symbol(head)
        {
            match form {
                SpecialForm::Quote => return,
                SpecialForm::Def => {
                    if items.len() == 3 {
                        let declared = match items[1] {
                            Term::Symbol(name) => env.get(name),
                            _ => None,
                        };
                        collect_typed_call_effects(out, items[2], env, declared);
                    }
                }
                SpecialForm::Fn => {
                    let mut body_env = env.clone();
                    let mut expected_parameter = expected;
                    if let Some(params) = items.get(1).and_then(|term| term.as_proper_list()) {
                        for param in params {
                            let parameter_ty = match expected_parameter {
                                Some(Ty::Fn { param, ret, .. }) => {
                                    expected_parameter = Some(ret);
                                    param.as_ref().clone()
                                }
                                _ => Ty::Any,
                            };
                            if let Term::Symbol(name) = param {
                                body_env.set(name.clone(), parameter_ty);
                            }
                        }
                    }
                    let bodies = &items[2..];
                    for (index, body) in bodies.iter().enumerate() {
                        let body_expected = (index + 1 == bodies.len())
                            .then_some(expected_parameter)
                            .flatten();
                        collect_typed_call_effects(out, body, &body_env, body_expected);
                    }
                }
                SpecialForm::If => {
                    for (index, branch) in items.iter().skip(1).enumerate() {
                        let branch_expected = (index > 0).then_some(expected).flatten();
                        collect_typed_call_effects(out, branch, env, branch_expected);
                    }
                }
                SpecialForm::Begin => {
                    let expressions = &items[1..];
                    for (index, expression) in expressions.iter().enumerate() {
                        let expression_expected = (index + 1 == expressions.len())
                            .then_some(expected)
                            .flatten();
                        collect_typed_call_effects(out, expression, env, expression_expected);
                    }
                }
                SpecialForm::Let => {
                    let mut body_env = env.clone();
                    if let Some(bindings) = items.get(1).and_then(|term| term.as_proper_list()) {
                        for binding in bindings {
                            if let Some(pair) = binding.as_proper_list()
                                && pair.len() == 2
                            {
                                collect_typed_call_effects(out, pair[1], &body_env, None);
                                if let Term::Symbol(name) = pair[0] {
                                    let mut session = InferSession::default();
                                    let ty = infer_term(pair[1], &body_env, &mut session);
                                    body_env.set(name.clone(), ty);
                                }
                            }
                        }
                    }
                    let bodies = &items[2..];
                    for (index, body) in bodies.iter().enumerate() {
                        let body_expected =
                            (index + 1 == bodies.len()).then_some(expected).flatten();
                        collect_typed_call_effects(out, body, &body_env, body_expected);
                    }
                }
                SpecialForm::Prim => {
                    for argument in items.iter().skip(2) {
                        collect_typed_call_effects(out, argument, env, None);
                    }
                }
                SpecialForm::Seal | SpecialForm::Unseal => {
                    for argument in items.iter().skip(1) {
                        collect_typed_call_effects(out, argument, env, None);
                    }
                }
            }
            return;
        }

        if let Some((head, args)) = flatten_app(t) {
            if let Term::Symbol(symbol) = &head
                && env.contains(symbol)
            {
                collect_application_type_effects(out, env.get(symbol), &args, env);
            }
            collect_typed_call_effects(out, &head, env, None);
            for argument in args {
                collect_typed_call_effects(out, &argument, env, None);
            }
            return;
        }

        for item in items {
            collect_typed_call_effects(out, item, env, None);
        }
        return;
    }

    if let Term::Map(entries) = t {
        for value in entries.values() {
            collect_typed_call_effects(out, value, env, None);
        }
    }
}

fn collect_application_type_effects(
    out: &mut InferredEffects,
    head_ty: Option<&Ty>,
    args: &[Term],
    env: &TypeEnv,
) {
    let Some(mut current) = head_ty.cloned() else {
        return;
    };
    for argument in args {
        match current {
            Ty::Fn { param, ret, eff } => {
                let mut session = InferSession::default();
                let argument_ty = infer_term(argument, env, &mut session);
                let mut bindings = EffectRowBindings::default();
                if !arg_type_match(&argument_ty, &param, &mut bindings) {
                    out.unknown = true;
                    return;
                }
                add_effect_row(out, &bindings.apply_row(&eff));
                current = bindings.apply_type(&ret);
            }
            Ty::Any => {
                out.unknown = true;
                return;
            }
            _ => return,
        }
    }
    add_reachable_type_effects(out, &current);
}

fn add_reachable_type_effects(out: &mut InferredEffects, ty: &Ty) {
    match ty {
        Ty::Fn { ret, eff, .. } => {
            add_effect_row(out, eff);
            add_reachable_type_effects(out, ret);
        }
        Ty::Prog { ret, eff } => {
            add_effect_row(out, eff);
            add_reachable_type_effects(out, ret);
        }
        Ty::Msg { payload, .. } => add_reachable_type_effects(out, payload),
        Ty::Rec { fields, .. } => {
            for field in fields.values() {
                add_reachable_type_effects(out, field);
            }
        }
        Ty::Contract { methods, .. } => {
            for method in methods.values() {
                add_reachable_type_effects(out, method);
            }
        }
        Ty::Any | Ty::Int | Ty::Bool | Ty::Nil | Ty::Str | Ty::Bytes | Ty::Symbol => {}
    }
}

fn add_effect_row(out: &mut InferredEffects, row: &EffRow) {
    out.ops.extend(row.ops.iter().cloned());
    out.unknown |= matches!(row.tail, RowTail::Any);
}

fn merge_inferred_effects(out: &mut InferredEffects, incoming: InferredEffects) {
    out.ops.extend(incoming.ops);
    out.unknown |= incoming.unknown;
}
