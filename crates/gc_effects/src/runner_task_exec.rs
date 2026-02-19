use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::Duration;

use gc_coreform::{Term, TermOrdKey, hash_term};
use gc_kernel::{Apply, EvalCtx, Value, eval_term, value_hash};
use gc_prelude::build_prelude;
use num_bigint::BigInt;
use num_traits::ToPrimitive;

use crate::runner_task::TaskOutcome;
use crate::{CapsPolicy, run};

pub(crate) fn execute_task_payload(
    payload: Term,
    cancel_flag: &AtomicBool,
    policy: &CapsPolicy,
) -> TaskOutcome {
    if cancel_flag.load(Ordering::Acquire) {
        return TaskOutcome::Cancelled;
    }
    if let Some(expr) = map_field(&payload, ":task/eval") {
        return execute_task_eval(
            expr.clone(),
            map_field(&payload, ":task/arg").cloned(),
            map_field(&payload, ":task/args").cloned(),
            cancel_flag,
            policy,
        );
    }
    if let Some(program) = map_field(&payload, ":task/program") {
        return execute_task_program(program.clone(), cancel_flag);
    }
    if let Some(ms) = map_field_int_u64(&payload, ":task/sleep-ms")
        .or_else(|| map_field_int_u64(&payload, ":sleep-ms"))
    {
        if sleep_cancelable(ms, cancel_flag) {
            return TaskOutcome::Cancelled;
        }
    }
    if cancel_flag.load(Ordering::Acquire) {
        return TaskOutcome::Cancelled;
    }
    if let Some(err) = map_field(&payload, ":task/error") {
        return TaskOutcome::Failed(err.clone());
    }
    if let Some(result) = map_field(&payload, ":task/result") {
        return TaskOutcome::Done(result.clone());
    }
    TaskOutcome::Done(payload)
}

fn execute_task_eval(
    expr: Term,
    arg: Option<Term>,
    args: Option<Term>,
    cancel_flag: &AtomicBool,
    policy: &CapsPolicy,
) -> TaskOutcome {
    let mut ctx = EvalCtx::new();
    let prelude = build_prelude(&mut ctx);
    let env = prelude.env;
    let expr_hash = hash_term(&expr);

    let mut current = match eval_term(&mut ctx, &env, &expr) {
        Ok(v) => v,
        Err(e) => {
            return TaskOutcome::Failed(task_program_error(
                0,
                format!("task eval failed: {}", e.msg),
            ));
        }
    };

    if let Some(args_t) = args {
        let Term::Vector(xs) = args_t else {
            return TaskOutcome::Failed(task_program_error(
                0,
                "task eval :task/args must be a vector".to_string(),
            ));
        };
        for (idx, x) in xs.into_iter().enumerate() {
            if cancel_flag.load(Ordering::Acquire) {
                return TaskOutcome::Cancelled;
            }
            current = match current.apply(&mut ctx, Value::Data(x)) {
                Ok(v) => v,
                Err(e) => {
                    return TaskOutcome::Failed(task_program_error(
                        idx,
                        format!("task eval apply failed: {}", e.msg),
                    ));
                }
            };
        }
    } else if let Some(arg_t) = arg {
        current = match current.apply(&mut ctx, Value::Data(arg_t)) {
            Ok(v) => v,
            Err(e) => {
                return TaskOutcome::Failed(task_program_error(
                    0,
                    format!("task eval apply failed: {}", e.msg),
                ));
            }
        };
    }

    resolve_task_value(current, expr_hash, cancel_flag, policy, &mut ctx)
}

fn resolve_task_value(
    mut current: Value,
    expr_hash: [u8; 32],
    cancel_flag: &AtomicBool,
    policy: &CapsPolicy,
    ctx: &mut EvalCtx,
) -> TaskOutcome {
    loop {
        if cancel_flag.load(Ordering::Acquire) {
            return TaskOutcome::Cancelled;
        }
        match current {
            Value::EffectProgram(_) => {
                let program_hash = value_hash(&current);
                let out = run(
                    ctx,
                    policy,
                    current,
                    if program_hash == [0u8; 32] {
                        expr_hash
                    } else {
                        program_hash
                    },
                    "gc_effects-task".to_string(),
                );
                let run_out = match out {
                    Ok(v) => v,
                    Err(e) => {
                        return TaskOutcome::Failed(task_program_error(
                            0,
                            format!("task effect run failed: {e}"),
                        ));
                    }
                };
                current = run_out.value;
            }
            Value::Sealed { payload, .. } => match value_to_term(payload.as_ref()) {
                Ok(t) => return TaskOutcome::Failed(t),
                Err(msg) => return TaskOutcome::Failed(task_program_error(0, msg)),
            },
            other => match value_to_term(&other) {
                Ok(t) => return TaskOutcome::Done(t),
                Err(msg) => return TaskOutcome::Failed(task_program_error(0, msg)),
            },
        }
    }
}

