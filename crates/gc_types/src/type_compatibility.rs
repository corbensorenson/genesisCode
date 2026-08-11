use std::collections::BTreeSet;

use crate::ty::{EffRow, EffectRowBindings, RowTail, Ty};

pub(super) fn declared_eff_row(ty: &Ty) -> Option<&EffRow> {
    match ty {
        Ty::Fn { eff, .. } => Some(eff),
        Ty::Prog { eff, .. } => Some(eff),
        _ => None,
    }
}

pub(super) fn validate_effect_row_declaration(ty: &Ty, strict_effects: bool) -> Result<(), String> {
    let mut variables = BTreeSet::new();
    let mut has_anonymous_tail = false;
    collect_effect_row_declaration(ty, &mut variables, &mut has_anonymous_tail);

    if strict_effects && has_anonymous_tail {
        return Err(
            "strict effect mode requires a closed declared effect row or a named row variable bound by a function parameter"
                .to_string(),
        );
    }

    let mut parameter_variables = BTreeSet::new();
    if let Ty::Fn { param, .. } = ty {
        let mut ignored_anonymous_tail = false;
        collect_effect_row_declaration(
            param,
            &mut parameter_variables,
            &mut ignored_anonymous_tail,
        );
    }
    let unbound: Vec<String> = variables
        .difference(&parameter_variables)
        .cloned()
        .collect();
    if !unbound.is_empty() {
        return Err(format!(
            "unbound effect row variable(s) {}; named tails must occur in the outermost function parameter",
            unbound.join(", ")
        ));
    }
    Ok(())
}

fn collect_effect_row_declaration(
    ty: &Ty,
    variables: &mut BTreeSet<String>,
    has_anonymous_tail: &mut bool,
) {
    match ty {
        Ty::Fn { param, ret, eff } => {
            collect_effect_tail(&eff.tail, variables, has_anonymous_tail);
            collect_effect_row_declaration(param, variables, has_anonymous_tail);
            collect_effect_row_declaration(ret, variables, has_anonymous_tail);
        }
        Ty::Prog { ret, eff } => {
            collect_effect_tail(&eff.tail, variables, has_anonymous_tail);
            collect_effect_row_declaration(ret, variables, has_anonymous_tail);
        }
        Ty::Msg { payload, .. } => {
            collect_effect_row_declaration(payload, variables, has_anonymous_tail)
        }
        Ty::Rec { fields, .. } => {
            for field in fields.values() {
                collect_effect_row_declaration(field, variables, has_anonymous_tail);
            }
        }
        Ty::Contract { methods, .. } => {
            for method in methods.values() {
                collect_effect_row_declaration(method, variables, has_anonymous_tail);
            }
        }
        Ty::Any | Ty::Int | Ty::Dec | Ty::Bool | Ty::Nil | Ty::Str | Ty::Bytes | Ty::Symbol => {}
    }
}

fn collect_effect_tail(
    tail: &RowTail,
    variables: &mut BTreeSet<String>,
    has_anonymous_tail: &mut bool,
) {
    match tail {
        RowTail::Closed => {}
        RowTail::Any => *has_anonymous_tail = true,
        RowTail::Var(variable) => {
            variables.insert(variable.clone());
        }
    }
}

