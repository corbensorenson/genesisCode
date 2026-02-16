use std::collections::BTreeMap;

use blake3::Hasher;
use serde::{Deserialize, Serialize};
use wasm_bindgen::prelude::*;

use gc_coreform::{
    Term, TermOrdKey, canonicalize_module, hash_module, hash_term, parse_module, parse_term,
    print_module, print_term,
};
use gc_kernel::{
    Apply, EffectProgram, EffectRequest, Env, EvalCtx, SealId, Value, eval_module, value_hash,
};
use gc_prelude::{build_prelude, load_selfhost_coreform_toolchain_v1};

fn js_err(code: &str, msg: impl ToString) -> JsValue {
    JsValue::from_str(&format!("{code}: {}", msg.to_string()))
}

fn extract_protocol_error_string(ctx: &EvalCtx, v: &Value) -> Option<String> {
    let tok = ctx.protocol?.error;
    let Value::Sealed { token, payload } = v else {
        return None;
    };
    if *token != tok {
        return None;
    }

    let payload_term = payload.to_term_for_log(Some(tok));
    match &payload_term {
        Term::Map(m) => {
            let code = m
                .get(&TermOrdKey(Term::Symbol(":error/code".to_string())))
                .and_then(|t| match t {
                    Term::Str(s) => Some(s.as_str()),
                    _ => None,
                })
                .unwrap_or("core/error");
            let msg = m
                .get(&TermOrdKey(Term::Symbol(":error/message".to_string())))
                .and_then(|t| match t {
                    Term::Str(s) => Some(s.as_str()),
                    _ => None,
                })
                .unwrap_or("error");
            Some(format!("{code}: {msg}"))
        }
        _ => Some(print_term(&payload_term)),
    }
}

fn selfhost_parse_canonicalize_module(
    ctx: &mut EvalCtx,
    env: &Env,
    src: &str,
) -> Result<Vec<Term>, JsValue> {
    let parse_fn = env
        .get("selfhost/parse::parse-module")
        .ok_or_else(|| js_err("selfhost/missing", "missing selfhost/parse::parse-module"))?;
    let parsed = parse_fn
        .apply(ctx, Value::Data(Term::Str(src.to_owned())))
        .map_err(|e| js_err("selfhost/eval", e))?;
    if let Some(s) = extract_protocol_error_string(ctx, &parsed) {
        return Err(js_err("selfhost/error", s));
    }
    let Some(Term::Vector(parsed_forms)) = parsed.as_data() else {
        return Err(js_err(
            "selfhost/bad_return",
            format!(
                "selfhost parse-module returned non-vector: {}",
                parsed.debug_repr()
            ),
        ));
    };

    let canon_fn = env
        .get("selfhost/canon::canonicalize-module")
        .ok_or_else(|| {
            js_err(
                "selfhost/missing",
                "missing selfhost/canon::canonicalize-module",
            )
        })?;
    let canon = canon_fn
        .apply(ctx, Value::Data(Term::Vector(parsed_forms.clone())))
        .map_err(|e| js_err("selfhost/eval", e))?;
    if let Some(s) = extract_protocol_error_string(ctx, &canon) {
        return Err(js_err("selfhost/error", s));
    }
    let Some(Term::Vector(forms)) = canon.as_data() else {
        return Err(js_err(
            "selfhost/bad_return",
            format!(
                "selfhost canonicalize-module returned non-vector: {}",
                canon.debug_repr()
            ),
        ));
    };
    Ok(forms.clone())
}

#[wasm_bindgen]
pub fn fmt_coreform_module(src: &str) -> Result<String, JsValue> {
    let forms = parse_module(src).map_err(|e| js_err("parse", e))?;
    let forms = canonicalize_module(forms).map_err(|e| js_err("canon", e))?;
    Ok(print_module(&forms))
}

#[wasm_bindgen]
pub fn hash_coreform_module(src: &str) -> Result<String, JsValue> {
    let forms = parse_module(src).map_err(|e| js_err("parse", e))?;
    let forms = canonicalize_module(forms).map_err(|e| js_err("canon", e))?;
    Ok(hex::encode(hash_module(&forms)))
}

