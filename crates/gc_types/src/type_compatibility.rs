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
    type_compatible_with_bindings(
        inferred,
        declared,
        strict_shapes,
        &mut EffectRowBindings::default(),
    )
}

fn type_compatible_with_bindings(
    inferred: &Ty,
    declared: &Ty,
    strict_shapes: bool,
    bindings: &mut EffectRowBindings,
) -> bool {
    // `?` in the declared position accepts anything.
    if matches!(declared, Ty::Any) {
        return true;
    }
    match (inferred, declared) {
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
            type_compatible_with_bindings(ip, dp, strict_shapes, bindings)
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
            if !type_compatible_with_bindings(ip, dp, strict_shapes, bindings) {
                return false;
            }
            if !type_compatible_with_bindings(ir, dr, strict_shapes, bindings) {
                return false;
            }
            bindings.match_row(ie, de)
        }
        (Ty::Prog { ret: ir, eff: ie }, Ty::Prog { ret: dr, eff: de }) => {
            type_compatible_with_bindings(ir, dr, strict_shapes, bindings)
                && bindings.match_row(ie, de)
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
                ifs.get(k).is_some_and(|it| {
                    type_compatible_with_bindings(it, dt, strict_shapes, bindings)
                })
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
                ims.get(k).is_some_and(|it| {
                    type_compatible_with_bindings(it, dt, strict_shapes, bindings)
                })
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
