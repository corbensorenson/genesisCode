use std::collections::BTreeMap;

use blake3::Hasher;
use gc_coreform::HASH_DOMAIN_PREFIX;
use serde::{Deserialize, Serialize};
use wasm_bindgen::prelude::*;

use super::*;

#[derive(Clone)]
pub(crate) struct PendingEffect {
    pub(crate) op: String,
    pub(crate) k: Value,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub(crate) enum StepResult {
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
pub(crate) struct ResumeResult {
    pub(crate) resp_h: String,
    pub(crate) next: StepResult,
}

#[wasm_bindgen]
pub struct Runtime {
    pub(crate) step_limit: Option<u64>,
    pub(crate) module_h: Option<[u8; 32]>,
    pub(crate) ctx: EvalCtx,
    pub(crate) env: Env,
    pub(crate) cur: Option<Value>,
    pub(crate) pending: Option<PendingEffect>,
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

    pub fn eval_module(&mut self, src: &str) -> Result<JsValue, JsValue> {
        self.eval_module_with_gates(src, false, false, false)
    }

    pub fn eval_module_with_gates(
        &mut self,
        src: &str,
        stage1_pipeline: bool,
        stage1_gate: bool,
        stage2_gate: bool,
    ) -> Result<JsValue, JsValue> {
        let r = self.eval_module_internal(src, stage1_pipeline, stage1_gate, stage2_gate)?;
        serde_wasm_bindgen::to_value(&r).map_err(|e| js_err("serde", e))
    }

    #[cfg(feature = "parity-harness")]
    pub fn eval_module_rust(&mut self, src: &str) -> Result<JsValue, JsValue> {
        self.eval_module_with_gates(src, false, false, false)
    }

    #[cfg(feature = "parity-harness")]
    pub fn eval_module_with_gates_rust(
        &mut self,
        src: &str,
        stage1_pipeline: bool,
        stage1_gate: bool,
        stage2_gate: bool,
    ) -> Result<JsValue, JsValue> {
        let r = self.eval_module_internal(src, stage1_pipeline, stage1_gate, stage2_gate)?;
        serde_wasm_bindgen::to_value(&r).map_err(|e| js_err("serde", e))
    }

    pub fn eval_module_selfhost(&mut self, src: &str) -> Result<JsValue, JsValue> {
        self.eval_module_selfhost_with_gates(src, false, false, false)
    }

    pub fn eval_module_selfhost_with_gates(
        &mut self,
        src: &str,
        stage1_pipeline: bool,
        stage1_gate: bool,
        stage2_gate: bool,
    ) -> Result<JsValue, JsValue> {
        require_wasm_selfhost_artifact("Runtime.eval_module_selfhost_with_gates")?;
        let r = self.eval_module_selfhost_internal(
            src,
            None,
            stage1_pipeline,
            stage1_gate,
            stage2_gate,
        )?;
        serde_wasm_bindgen::to_value(&r).map_err(|e| js_err("serde", e))
    }

    pub fn eval_module_selfhost_with_artifact(
        &mut self,
        src: &str,
        artifact_src: &str,
    ) -> Result<JsValue, JsValue> {
        self.eval_module_selfhost_with_artifact_and_gates(src, artifact_src, false, false, false)
    }

    pub fn eval_module_selfhost_with_artifact_and_gates(
        &mut self,
        src: &str,
        artifact_src: &str,
        stage1_pipeline: bool,
        stage1_gate: bool,
        stage2_gate: bool,
    ) -> Result<JsValue, JsValue> {
        let r = self.eval_module_selfhost_internal(
            src,
            Some(artifact_src),
            stage1_pipeline,
            stage1_gate,
            stage2_gate,
        )?;
        serde_wasm_bindgen::to_value(&r).map_err(|e| js_err("serde", e))
    }

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

    pub fn respond_data(&mut self, resp_term_src: &str) -> Result<JsValue, JsValue> {
        let term = parse_term(resp_term_src).map_err(|e| js_err("parse", e))?;
        self.respond_value(Value::data(term))
    }

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
    fn reset_plain(&mut self) {
        self.ctx = EvalCtx::with_step_limit(self.step_limit);
        let prelude = build_prelude(&mut self.ctx);
        self.env = prelude.env;
        self.cur = None;
        self.pending = None;
    }

    fn reset_selfhost(&mut self, artifact_src: Option<&str>) -> Result<(), JsValue> {
        self.ctx = EvalCtx::with_step_limit(None);
        let prelude = build_prelude(&mut self.ctx);
        self.env = prelude.env;
        self.cur = None;
        self.pending = None;
        bootstrap_selfhost(&mut self.ctx, &mut self.env, artifact_src)?;
        Ok(())
    }

    pub(crate) fn eval_module_internal(
        &mut self,
        src: &str,
        stage1_pipeline: bool,
        stage1_gate: bool,
        stage2_gate: bool,
    ) -> Result<StepResult, JsValue> {
        self.reset_plain();

        let forms = parse_module(src).map_err(|e| js_err("parse", e))?;
        let mut forms = canonicalize_module(forms).map_err(|e| js_err("canon", e))?;
        gate_eval_forms(&mut forms, stage1_pipeline, stage1_gate, stage2_gate)?;
        self.module_h = Some(hash_module(&forms));

        let v = eval_module(&mut self.ctx, &mut self.env, &forms).map_err(|e| js_err("eval", e))?;
        self.cur = Some(v);
        self.step_internal()
    }

    pub(crate) fn eval_module_selfhost_internal(
        &mut self,
        src: &str,
        artifact_src: Option<&str>,
        stage1_pipeline: bool,
        stage1_gate: bool,
        stage2_gate: bool,
    ) -> Result<StepResult, JsValue> {
        self.reset_selfhost(artifact_src)?;

        self.ctx.steps = 0;
        self.ctx.step_limit = None;
        let mut forms = selfhost_parse_and_canon_forms(&mut self.ctx, &self.env, src)?;
        gate_eval_forms(&mut forms, stage1_pipeline, stage1_gate, stage2_gate)?;
        self.module_h = Some(hash_module(&forms));

        self.ctx.steps = 0;
        self.ctx.step_limit = self.step_limit;
        let v = eval_module(&mut self.ctx, &mut self.env, &forms).map_err(|e| js_err("eval", e))?;
        self.cur = Some(v);
        self.step_internal()
    }

    pub(crate) fn step_internal(&mut self) -> Result<StepResult, JsValue> {
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

    pub(crate) fn respond_value_internal(
        &mut self,
        resp_val: Value,
    ) -> Result<ResumeResult, JsValue> {
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
    Ok(r.as_ref().clone())
}

pub(crate) fn hash_request(op: &str, payload_h: [u8; 32], cont_h: [u8; 32]) -> [u8; 32] {
    let mut h = Hasher::new();
    h.update(HASH_DOMAIN_PREFIX);
    h.update(b"effect-req\0");
    h.update(op.as_bytes());
    h.update(b"\0");
    h.update(&payload_h);
    h.update(&cont_h);
    *h.finalize().as_bytes()
}

pub(crate) fn mk_caps_denied(error_tok: SealId, op: &str) -> Value {
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
        payload: Box::new(Value::data(Term::Map(m))),
    }
}
