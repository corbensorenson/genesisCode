use std::collections::BTreeMap;

use gc_coreform::{Term, hash_term};

use super::{Env, EvalCtx, EvalOutcome, KernelError, KernelErrorKind, Value, eval_forms};
use crate::value::Apply;

fn hash_hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        out.push(HEX[(b >> 4) as usize] as char);
        out.push(HEX[(b & 0x0f) as usize] as char);
    }
    out
}

fn treewalk_site_id(kind: &str, term: &Term) -> String {
    let h = hash_term(term);
    format!("{kind}:{}", hash_hex(&h))
}

pub(super) fn eval_term_impl(
    ctx: &mut EvalCtx,
    env: &Env,
    term: &Term,
) -> Result<Value, KernelError> {
    // Implement a small tail-call optimization for:
    // - (if ...) branches
    // - (begin ...) last form
    // - general application where the final apply is a closure call
    //
    // This makes typical tail-recursive CoreForm code stack-safe without changing semantics.
    let mut cur_env = env.clone();
    let mut cur_term = term.clone();
    loop {
        ctx.tick()?;

        match &cur_term {
            Term::Nil | Term::Bool(_) | Term::Int(_) => return Ok(Value::Data(cur_term.clone())),
            Term::Str(s) => {
                ctx.mem_observe_string_len(s.len())?;
                return Ok(Value::Data(cur_term.clone()));
            }
            Term::Bytes(b) => {
                ctx.mem_observe_bytes_len(b.len())?;
                return Ok(Value::Data(cur_term.clone()));
            }
            Term::Vector(xs) => {
                ctx.mem_observe_vec_len(xs.len())?;
                for x in xs {
                    ctx.mem_observe_data_term(x)?;
                }
                return Ok(Value::Vector(xs.iter().cloned().map(Value::Data).collect()));
            }
            Term::Map(m) => {
                // Map literal: keys are data terms (not evaluated), values are expressions (evaluated).
                ctx.mem_observe_map_len(m.len())?;
                for (k, _v) in m.iter() {
                    ctx.mem_observe_data_term(&k.0)?;
                }
                let mut out = BTreeMap::new();
                for (k, v) in m.iter() {
                    let vv = super::eval_term(ctx, &cur_env, v)?;
                    out.insert(k.clone(), vv);
                }
                return Ok(Value::Map(out));
            }
            Term::Symbol(s) => {
                let value = cur_env.get(s).ok_or_else(|| {
                    KernelError::new(KernelErrorKind::Unbound, format!("unbound symbol: {s}"))
                })?;
                ctx.coverage_statement_site(&treewalk_site_id("stmt", &cur_term));
                ctx.coverage_hit(s, &value);
                return Ok(value);
            }
            Term::Pair(_, _) => match eval_list_tco(ctx, &cur_env, &cur_term)? {
                EvalOutcome::Value(v) => return Ok(v),
                EvalOutcome::Tail { env, term } => {
                    cur_env = env;
                    cur_term = term;
                }
            },
        }
    }
}

fn eval_list_tco(ctx: &mut EvalCtx, env: &Env, t: &Term) -> Result<EvalOutcome, KernelError> {
    let Some(items) = t.as_proper_list() else {
        return Err(KernelError::new(
            KernelErrorKind::BadForm,
            "improper list is not a valid form",
        ));
    };
    if items.is_empty() {
        return Ok(EvalOutcome::Value(Value::Data(Term::Nil)));
    }

    // Special forms keyed by head symbol.
    if let Term::Symbol(h) = items[0] {
        match h.as_str() {
            "quote" => {
                if items.len() != 2 {
                    return Err(KernelError::new(
                        KernelErrorKind::BadForm,
                        "(quote datum) expects exactly 1 argument",
                    ));
                }
                ctx.mem_observe_data_term(items[1])?;
                return Ok(EvalOutcome::Value(Value::Data(items[1].clone())));
            }
            "fn" => {
                return Ok(EvalOutcome::Value(eval_forms::eval_fn(ctx, env, items)?));
            }
            "if" => {
                if items.len() != 4 {
                    return Err(KernelError::new(
                        KernelErrorKind::BadForm,
                        "(if c t e) expects exactly 3 arguments",
                    ));
                }
                ctx.coverage_begin_decision_site(&treewalk_site_id("decision", t));
                let c = match super::eval_term(ctx, env, items[1]) {
                    Ok(v) => v,
                    Err(e) => {
                        ctx.coverage_abort_decision_site();
                        return Err(e);
                    }
                };
                let cond_truthy = c.truthy();
                ctx.coverage_finish_decision_site(cond_truthy);
                let next = if cond_truthy { items[2] } else { items[3] };
                return Ok(EvalOutcome::Tail {
                    env: env.clone(),
                    term: next.clone(),
                });
            }
            "begin" => {
                if items.len() < 2 {
                    return Err(KernelError::new(
                        KernelErrorKind::BadForm,
                        "(begin ...) expects at least 1 argument",
                    ));
                }
                if items.len() == 2 {
                    return Ok(EvalOutcome::Tail {
                        env: env.clone(),
                        term: items[1].clone(),
                    });
                }
                for e in items.iter().skip(1).take(items.len() - 2) {
                    let _ = super::eval_term(ctx, env, e)?;
                }
                return Ok(EvalOutcome::Tail {
                    env: env.clone(),
                    term: items[items.len() - 1].clone(),
                });
            }
            "let" => {
                return eval_forms::eval_let_tco(ctx, env, items);
            }
            "prim" => {
                return Ok(EvalOutcome::Value(eval_forms::eval_prim(ctx, env, items)?));
            }
            "seal" => {
                return Ok(EvalOutcome::Value(eval_forms::eval_seal(ctx, env, items)?));
            }
            "unseal" => {
                return Ok(EvalOutcome::Value(eval_forms::eval_unseal(
                    ctx, env, items,
                )?));
            }
            "def" => {
                return Err(KernelError::new(
                    KernelErrorKind::BadForm,
                    "(def ...) is only allowed at module top-level",
                ));
            }
            _ => {}
        }
    }

    // General application (supports sugar forms with more than one argument).
    let f = super::eval_term(ctx, env, items[0])?;
    if items.len() == 1 {
        return Ok(EvalOutcome::Value(f));
    }

    // Apply all but the final argument normally.
    let mut acc = f;
    for a in items.iter().skip(1).take(items.len() - 2) {
        let av = super::eval_term(ctx, env, a)?;
        acc = acc.apply(ctx, av)?;
    }

    // Tail-call optimize the final apply when it is a closure call.
    let last_arg = super::eval_term(ctx, env, items[items.len() - 1])?;
    match acc {
        Value::Closure {
            param,
            body,
            env: fenv,
        } => Ok(EvalOutcome::Tail {
            env: Env::with_binding(&fenv, param, last_arg),
            term: body,
        }),
        other => Ok(EvalOutcome::Value(other.apply(ctx, last_arg)?)),
    }
}
