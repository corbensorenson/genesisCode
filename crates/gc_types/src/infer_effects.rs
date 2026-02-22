use std::collections::BTreeSet;

use gc_coreform::{Term, print_term};

use crate::ty::{EffRow, RowTail, Ty};

use super::{InferSession, TypeEnv, infer_term, literal_op_symbol, merge_eff_rows};

pub(super) fn infer_core_effect_pure(args: &[Term], env: &TypeEnv, sess: &mut InferSession) -> Ty {
    if args.len() != 1 {
        sess.errors.push(format!(
            "core/effect::pure expects 1 arg, got {}",
            args.len()
        ));
        return Ty::Any;
    }
    let ret = infer_term(&args[0], env, sess);
    Ty::Prog {
        ret: Box::new(ret),
        eff: EffRow::empty(),
    }
}

pub(super) fn infer_core_effect_bind(args: &[Term], env: &TypeEnv, sess: &mut InferSession) -> Ty {
    if args.len() != 2 {
        sess.errors.push(format!(
            "core/effect::bind expects 2 args (prog k), got {}",
            args.len()
        ));
        return Ty::Any;
    }
    let p_ty = infer_term(&args[0], env, sess);

    let (p_ret, mut p_eff) = match p_ty {
        Ty::Prog { ret, eff } => (ret, eff),
        Ty::Any => {
            return Ty::Prog {
                ret: Box::new(Ty::Any),
                eff: EffRow {
                    ops: BTreeSet::new(),
                    tail: RowTail::Any,
                },
            };
        }
        _ => {
            sess.errors
                .push("core/effect::bind first arg must be a Prog".to_string());
            return Ty::Any;
        }
    };

    let k_ty = infer_bind_continuation_with_param(&args[1], &p_ret, env, sess);
    let (k_param, k_ret, k_fn_eff) = match k_ty {
        Ty::Fn { param, ret, eff } => (param, ret, eff),
        Ty::Any => {
            return Ty::Prog {
                ret: Box::new(Ty::Any),
                eff: EffRow {
                    ops: BTreeSet::new(),
                    tail: RowTail::Any,
                },
            };
        }
        _ => {
            sess.errors
                .push("core/effect::bind continuation must be a function".to_string());
            return Ty::Any;
        }
    };

    if !super::arg_type_compatible(&p_ret, &k_param) {
        sess.errors.push(format!(
            "core/effect::bind continuation param mismatch; prog returns {}, continuation expects {}",
            print_term(&p_ret.to_term()),
            print_term(&k_param.to_term())
        ));
    }

    let (ret, k_prog_eff) = match *k_ret {
        Ty::Prog { ret, eff } => (ret, eff),
        Ty::Any => (
            Box::new(Ty::Any),
            EffRow {
                ops: BTreeSet::new(),
                tail: RowTail::Any,
            },
        ),
        other => {
            sess.errors.push(format!(
                "core/effect::bind continuation must return Prog, got {}",
                print_term(&other.to_term())
            ));
            (
                Box::new(Ty::Any),
                EffRow {
                    ops: BTreeSet::new(),
                    tail: RowTail::Any,
                },
            )
        }
    };

    p_eff = merge_eff_rows(p_eff, &k_fn_eff);
    p_eff = merge_eff_rows(p_eff, &k_prog_eff);
    Ty::Prog { ret, eff: p_eff }
}

fn infer_bind_continuation_with_param(
    continuation: &Term,
    param_ty: &Ty,
    env: &TypeEnv,
    sess: &mut InferSession,
) -> Ty {
    let Some(items) = continuation.as_proper_list() else {
        return infer_term(continuation, env, sess);
    };
    if items.len() < 3 || !matches!(items[0], Term::Symbol(s) if s == "fn") {
        return infer_term(continuation, env, sess);
    }
    let Some(params) = items[1].as_proper_list() else {
        return infer_term(continuation, env, sess);
    };
    if params.len() != 1 {
        return infer_term(continuation, env, sess);
    }
    let Term::Symbol(param_name) = params[0] else {
        return infer_term(continuation, env, sess);
    };
    let body = if items.len() == 3 {
        items[2].clone()
    } else {
        let mut xs = Vec::new();
        xs.push(Term::Symbol("begin".to_string()));
        for b in items.iter().skip(2) {
            xs.push((*b).clone());
        }
        Term::list(xs)
    };
    let mut env2 = env.clone();
    env2.set(param_name.clone(), param_ty.clone());
    let ret = infer_term(&body, &env2, sess);
    let inf = crate::infer_effects_in_term(&body);
    let tail = if inf.unknown {
        RowTail::Any
    } else {
        RowTail::Closed
    };
    Ty::Fn {
        param: Box::new(param_ty.clone()),
        ret: Box::new(ret),
        eff: EffRow { ops: inf.ops, tail },
    }
}

pub(super) fn infer_core_effect_perform(
    args: &[Term],
    env: &TypeEnv,
    sess: &mut InferSession,
) -> Ty {
    if args.len() != 3 {
        sess.errors.push(format!(
            "core/effect::perform expects 3 args (op payload k), got {}",
            args.len()
        ));
        return Ty::Any;
    }
    let op = literal_op_symbol(&args[0]);
    let _payload = infer_term(&args[1], env, sess);
    let k_ty = infer_term(&args[2], env, sess);
    let (k_ret, mut eff) = match k_ty {
        Ty::Fn { ret, eff, .. } => (ret, eff),
        Ty::Any => (
            Box::new(Ty::Any),
            EffRow {
                ops: BTreeSet::new(),
                tail: RowTail::Any,
            },
        ),
        _ => {
            sess.errors
                .push("core/effect::perform k must be a function".to_string());
            (
                Box::new(Ty::Any),
                EffRow {
                    ops: BTreeSet::new(),
                    tail: RowTail::Any,
                },
            )
        }
    };

    let (ret, k_eff) = match *k_ret {
        Ty::Prog { ret, eff: keff } => (ret, keff),
        Ty::Any => (
            Box::new(Ty::Any),
            EffRow {
                ops: BTreeSet::new(),
                tail: RowTail::Any,
            },
        ),
        other => {
            sess.errors.push(format!(
                "core/effect::perform continuation must return a Prog, got {}",
                print_term(&other.to_term())
            ));
            (
                Box::new(Ty::Any),
                EffRow {
                    ops: BTreeSet::new(),
                    tail: RowTail::Any,
                },
            )
        }
    };

    eff.ops.extend(k_eff.ops);
    if k_eff.tail.is_open() {
        eff.tail = k_eff.tail;
    }

    if let Some(op) = op {
        eff.ops.insert(op);
    } else {
        eff.tail = RowTail::Any;
    }

    Ty::Prog { ret, eff }
}
