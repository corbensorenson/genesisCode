use gc_coreform::print_term;

use crate::ty::{EffectRowBindings, Ty};

use super::InferSession;

pub(super) fn infer_apply_types(head: Ty, args: &[Ty], sess: &mut InferSession) -> Option<Ty> {
    let mut cur = head;
    for arg in args {
        cur = apply_once(cur, arg, sess)?;
    }
    Some(cur)
}

fn apply_once(f_ty: Ty, arg_ty: &Ty, sess: &mut InferSession) -> Option<Ty> {
    match f_ty {
        Ty::Fn { param, ret, eff: _ } => {
            let mut bindings = EffectRowBindings::default();
            if !arg_type_match(arg_ty, &param, &mut bindings) {
                sess.errors.push(format!(
                    "application arg type mismatch: expected {}, got {}",
                    print_term(&param.to_term()),
                    print_term(&arg_ty.to_term())
                ));
                return Some(Ty::Any);
            }
            Some(bindings.apply_type(&ret))
        }
        Ty::Any => Some(Ty::Any),
        _ => None,
    }
}

pub(super) fn arg_type_compatible(inferred: &Ty, declared: &Ty) -> bool {
    arg_type_match(inferred, declared, &mut EffectRowBindings::default())
}

pub(super) fn arg_type_match(
    inferred: &Ty,
    declared: &Ty,
    bindings: &mut EffectRowBindings,
) -> bool {
    if matches!(declared, Ty::Any) {
        return true;
    }
    if matches!(inferred, Ty::Any) {
        bindings.bind_unknowns_in_type(declared);
        return true;
    }
    match (inferred, declared) {
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
            arg_type_match(ip, dp, bindings)
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
            arg_type_match(ip, dp, bindings)
                && arg_type_match(ir, dr, bindings)
                && bindings.match_row(ie, de)
        }
        (Ty::Prog { ret: ir, eff: ie }, Ty::Prog { ret: dr, eff: de }) => {
            arg_type_match(ir, dr, bindings) && bindings.match_row(ie, de)
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
        ) => dfs.iter().all(|(k, dt)| {
            ifs.get(k)
                .is_some_and(|it| arg_type_match(it, dt, bindings))
        }),
        (
            Ty::Contract {
                methods: ims,
                tail: _,
            },
            Ty::Contract {
                methods: dms,
                tail: _,
            },
        ) => dms.iter().all(|(k, dt)| {
            ims.get(k)
                .is_some_and(|it| arg_type_match(it, dt, bindings))
        }),
        _ => false,
    }
}