#[wasm_bindgen]
pub fn fmt_coreform_module_selfhost(src: &str, step_limit: u32) -> Result<String, JsValue> {
    // Toolchain bootstrap is trusted; do not charge it against the step limit for the input module.
    let mut ctx = EvalCtx::with_step_limit(None);
    let prelude = build_prelude(&mut ctx);
    let mut env = prelude.env;

    load_selfhost_coreform_toolchain_v1(&mut ctx, &mut env)
        .map_err(|e| js_err("selfhost/init", e))?;

    ctx.steps = 0;
    ctx.step_limit = if step_limit == 0 {
        None
    } else {
        Some(step_limit as u64)
    };

    let f = env
        .get("selfhost/tool::fmt-module")
        .ok_or_else(|| js_err("selfhost/missing", "missing selfhost/tool::fmt-module"))?;
    let r = f
        .apply(&mut ctx, Value::Data(Term::Str(src.to_owned())))
        .map_err(|e| js_err("selfhost/eval", e))?;

    if let Some(s) = extract_protocol_error_string(&ctx, &r) {
        return Err(js_err("selfhost/error", s));
    }
    let Some(Term::Str(out)) = r.as_data() else {
        return Err(js_err(
            "selfhost/bad_return",
            format!("expected string, got {}", r.debug_repr()),
        ));
    };
    Ok(out.clone())
}

#[wasm_bindgen]
pub fn hash_coreform_module_selfhost(src: &str, step_limit: u32) -> Result<String, JsValue> {
    // Toolchain bootstrap is trusted; do not charge it against the step limit for the input module.
    let mut ctx = EvalCtx::with_step_limit(None);
    let prelude = build_prelude(&mut ctx);
    let mut env = prelude.env;

    load_selfhost_coreform_toolchain_v1(&mut ctx, &mut env)
        .map_err(|e| js_err("selfhost/init", e))?;

    ctx.steps = 0;
    ctx.step_limit = if step_limit == 0 {
        None
    } else {
        Some(step_limit as u64)
    };

    let f = env
        .get("selfhost/tool::hash-module-src")
        .ok_or_else(|| js_err("selfhost/missing", "missing selfhost/tool::hash-module-src"))?;
    let r = f
        .apply(&mut ctx, Value::Data(Term::Str(src.to_owned())))
        .map_err(|e| js_err("selfhost/eval", e))?;

    if let Some(s) = extract_protocol_error_string(&ctx, &r) {
        return Err(js_err("selfhost/error", s));
    }
    let Some(Term::Str(out)) = r.as_data() else {
        return Err(js_err(
            "selfhost/bad_return",
            format!("expected string, got {}", r.debug_repr()),
        ));
    };
    Ok(out.clone())
}

#[wasm_bindgen]
pub fn fmt_coreform_term(src: &str) -> Result<String, JsValue> {
    let t = parse_term(src).map_err(|e| js_err("parse", e))?;
    Ok(print_term(&t) + "\n")
}

#[wasm_bindgen]
pub fn hash_coreform_term(src: &str) -> Result<String, JsValue> {
    let t = parse_term(src).map_err(|e| js_err("parse", e))?;
    Ok(hex::encode(hash_term(&t)))
}

#[wasm_bindgen]
pub fn eval_coreform_module(src: &str, step_limit: u32) -> Result<String, JsValue> {
    let forms = parse_module(src).map_err(|e| js_err("parse", e))?;
    let forms = canonicalize_module(forms).map_err(|e| js_err("canon", e))?;

    let mut ctx = if step_limit == 0 {
        EvalCtx::with_step_limit(None)
    } else {
        EvalCtx::with_step_limit(Some(step_limit as u64))
    };
    let prelude = build_prelude(&mut ctx);
    let mut env = prelude.env;

    let v = eval_module(&mut ctx, &mut env, &forms).map_err(|e| js_err("eval", e))?;
    if matches!(v, Value::EffectProgram(_)) {
        return Err(js_err(
            "eval",
            "program produced an effect program; use the host runner for effects",
        ));
    }

    let protocol_error = ctx.protocol.map(|p| p.error);
    Ok(print_term(&v.to_term_for_log(protocol_error)) + "\n")
}