fn value_to_term(v: &Value) -> Result<Term, String> {
    match v {
        Value::Data(t) => Ok(t.clone()),
        Value::Vector(xs) => {
            let mut out = Vec::with_capacity(xs.len());
            for x in xs {
                out.push(value_to_term(x)?);
            }
            Ok(Term::Vector(out))
        }
        Value::Map(m) => {
            let mut out = BTreeMap::new();
            for (k, vv) in m {
                out.insert(TermOrdKey(k.0.clone()), value_to_term(vv)?);
            }
            Ok(Term::Map(out))
        }
        Value::Sealed { payload, .. } => value_to_term(payload.as_ref()),
        _ => Err(format!(
            "task evaluation returned non-datum value: {}",
            v.debug_repr()
        )),
    }
}

fn execute_task_program(program: Term, cancel_flag: &AtomicBool) -> TaskOutcome {
    let Term::Map(program_map) = program else {
        return TaskOutcome::Failed(task_program_error(
            0,
            "task program must be a map with :steps vector".to_string(),
        ));
    };
    let steps = match program_map.get(&TermOrdKey(Term::symbol(":steps"))) {
        Some(Term::Vector(v)) => v.clone(),
        Some(Term::Nil) | None => Vec::new(),
        Some(_) => {
            return TaskOutcome::Failed(task_program_error(
                0,
                "task program :steps must be a vector".to_string(),
            ));
        }
    };
    let mut acc = program_map
        .get(&TermOrdKey(Term::symbol(":initial")))
        .cloned()
        .unwrap_or(Term::Nil);
    for (idx, step) in steps.into_iter().enumerate() {
        if cancel_flag.load(Ordering::Acquire) {
            return TaskOutcome::Cancelled;
        }
        let Term::Map(step_map) = step else {
            return TaskOutcome::Failed(task_program_error(idx, "step must be a map".to_string()));
        };
        let Some(op) = map_field_str_or_symbol_map(&step_map, ":op") else {
            return TaskOutcome::Failed(task_program_error(
                idx,
                "step must include :op string or symbol".to_string(),
            ));
        };
        match op.as_str() {
            ":sleep-ms" | "sleep-ms" => {
                let Some(ms) = map_field_int_u64_map(&step_map, ":ms")
                    .or_else(|| map_field_int_u64_map(&step_map, ":value"))
                else {
                    return TaskOutcome::Failed(task_program_error(
                        idx,
                        "sleep-ms step must include :ms int".to_string(),
                    ));
                };
                if sleep_cancelable(ms, cancel_flag) {
                    return TaskOutcome::Cancelled;
                }
            }
            ":set" | "set" => {
                let Some(v) = step_map.get(&TermOrdKey(Term::symbol(":value"))) else {
                    return TaskOutcome::Failed(task_program_error(
                        idx,
                        "set step must include :value".to_string(),
                    ));
                };
                acc = v.clone();
            }
            ":int-add" | "int-add" => {
                let Some(rhs) = map_field_int_bigint_map(&step_map, ":value") else {
                    return TaskOutcome::Failed(task_program_error(
                        idx,
                        "int-add step must include :value int".to_string(),
                    ));
                };
                let Term::Int(lhs) = acc else {
                    return TaskOutcome::Failed(task_program_error(
                        idx,
                        "int-add requires integer accumulator".to_string(),
                    ));
                };
                acc = Term::Int(lhs + rhs);
            }
            ":int-mul" | "int-mul" => {
                let Some(rhs) = map_field_int_bigint_map(&step_map, ":value") else {
                    return TaskOutcome::Failed(task_program_error(
                        idx,
                        "int-mul step must include :value int".to_string(),
                    ));
                };
                let Term::Int(lhs) = acc else {
                    return TaskOutcome::Failed(task_program_error(
                        idx,
                        "int-mul requires integer accumulator".to_string(),
                    ));
                };
                acc = Term::Int(lhs * rhs);
            }
            ":str-append" | "str-append" => {
                let Some(rhs) = map_field_str_map(&step_map, ":value") else {
                    return TaskOutcome::Failed(task_program_error(
                        idx,
                        "str-append step must include :value string".to_string(),
                    ));
                };
                let Term::Str(lhs) = acc else {
                    return TaskOutcome::Failed(task_program_error(
                        idx,
                        "str-append requires string accumulator".to_string(),
                    ));
                };
                acc = Term::Str(format!("{lhs}{rhs}"));
            }
            ":vec-push" | "vec-push" => {
                let Some(v) = step_map.get(&TermOrdKey(Term::symbol(":value"))) else {
                    return TaskOutcome::Failed(task_program_error(
                        idx,
                        "vec-push step must include :value".to_string(),
                    ));
                };
                let Term::Vector(mut xs) = acc else {
                    return TaskOutcome::Failed(task_program_error(
                        idx,
                        "vec-push requires vector accumulator".to_string(),
                    ));
                };
                xs.push(v.clone());
                acc = Term::Vector(xs);
            }
            ":map-put" | "map-put" => {
                let Some(k) = step_map.get(&TermOrdKey(Term::symbol(":key"))) else {
                    return TaskOutcome::Failed(task_program_error(
                        idx,
                        "map-put step must include :key".to_string(),
                    ));
                };
                let Some(v) = step_map.get(&TermOrdKey(Term::symbol(":value"))) else {
                    return TaskOutcome::Failed(task_program_error(
                        idx,
                        "map-put step must include :value".to_string(),
                    ));
                };
                let Term::Map(mut m) = acc else {
                    return TaskOutcome::Failed(task_program_error(
                        idx,
                        "map-put requires map accumulator".to_string(),
                    ));
                };
                m.insert(TermOrdKey(k.clone()), v.clone());
                acc = Term::Map(m);
            }
            ":fail" | "fail" => {
                let err = step_map
                    .get(&TermOrdKey(Term::symbol(":error")))
                    .cloned()
                    .unwrap_or_else(|| {
                        task_program_error(idx, "fail step reached without :error".to_string())
                    });
                return TaskOutcome::Failed(err);
            }
            ":return" | "return" => {
                let out = step_map
                    .get(&TermOrdKey(Term::symbol(":value")))
                    .cloned()
                    .unwrap_or(acc);
                return TaskOutcome::Done(out);
            }
            ":yield" | "yield" => {}
            _ => {
                return TaskOutcome::Failed(task_program_error(
                    idx,
                    format!("unsupported task step op: {op}"),
                ));
            }
        }
    }
    TaskOutcome::Done(acc)
}

