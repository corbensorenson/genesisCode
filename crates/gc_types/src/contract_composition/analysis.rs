use std::collections::BTreeMap;

use gc_coreform::Term;

use super::{OptimizationPreconditions, contains_anonymous_row, contains_gradual_type};
use crate::ty::{EffRow, RowTail, Ty};

pub(super) struct TypeAnalysis {
    pub(super) normalized_type: Term,
    pub(super) effect_row_variables: Vec<String>,
    pub(super) optimization: OptimizationPreconditions,
}

#[derive(Default)]
struct AlphaNames {
    effects: BTreeMap<String, String>,
}

pub(super) fn analyze_type(ty: &Ty, refinement_free: bool) -> TypeAnalysis {
    let mut names = AlphaNames::default();
    let normalized_type = normalized_type_term(ty, &mut names);
    let effect_row_variables = names.effects.keys().cloned().collect();
    TypeAnalysis {
        normalized_type,
        effect_row_variables,
        optimization: optimization_preconditions(ty, refinement_free),
    }
}

fn normalized_type_term(ty: &Ty, names: &mut AlphaNames) -> Term {
    match ty {
        Ty::Any => Term::symbol("?"),
        Ty::Int => Term::symbol("Int"),
        Ty::Dec => Term::symbol("Dec"),
        Ty::Bool => Term::symbol("Bool"),
        Ty::Nil => Term::symbol("Nil"),
        Ty::Str => Term::symbol("Str"),
        Ty::Bytes => Term::symbol("Bytes"),
        Ty::Symbol => Term::symbol("Symbol"),
        Ty::Msg { payload, .. } => Term::list(vec![
            Term::symbol("Msg"),
            normalized_type_term(payload, names),
        ]),
        Ty::Fn { param, ret, eff } => Term::list(vec![
            Term::symbol("Fn"),
            normalized_type_term(param, names),
            normalized_type_term(ret, names),
            normalized_effect_term(eff, names),
        ]),
        Ty::Prog { ret, eff } => Term::list(vec![
            Term::symbol("Prog"),
            normalized_type_term(ret, names),
            normalized_effect_term(eff, names),
        ]),
        Ty::Rec { fields, tail } => normalized_shape_row("Rec", fields, tail, names),
        Ty::Contract { methods, tail } => normalized_shape_row("Contract", methods, tail, names),
    }
}

fn normalized_shape_row(
    head: &str,
    fields: &BTreeMap<String, Ty>,
    tail: &RowTail,
    names: &mut AlphaNames,
) -> Term {
    let entries = fields
        .iter()
        .map(|(name, ty)| {
            Term::Vector(vec![
                Term::symbol(name.clone()),
                normalized_type_term(ty, names),
            ])
        })
        .collect();
    let normalized_tail = match tail {
        RowTail::Closed => Term::Nil,
        RowTail::Any => Term::symbol("?"),
        RowTail::Var(_) => Term::symbol("shape-open"),
    };
    Term::list(vec![
        Term::symbol(head),
        Term::Vector(entries),
        normalized_tail,
    ])
}

fn normalized_effect_term(eff: &EffRow, names: &mut AlphaNames) -> Term {
    let tail = match &eff.tail {
        RowTail::Closed => Term::Nil,
        RowTail::Any => Term::symbol("?"),
        RowTail::Var(name) => {
            let next = names.effects.len();
            let normalized = names
                .effects
                .entry(name.clone())
                .or_insert_with(|| format!("effect-row-{next}"));
            Term::symbol(normalized.clone())
        }
    };
    Term::list(vec![
        Term::symbol("Eff"),
        Term::Vector(eff.ops.iter().cloned().map(Term::Symbol).collect()),
        tail,
    ])
}