#[wasm_bindgen]
pub fn eval_coreform_module_selfhost(src: &str, step_limit: u32) -> Result<String, JsValue> {
    // Toolchain bootstrap is trusted; do not charge it against the step limit for the input module.
    let mut ctx = EvalCtx::with_step_limit(None);
    let prelude = build_prelude(&mut ctx);
    let mut env = prelude.env;

    load_selfhost_coreform_toolchain_v1(&mut ctx, &mut env)
        .map_err(|e| js_err("selfhost/init", e))?;

    // Keep parse/canonicalize out of user eval step budgets for parity with Rust frontend.
    ctx.steps = 0;
    ctx.step_limit = None;
    let forms = selfhost_parse_canonicalize_module(&mut ctx, &env, src)?;

    ctx.steps = 0;
    ctx.step_limit = if step_limit == 0 {
        None
    } else {
        Some(step_limit as u64)
    };
    let v = eval_module(&mut ctx, &mut env, &forms).map_err(|e| js_err("eval", e))?;
    if matches!(v, Value::EffectProgram(_)) {
        return Err(js_err(
            "eval",
            "program produced an effect program; use the host runner for effects",
        ));
    }

    let protocol_error = ctx.protocol.map(|p| p.error);
    Ok(print_term(&v.to_term_for_log(protocol_error)) + "\n")
}

#[derive(Clone)]
struct PendingEffect {
    op: String,
    k: Value,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum StepResult {
    Done {
        module_h: String,
        value: String,
        value_h: String,
    },
    Effect {
        module_h: String,
        op: String,
        payload: String,
        payload_h: String,
        cont_h: String,
        req_h: String,
    },
}

#[derive(Debug, Deserialize, Serialize)]
struct ResumeResult {
    resp_h: String,
    next: StepResult,
}

#[wasm_bindgen]
pub struct Runtime {
    step_limit: Option<u64>,
    module_h: Option<[u8; 32]>,
    ctx: EvalCtx,
    env: Env,
    cur: Option<Value>,
    pending: Option<PendingEffect>,
}

#[wasm_bindgen]
impl Runtime {
    #[wasm_bindgen(constructor)]
    pub fn new(step_limit: u32) -> Runtime {
        let step_limit = if step_limit == 0 {
            None
        } else {
            Some(step_limit as u64)
        };
        let mut ctx = EvalCtx::with_step_limit(step_limit);
        let prelude = build_prelude(&mut ctx);
        Runtime {
            step_limit,
            module_h: None,
            ctx,
            env: prelude.env,
            cur: None,
            pending: None,
        }
    }

    /// Parse + canonicalize + eval a CoreForm module, then step to the first done/effect result.
    pub fn eval_module(&mut self, src: &str) -> Result<JsValue, JsValue> {
        // Reset to a clean deterministic state so reusing a Runtime doesn't perturb seal IDs.
        self.ctx = EvalCtx::with_step_limit(self.step_limit);
        let prelude = build_prelude(&mut self.ctx);
        self.env = prelude.env;
        self.cur = None;
        self.pending = None;

        let forms = parse_module(src).map_err(|e| js_err("parse", e))?;
        let forms = canonicalize_module(forms).map_err(|e| js_err("canon", e))?;
        let mh = hash_module(&forms);
        self.module_h = Some(mh);

        let v = eval_module(&mut self.ctx, &mut self.env, &forms).map_err(|e| js_err("eval", e))?;
        self.cur = Some(v);
        self.step()
    }