fn sleep_cancelable(ms: u64, cancel_flag: &AtomicBool) -> bool {
    let mut remaining = ms;
    while remaining > 0 {
        if cancel_flag.load(Ordering::Acquire) {
            return true;
        }
        let chunk = remaining.min(10);
        thread::sleep(Duration::from_millis(chunk));
        remaining = remaining.saturating_sub(chunk);
    }
    cancel_flag.load(Ordering::Acquire)
}

fn map_field<'a>(t: &'a Term, key: &str) -> Option<&'a Term> {
    let Term::Map(m) = t else {
        return None;
    };
    m.get(&TermOrdKey(Term::symbol(key)))
}

fn map_field_int_u64(t: &Term, key: &str) -> Option<u64> {
    match map_field(t, key) {
        Some(Term::Int(i)) if i.sign() != num_bigint::Sign::Minus => i.to_u64(),
        _ => None,
    }
}

fn map_field_int_u64_map(m: &BTreeMap<TermOrdKey, Term>, key: &str) -> Option<u64> {
    match m.get(&TermOrdKey(Term::symbol(key))) {
        Some(Term::Int(i)) if i.sign() != num_bigint::Sign::Minus => i.to_u64(),
        _ => None,
    }
}

fn map_field_int_bigint_map(m: &BTreeMap<TermOrdKey, Term>, key: &str) -> Option<BigInt> {
    match m.get(&TermOrdKey(Term::symbol(key))) {
        Some(Term::Int(i)) => Some(i.clone()),
        _ => None,
    }
}

fn map_field_str_map(m: &BTreeMap<TermOrdKey, Term>, key: &str) -> Option<String> {
    match m.get(&TermOrdKey(Term::symbol(key))) {
        Some(Term::Str(s)) => Some(s.clone()),
        _ => None,
    }
}

fn map_field_str_or_symbol_map(m: &BTreeMap<TermOrdKey, Term>, key: &str) -> Option<String> {
    match m.get(&TermOrdKey(Term::symbol(key))) {
        Some(Term::Str(s)) => Some(s.clone()),
        Some(Term::Symbol(s)) => Some(s.clone()),
        _ => None,
    }
}

fn task_program_error(step: usize, message: String) -> Term {
    Term::Map(
        [
            (
                TermOrdKey(Term::symbol(":error/code")),
                Term::Str("core/task/program-error".to_string()),
            ),
            (
                TermOrdKey(Term::symbol(":error/step")),
                Term::Int(BigInt::from(step)),
            ),
            (
                TermOrdKey(Term::symbol(":error/message")),
                Term::Str(message),
            ),
        ]
        .into_iter()
        .collect(),
    )
}