fn optimization_preconditions(ty: &Ty, refinement_free: bool) -> OptimizationPreconditions {
    let concrete = !contains_gradual_type(ty) && !contains_anonymous_row(ty);
    let closed_shapes = shapes_closed(ty);
    let closed_effects = effects_closed(ty);
    let pure = closed_effects && effects_empty(ty);
    let contract_free = !contains_contract(ty);
    let monomorphic = !contains_effect_variable(ty) && closed_shapes;
    let eligible = concrete
        && closed_shapes
        && closed_effects
        && pure
        && refinement_free
        && contract_free
        && monomorphic;
    OptimizationPreconditions {
        concrete,
        closed_shapes,
        closed_effects,
        pure,
        refinement_free,
        contract_free,
        monomorphic,
        eligible,
    }
}

fn shapes_closed(ty: &Ty) -> bool {
    match ty {
        Ty::Rec { fields, tail } => {
            matches!(tail, RowTail::Closed) && fields.values().all(shapes_closed)
        }
        Ty::Contract { methods, tail } => {
            matches!(tail, RowTail::Closed) && methods.values().all(shapes_closed)
        }
        Ty::Fn { param, ret, .. } => shapes_closed(param) && shapes_closed(ret),
        Ty::Prog { ret, .. } | Ty::Msg { payload: ret, .. } => shapes_closed(ret),
        Ty::Any | Ty::Int | Ty::Dec | Ty::Bool | Ty::Nil | Ty::Str | Ty::Bytes | Ty::Symbol => true,
    }
}

fn effects_closed(ty: &Ty) -> bool {
    match ty {
        Ty::Fn { param, ret, eff } => {
            matches!(eff.tail, RowTail::Closed) && effects_closed(param) && effects_closed(ret)
        }
        Ty::Prog { ret, eff } => matches!(eff.tail, RowTail::Closed) && effects_closed(ret),
        Ty::Msg { payload, .. } => effects_closed(payload),
        Ty::Rec { fields, .. } => fields.values().all(effects_closed),
        Ty::Contract { methods, .. } => methods.values().all(effects_closed),
        Ty::Any | Ty::Int | Ty::Dec | Ty::Bool | Ty::Nil | Ty::Str | Ty::Bytes | Ty::Symbol => true,
    }
}

fn effects_empty(ty: &Ty) -> bool {
    match ty {
        Ty::Fn { param, ret, eff } => {
            eff.ops.is_empty() && effects_empty(param) && effects_empty(ret)
        }
        Ty::Prog { ret, eff } => eff.ops.is_empty() && effects_empty(ret),
        Ty::Msg { payload, .. } => effects_empty(payload),
        Ty::Rec { fields, .. } => fields.values().all(effects_empty),
        Ty::Contract { methods, .. } => methods.values().all(effects_empty),
        Ty::Any | Ty::Int | Ty::Dec | Ty::Bool | Ty::Nil | Ty::Str | Ty::Bytes | Ty::Symbol => true,
    }
}

fn contains_contract(ty: &Ty) -> bool {
    match ty {
        Ty::Contract { .. } => true,
        Ty::Fn { param, ret, .. } => contains_contract(param) || contains_contract(ret),
        Ty::Prog { ret, .. } | Ty::Msg { payload: ret, .. } => contains_contract(ret),
        Ty::Rec { fields, .. } => fields.values().any(contains_contract),
        Ty::Any | Ty::Int | Ty::Dec | Ty::Bool | Ty::Nil | Ty::Str | Ty::Bytes | Ty::Symbol => {
            false
        }
    }
}

fn contains_effect_variable(ty: &Ty) -> bool {
    match ty {
        Ty::Fn { param, ret, eff } => {
            matches!(eff.tail, RowTail::Var(_))
                || contains_effect_variable(param)
                || contains_effect_variable(ret)
        }
        Ty::Prog { ret, eff } => {
            matches!(eff.tail, RowTail::Var(_)) || contains_effect_variable(ret)
        }
        Ty::Msg { payload, .. } => contains_effect_variable(payload),
        Ty::Rec { fields, .. } => fields.values().any(contains_effect_variable),
        Ty::Contract { methods, .. } => methods.values().any(contains_effect_variable),
        Ty::Any | Ty::Int | Ty::Dec | Ty::Bool | Ty::Nil | Ty::Str | Ty::Bytes | Ty::Symbol => {
            false
        }
    }
}