    /// Self-hosted frontend path: parse + canonicalize inside the kernel, then step.
    pub fn eval_module_selfhost(&mut self, src: &str) -> Result<JsValue, JsValue> {
        // Bootstrap toolchain without charging user step budgets.
        self.ctx = EvalCtx::with_step_limit(None);
        let prelude = build_prelude(&mut self.ctx);
        self.env = prelude.env;
        self.cur = None;
        self.pending = None;

        load_selfhost_coreform_toolchain_v1(&mut self.ctx, &mut self.env)
            .map_err(|e| js_err("selfhost/init", e))?;

        self.ctx.steps = 0;
        self.ctx.step_limit = None;
        let forms = selfhost_parse_canonicalize_module(&mut self.ctx, &self.env, src)?;
        let mh = hash_module(&forms);
        self.module_h = Some(mh);

        self.ctx.steps = 0;
        self.ctx.step_limit = self.step_limit;

        let v = eval_module(&mut self.ctx, &mut self.env, &forms).map_err(|e| js_err("eval", e))?;
        self.cur = Some(v);
        self.step()
    }

    /// Step until either the program is done or an effect request is produced.
    pub fn step(&mut self) -> Result<JsValue, JsValue> {
        if self.pending.is_some() {
            return Err(js_err(
                "state",
                "pending effect request; call respond_* before stepping again",
            ));
        }
        let r = self.step_internal()?;
        serde_wasm_bindgen::to_value(&r).map_err(|e| js_err("serde", e))
    }

    /// Resume by responding with a data term value.
    pub fn respond_data(&mut self, resp_term_src: &str) -> Result<JsValue, JsValue> {
        let term = parse_term(resp_term_src).map_err(|e| js_err("parse", e))?;
        self.respond_value(Value::Data(term))
    }

    /// Resume by denying the capability (constructs a sealed ERROR inside the kernel).
    pub fn respond_denied(&mut self) -> Result<JsValue, JsValue> {
        let op = self
            .pending
            .as_ref()
            .ok_or_else(|| js_err("state", "no pending effect request"))?
            .op
            .clone();
        let error_tok = self
            .ctx
            .protocol
            .map(|p| p.error)
            .ok_or_else(|| js_err("state", "missing protocol tokens"))?;
        self.respond_value(mk_caps_denied(error_tok, &op))
    }

    /// Resume by constructing a sealed ERROR value inside the kernel.
    pub fn respond_error(&mut self, code: &str, message: &str) -> Result<JsValue, JsValue> {
        let op = self
            .pending
            .as_ref()
            .ok_or_else(|| js_err("state", "no pending effect request"))?
            .op
            .clone();
        let error_tok = self
            .ctx
            .protocol
            .map(|p| p.error)
            .ok_or_else(|| js_err("state", "missing protocol tokens"))?;
        self.respond_value(mk_error(error_tok, code, message.to_string(), Some(&op)))
    }
}

impl Runtime {
    fn step_internal(&mut self) -> Result<StepResult, JsValue> {
        let module_h = hex::encode(
            self.module_h
                .ok_or_else(|| js_err("state", "no module loaded"))?,
        );

        let cur = self
            .cur
            .as_ref()
            .ok_or_else(|| js_err("state", "no program loaded"))?
            .clone();

        let (out, pending) = match cur {
            Value::EffectProgram(p) => match p.as_ref() {
                EffectProgram::Pure(v) => {
                    let protocol_error = self.ctx.protocol.map(|p| p.error);
                    let value = print_term(&v.to_term_for_log(protocol_error));
                    let value_h = hex::encode(value_hash(v));
                    (
                        StepResult::Done {
                            module_h,
                            value,
                            value_h,
                        },
                        None,
                    )
                }
                EffectProgram::Perform { request } => {
                    let effect_tok = self
                        .ctx
                        .protocol
                        .map(|p| p.effect)
                        .ok_or_else(|| js_err("state", "missing protocol tokens"))?;
                    let req = unseal_effect_request(request.as_ref(), effect_tok)?;

                    let payload_s = print_term(&req.payload);
                    let payload_h = hash_term(&req.payload);
                    let cont_h = value_hash(&req.k);
                    let req_h = hash_request(&req.op, payload_h, cont_h);

                    (
                        StepResult::Effect {
                            module_h,
                            op: req.op.clone(),
                            payload: payload_s,
                            payload_h: hex::encode(payload_h),
                            cont_h: hex::encode(cont_h),
                            req_h: hex::encode(req_h),
                        },
                        Some(PendingEffect {
                            op: req.op,
                            k: (*req.k).clone(),
                        }),
                    )
                }
            },
            other => {
                let protocol_error = self.ctx.protocol.map(|p| p.error);
                let value = print_term(&other.to_term_for_log(protocol_error));
                let value_h = hex::encode(value_hash(&other));
                (
                    StepResult::Done {
                        module_h,
                        value,
                        value_h,
                    },
                    None,
                )
            }
        };

        if let Some(p) = pending {
            self.pending = Some(p);
        }
        Ok(out)
    }