pub(super) fn has_unresolved_contract_ops(ty: &Ty) -> bool {
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

pub(super) fn type_compatible(inferred: &Ty, declared: &Ty, strict_shapes: bool) -> bool {
    let mut bindings = EffectRowBindings::default();
    if !bind_declared_effect_rows(inferred, declared, &mut bindings) {
        return false;
    }
    is_subtype(inferred, &bindings.apply_type(declared), strict_shapes)
}

fn bind_declared_effect_rows(
    inferred: &Ty,
    declared: &Ty,
    bindings: &mut EffectRowBindings,
) -> bool {
    if matches!(inferred, Ty::Any) {
        bindings.bind_unknowns_in_type(declared);
        return true;
    }
    match (inferred, declared) {
        (_, Ty::Any) => true,
        (Ty::Msg { payload: ip, .. }, Ty::Msg { payload: dp, .. }) => {
            bind_declared_effect_rows(ip, dp, bindings)
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
            if !bind_declared_effect_rows(ip, dp, bindings) {
                return false;
            }
            if !bind_declared_effect_rows(ir, dr, bindings) {
                return false;
            }
            bind_declared_effect_row(ie, de, bindings)
        }
        (Ty::Prog { ret: ir, eff: ie }, Ty::Prog { ret: dr, eff: de }) => {
            bind_declared_effect_rows(ir, dr, bindings)
                && bind_declared_effect_row(ie, de, bindings)
        }
        (Ty::Rec { fields: ifs, .. }, Ty::Rec { fields: dfs, .. })
        | (Ty::Contract { methods: ifs, .. }, Ty::Contract { methods: dfs, .. }) => {
            dfs.iter().all(|(name, declared_field)| {
                ifs.get(name).is_none_or(|inferred_field| {
                    bind_declared_effect_rows(inferred_field, declared_field, bindings)
                })
            })
        }
        _ => true,
    }
}

fn bind_declared_effect_row(
    inferred: &EffRow,
    declared: &EffRow,
    bindings: &mut EffectRowBindings,
) -> bool {
    if matches!(declared.tail, RowTail::Var(_)) {
        bindings.match_row(inferred, declared)
    } else {
        true
    }
}

fn is_subtype(actual: &Ty, expected: &Ty, strict_shapes: bool) -> bool {
    if matches!(expected, Ty::Any) {
        return true;
    }
    match (actual, expected) {
        (Ty::Any, _) => false,
        (Ty::Int, Ty::Int)
        | (Ty::Dec, Ty::Dec)
        | (Ty::Bool, Ty::Bool)
        | (Ty::Nil, Ty::Nil)
        | (Ty::Str, Ty::Str)
        | (Ty::Bytes, Ty::Bytes)
        | (Ty::Symbol, Ty::Symbol) => true,
        (
            Ty::Msg {
                op: actual_op,
                payload: actual_payload,
            },
            Ty::Msg {
                op: expected_op,
                payload: expected_payload,
            },
        ) => {
            expected_op
                .as_ref()
                .is_none_or(|op| actual_op.as_ref() == Some(op))
                && is_subtype(actual_payload, expected_payload, strict_shapes)
        }
        (
            Ty::Fn {
                param: actual_param,
                ret: actual_ret,
                eff: actual_eff,
            },
            Ty::Fn {
                param: expected_param,
                ret: expected_ret,
                eff: expected_eff,
            },
        ) => {
            is_subtype(expected_param, actual_param, strict_shapes)
                && is_subtype(actual_ret, expected_ret, strict_shapes)
                && effect_row_is_subtype(actual_eff, expected_eff)
        }
        (
            Ty::Prog {
                ret: actual_ret,
                eff: actual_eff,
            },
            Ty::Prog {
                ret: expected_ret,
                eff: expected_eff,
            },
        ) => {
            is_subtype(actual_ret, expected_ret, strict_shapes)
                && effect_row_is_subtype(actual_eff, expected_eff)
        }
        (
            Ty::Rec {
                fields: actual_fields,
                tail: actual_tail,
            },
            Ty::Rec {
                fields: expected_fields,
                tail: expected_tail,
            },
        ) => {
            expected_fields.iter().all(|(name, expected_field)| {
                actual_fields.get(name).is_some_and(|actual_field| {
                    is_subtype(actual_field, expected_field, strict_shapes)
                })
            }) && shape_tail_is_compatible(
                actual_fields.len(),
                actual_tail,
                expected_fields.len(),
                expected_tail,
                strict_shapes,
            )
        }
        (
            Ty::Contract {
                methods: actual_fields,
                tail: actual_tail,
            },
            Ty::Contract {
                methods: expected_fields,
                tail: expected_tail,
            },
        ) => {
            expected_fields.iter().all(|(name, expected_field)| {
                actual_fields.get(name).is_some_and(|actual_field| {
                    contract_method_is_subtype(name, actual_field, expected_field, strict_shapes)
                })
            }) && shape_tail_is_compatible(
                actual_fields.len(),
                actual_tail,
                expected_fields.len(),
                expected_tail,
                strict_shapes,
            )
        }
        _ => false,
    }
}

fn contract_method_is_subtype(
    operation: &str,
    actual: &Ty,
    expected: &Ty,
    strict_shapes: bool,
) -> bool {
    fn bind_operation(ty: &Ty, operation: &str) -> Ty {
        let Ty::Fn { param, ret, eff } = ty else {
            return ty.clone();
        };
        let param = match param.as_ref() {
            Ty::Msg { op: None, payload } => Ty::Msg {
                op: Some(operation.to_string()),
                payload: payload.clone(),
            },
            other => other.clone(),
        };
        Ty::Fn {
            param: Box::new(param),
            ret: ret.clone(),
            eff: eff.clone(),
        }
    }

    is_subtype(
        &bind_operation(actual, operation),
        &bind_operation(expected, operation),
        strict_shapes,
    )
}

fn shape_tail_is_compatible(
    actual_len: usize,
    actual_tail: &RowTail,
    expected_len: usize,
    expected_tail: &RowTail,
    strict_shapes: bool,
) -> bool {
    !strict_shapes
        || !matches!(expected_tail, RowTail::Closed)
        || (matches!(actual_tail, RowTail::Closed) && actual_len == expected_len)
}

fn effect_row_is_subtype(actual: &EffRow, expected: &EffRow) -> bool {
    match &expected.tail {
        RowTail::Any => true,
        RowTail::Closed => {
            matches!(actual.tail, RowTail::Closed) && actual.ops.is_subset(&expected.ops)
        }
        RowTail::Var(expected_name) => {
            matches!(&actual.tail, RowTail::Var(actual_name) if actual_name == expected_name)
                && actual.ops.is_subset(&expected.ops)
        }
    }
}