    fn respond_value(&mut self, resp_val: Value) -> Result<JsValue, JsValue> {
        let out = self.respond_value_internal(resp_val)?;
        serde_wasm_bindgen::to_value(&out).map_err(|e| js_err("serde", e))
    }

    fn respond_value_internal(&mut self, resp_val: Value) -> Result<ResumeResult, JsValue> {
        let PendingEffect { k, .. } = self
            .pending
            .take()
            .ok_or_else(|| js_err("state", "no pending effect request"))?;

        let resp_h = value_hash(&resp_val);

        let next = k
            .apply(&mut self.ctx, resp_val)
            .map_err(|e| js_err("eval", e))?;
        let next = match next {
            Value::EffectProgram(_) => next,
            other => Value::EffectProgram(Box::new(EffectProgram::Pure(Box::new(other)))),
        };
        self.cur = Some(next);

        let next_step = self.step_internal()?;
        Ok(ResumeResult {
            resp_h: hex::encode(resp_h),
            next: next_step,
        })
    }
}

fn unseal_effect_request(v: &Value, effect_tok: SealId) -> Result<EffectRequest, JsValue> {
    let Value::Sealed { token, payload } = v else {
        return Err(js_err("effect", "bad effect seal"));
    };
    if *token != effect_tok {
        return Err(js_err("effect", "bad effect seal token"));
    }
    let Value::EffectRequest(r) = payload.as_ref() else {
        return Err(js_err("effect", "bad effect request payload"));
    };
    Ok(r.clone())
}

fn hash_request(op: &str, payload_h: [u8; 32], cont_h: [u8; 32]) -> [u8; 32] {
    let mut h = Hasher::new();
    h.update(b"GCv0.2\0effect-req\0");
    h.update(op.as_bytes());
    h.update(b"\0");
    h.update(&payload_h);
    h.update(&cont_h);
    *h.finalize().as_bytes()
}

fn mk_caps_denied(error_tok: SealId, op: &str) -> Value {
    mk_error(
        error_tok,
        "core/caps/denied",
        format!("capability denied: {op}"),
        Some(op),
    )
}

fn mk_error(error_tok: SealId, code: &str, msg: String, op: Option<&str>) -> Value {
    let mut m = BTreeMap::new();
    m.insert(
        TermOrdKey(Term::Symbol(":error/code".to_string())),
        Term::Str(code.to_string()),
    );
    m.insert(
        TermOrdKey(Term::Symbol(":error/message".to_string())),
        Term::Str(msg),
    );

    let mut ctxm = BTreeMap::new();
    ctxm.insert(
        TermOrdKey(Term::Symbol(":subsystem".to_string())),
        Term::Str("effects".to_string()),
    );
    if let Some(op) = op {
        m.insert(
            TermOrdKey(Term::Symbol(":error/op".to_string())),
            Term::Symbol(op.to_string()),
        );
        ctxm.insert(
            TermOrdKey(Term::Symbol(":op".to_string())),
            Term::Symbol(op.to_string()),
        );
    }
    m.insert(
        TermOrdKey(Term::Symbol(":error/context".to_string())),
        Term::Map(ctxm),
    );
    Value::Sealed {
        token: error_tok,
        payload: Box::new(Value::Data(Term::Map(m))),
    }
}

// Tiny dependency-free hex encoding for wasm.
mod hex {
    pub fn encode(bytes: [u8; 32]) -> String {
        const HEX: &[u8; 16] = b"0123456789abcdef";
        let mut out = String::with_capacity(64);
        for b in bytes {
            out.push(HEX[(b >> 4) as usize] as char);
            out.push(HEX[(b & 0x0f) as usize] as char);
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gc_effects::CapsPolicy;

    fn eval_to_first_step(rt: &mut Runtime, src: &str) -> StepResult {
        rt.ctx = EvalCtx::with_step_limit(rt.step_limit);
        let prelude = build_prelude(&mut rt.ctx);
        rt.env = prelude.env;
        rt.cur = None;
        rt.pending = None;

        let forms = canonicalize_module(parse_module(src).unwrap()).unwrap();
        rt.module_h = Some(hash_module(&forms));
        let v = eval_module(&mut rt.ctx, &mut rt.env, &forms).unwrap();
        rt.cur = Some(v);
        rt.step_internal().unwrap()
    }

    fn eval_to_first_step_selfhost(rt: &mut Runtime, src: &str) -> StepResult {
        rt.ctx = EvalCtx::with_step_limit(None);
        let prelude = build_prelude(&mut rt.ctx);
        rt.env = prelude.env;
        rt.cur = None;
        rt.pending = None;

        load_selfhost_coreform_toolchain_v1(&mut rt.ctx, &mut rt.env).unwrap();
        rt.ctx.steps = 0;
        rt.ctx.step_limit = None;
        let forms = selfhost_parse_canonicalize_module(&mut rt.ctx, &rt.env, src).unwrap();
        rt.module_h = Some(hash_module(&forms));
        rt.ctx.steps = 0;
        rt.ctx.step_limit = rt.step_limit;

        let v = eval_module(&mut rt.ctx, &mut rt.env, &forms).unwrap();
        rt.cur = Some(v);
        rt.step_internal().unwrap()
    }

    #[test]
    fn runtime_steps_pure_program_to_done() {
        let mut rt = Runtime::new(0);
        let step = eval_to_first_step(
            &mut rt,
            r#"
              (def m::x 1)
              m::x
            "#,
        );
        let StepResult::Done { value, .. } = step else {
            panic!("expected done, got {:?}", step);
        };
        assert_eq!(value, "1");
    }

    #[test]
    fn runtime_steps_effect_program_and_resumes_with_data() {
        let mut rt = Runtime::new(0);
        let step = eval_to_first_step(
            &mut rt,
            r#"
              (core/effect::perform
                'sys/time::now
                nil
                (fn (t) (core/effect::pure t)))
            "#,
        );
        let StepResult::Effect {
            op,
            payload,
            payload_h,
            cont_h,
            req_h,
            ..
        } = step
        else {
            panic!("expected effect, got {:?}", step);
        };
        assert_eq!(op, "sys/time::now");
        assert_eq!(payload, "nil");

        let pending_k = rt.pending.as_ref().unwrap().k.clone();
        let expected_payload_h = hex::encode(hash_term(&Term::Nil));
        let expected_cont_h = hex::encode(value_hash(&pending_k));
        let expected_req_h = hex::encode(hash_request(
            "sys/time::now",
            hash_term(&Term::Nil),
            value_hash(&pending_k),
        ));
        assert_eq!(payload_h, expected_payload_h);
        assert_eq!(cont_h, expected_cont_h);
        assert_eq!(req_h, expected_req_h);

        let resp = Value::Data(parse_term("123").unwrap());
        let resumed = rt.respond_value_internal(resp).unwrap();
        assert_eq!(
            resumed.resp_h,
            hex::encode(value_hash(&Value::Data(Term::Int(123.into()))))
        );
        match resumed.next {
            StepResult::Done { value, .. } => assert_eq!(value, "123"),
            other => panic!("expected done after resume, got {:?}", other),
        }
    }

    #[test]
    fn runtime_can_resume_with_denied_error() {
        let mut rt = Runtime::new(0);
        let step = eval_to_first_step(
            &mut rt,
            r#"
              (core/effect::perform
                'sys/time::now
                nil
                (fn (t) (core/effect::pure t)))
            "#,
        );
        assert!(matches!(step, StepResult::Effect { .. }));
        let error_tok = rt.ctx.protocol.unwrap().error;
        let resumed = rt
            .respond_value_internal(mk_caps_denied(error_tok, "sys/time::now"))
            .unwrap();
        match resumed.next {
            StepResult::Done { value, .. } => {
                assert!(value.contains(":error/code"));
                assert!(value.contains("core/caps/denied"));
            }
            other => panic!("expected done after denied, got {:?}", other),
        }
    }

    #[test]
    fn wasm_runtime_hashes_match_native_effect_runner_entry() {
        let src = r#"
          (core/effect::perform
            'sys/time::now
            nil
            (fn (t) (core/effect::pure t)))
        "#;

        // WASM-side first step.
        let mut rt = Runtime::new(0);
        let step = eval_to_first_step(&mut rt, src);
        let StepResult::Effect {
            op,
            payload_h,
            cont_h,
            req_h,
            ..
        } = step
        else {
            panic!("expected effect");
        };
        assert_eq!(op, "sys/time::now");

        // Native runner first entry (deny-by-default, so response is deterministic).
        let forms = canonicalize_module(parse_module(src).unwrap()).unwrap();
        let program_hash = hash_module(&forms);
        let mut ctx = EvalCtx::with_step_limit(None);
        let prelude = build_prelude(&mut ctx);
        let mut env = prelude.env;
        let v = eval_module(&mut ctx, &mut env, &forms).unwrap();

        let policy = CapsPolicy::empty(); // deny everything
        let r = gc_effects::run(&mut ctx, &policy, v, program_hash, "test".to_string()).unwrap();
        assert_eq!(r.log.entries.len(), 1);
        let e = &r.log.entries[0];

        assert_eq!(hex::encode(e.payload_h), payload_h);
        assert_eq!(hex::encode(e.cont_h), cont_h);
        assert_eq!(hex::encode(e.req_h), req_h);

        // Now resume in wasm runtime with denied and compare response hash deterministically.
        let error_tok = rt.ctx.protocol.unwrap().error;
        let resumed = rt
            .respond_value_internal(mk_caps_denied(error_tok, "sys/time::now"))
            .unwrap();
        assert_eq!(hex::encode(e.resp_h), resumed.resp_h);

        match resumed.next {
            StepResult::Done { value, .. } => {
                // Deny produces a sealed ERROR; log serialization for ERROR drops the seal and records payload map.
                assert!(value.contains(":error/code"));
                assert!(value.contains("core/caps/denied"));
            }
            other => panic!("expected done after denied, got {:?}", other),
        }
    }

    #[test]
    fn eval_coreform_module_selfhost_matches_rust_frontend_eval() {
        let src = r#"
          (def x 19)
          (def y (prim int/add x 23))
          y
        "#;

        let rust = eval_coreform_module(src, 0).expect("rust eval");
        let selfhost = eval_coreform_module_selfhost(src, 0).expect("selfhost eval");
        assert_eq!(rust, selfhost);
    }

    #[test]
    fn runtime_eval_module_selfhost_matches_rust_frontend_path() {
        let src = r#"
          (def x 5)
          (def y (prim int/mul x 9))
          y
        "#;

        let mut rt_rust = Runtime::new(0);
        let rust_step = eval_to_first_step(&mut rt_rust, src);

        let mut rt_selfhost = Runtime::new(0);
        let selfhost_step = eval_to_first_step_selfhost(&mut rt_selfhost, src);

        match (rust_step, selfhost_step) {
            (StepResult::Done { value: a, .. }, StepResult::Done { value: b, .. }) => {
                assert_eq!(a, b);
            }
            (a, b) => panic!("expected done/done parity, got {:?} vs {:?}", a, b),
        }
    }
}
