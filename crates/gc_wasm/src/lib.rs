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
use gc_prelude::{
    build_prelude, load_selfhost_coreform_toolchain_v1,
    load_selfhost_coreform_toolchain_v1_from_artifact_source,
};

#[path = "coreform_bridge.rs"]
mod coreform_bridge;

use coreform_bridge::{
    bootstrap_selfhost, extract_protocol_error_string, gate_eval_forms, js_err,
    selfhost_parse_and_canon_forms,
};

#[wasm_bindgen]
pub fn fmt_coreform_module(src: &str) -> Result<String, JsValue> {
    if cfg!(target_arch = "wasm32") {
        fmt_coreform_module_selfhost(src, 0)
    } else {
        fmt_coreform_module_rust(src)
    }
}

#[wasm_bindgen]
pub fn fmt_coreform_module_rust(src: &str) -> Result<String, JsValue> {
    let forms = parse_module(src).map_err(|e| js_err("parse", e))?;
    let forms = canonicalize_module(forms).map_err(|e| js_err("canon", e))?;
    Ok(print_module(&forms))
}

#[wasm_bindgen]
pub fn hash_coreform_module(src: &str) -> Result<String, JsValue> {
    if cfg!(target_arch = "wasm32") {
        hash_coreform_module_selfhost(src, 0)
    } else {
        hash_coreform_module_rust(src)
    }
}

#[wasm_bindgen]
pub fn hash_coreform_module_rust(src: &str) -> Result<String, JsValue> {
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

    bootstrap_selfhost(&mut ctx, &mut env, None)?;

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
pub fn fmt_coreform_module_selfhost_with_artifact(
    src: &str,
    artifact_src: &str,
    step_limit: u32,
) -> Result<String, JsValue> {
    let mut ctx = EvalCtx::with_step_limit(None);
    let prelude = build_prelude(&mut ctx);
    let mut env = prelude.env;
    bootstrap_selfhost(&mut ctx, &mut env, Some(artifact_src))?;

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

    bootstrap_selfhost(&mut ctx, &mut env, None)?;

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
pub fn hash_coreform_module_selfhost_with_artifact(
    src: &str,
    artifact_src: &str,
    step_limit: u32,
) -> Result<String, JsValue> {
    let mut ctx = EvalCtx::with_step_limit(None);
    let prelude = build_prelude(&mut ctx);
    let mut env = prelude.env;
    bootstrap_selfhost(&mut ctx, &mut env, Some(artifact_src))?;

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
    eval_coreform_module_with_gates(src, step_limit, false, false, false)
}

#[wasm_bindgen]
pub fn eval_coreform_module_with_gates(
    src: &str,
    step_limit: u32,
    stage1_pipeline: bool,
    stage1_gate: bool,
    stage2_gate: bool,
) -> Result<String, JsValue> {
    if cfg!(target_arch = "wasm32") {
        eval_coreform_module_selfhost_with_gates(
            src,
            step_limit,
            stage1_pipeline,
            stage1_gate,
            stage2_gate,
        )
    } else {
        eval_coreform_module_with_gates_rust(
            src,
            step_limit,
            stage1_pipeline,
            stage1_gate,
            stage2_gate,
        )
    }
}

#[wasm_bindgen]
pub fn eval_coreform_module_rust(src: &str, step_limit: u32) -> Result<String, JsValue> {
    eval_coreform_module_with_gates_rust(src, step_limit, false, false, false)
}

#[wasm_bindgen]
pub fn eval_coreform_module_with_gates_rust(
    src: &str,
    step_limit: u32,
    stage1_pipeline: bool,
    stage1_gate: bool,
    stage2_gate: bool,
) -> Result<String, JsValue> {
    let forms = parse_module(src).map_err(|e| js_err("parse", e))?;
    let mut forms = canonicalize_module(forms).map_err(|e| js_err("canon", e))?;
    gate_eval_forms(&mut forms, stage1_pipeline, stage1_gate, stage2_gate)?;

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
    eval_coreform_module_selfhost_with_gates(src, step_limit, false, false, false)
}

#[wasm_bindgen]
pub fn eval_coreform_module_selfhost_with_gates(
    src: &str,
    step_limit: u32,
    stage1_pipeline: bool,
    stage1_gate: bool,
    stage2_gate: bool,
) -> Result<String, JsValue> {
    // Toolchain bootstrap is trusted; do not charge it against the step limit for the input module.
    let mut ctx = EvalCtx::with_step_limit(None);
    let prelude = build_prelude(&mut ctx);
    let mut env = prelude.env;

    bootstrap_selfhost(&mut ctx, &mut env, None)?;

    // Keep parse/canonicalize out of user eval step budgets for parity with Rust frontend.
    ctx.steps = 0;
    ctx.step_limit = None;
    let mut forms = selfhost_parse_and_canon_forms(&mut ctx, &env, src)?;
    gate_eval_forms(&mut forms, stage1_pipeline, stage1_gate, stage2_gate)?;

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

#[wasm_bindgen]
pub fn eval_coreform_module_selfhost_with_artifact(
    src: &str,
    artifact_src: &str,
    step_limit: u32,
) -> Result<String, JsValue> {
    eval_coreform_module_selfhost_with_artifact_and_gates(
        src,
        artifact_src,
        step_limit,
        false,
        false,
        false,
    )
}

#[wasm_bindgen]
pub fn eval_coreform_module_selfhost_with_artifact_and_gates(
    src: &str,
    artifact_src: &str,
    step_limit: u32,
    stage1_pipeline: bool,
    stage1_gate: bool,
    stage2_gate: bool,
) -> Result<String, JsValue> {
    let mut ctx = EvalCtx::with_step_limit(None);
    let prelude = build_prelude(&mut ctx);
    let mut env = prelude.env;
    bootstrap_selfhost(&mut ctx, &mut env, Some(artifact_src))?;

    ctx.steps = 0;
    ctx.step_limit = None;
    let mut forms = selfhost_parse_and_canon_forms(&mut ctx, &env, src)?;
    gate_eval_forms(&mut forms, stage1_pipeline, stage1_gate, stage2_gate)?;

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
        self.eval_module_with_gates(src, false, false, false)
    }

    /// Parse + canonicalize + eval with optional Stage-1/Stage-2 gate enforcement, then step.
    pub fn eval_module_with_gates(
        &mut self,
        src: &str,
        stage1_pipeline: bool,
        stage1_gate: bool,
        stage2_gate: bool,
    ) -> Result<JsValue, JsValue> {
        let r = if cfg!(target_arch = "wasm32") {
            self.eval_module_selfhost_internal(
                src,
                None,
                stage1_pipeline,
                stage1_gate,
                stage2_gate,
            )?
        } else {
            self.eval_module_internal(src, stage1_pipeline, stage1_gate, stage2_gate)?
        };
        serde_wasm_bindgen::to_value(&r).map_err(|e| js_err("serde", e))
    }

    /// Rust frontend parity-only path: parse + canonicalize outside the kernel, then step.
    pub fn eval_module_rust(&mut self, src: &str) -> Result<JsValue, JsValue> {
        self.eval_module_with_gates_rust(src, false, false, false)
    }

    /// Rust frontend parity-only path with optional Stage-1/Stage-2 gate enforcement.
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

    /// Self-hosted frontend path: parse + canonicalize inside the kernel, then step.
    pub fn eval_module_selfhost(&mut self, src: &str) -> Result<JsValue, JsValue> {
        self.eval_module_selfhost_with_gates(src, false, false, false)
    }

    /// Self-hosted frontend path with optional Stage-1/Stage-2 gate enforcement.
    pub fn eval_module_selfhost_with_gates(
        &mut self,
        src: &str,
        stage1_pipeline: bool,
        stage1_gate: bool,
        stage2_gate: bool,
    ) -> Result<JsValue, JsValue> {
        let r = self.eval_module_selfhost_internal(
            src,
            None,
            stage1_pipeline,
            stage1_gate,
            stage2_gate,
        )?;
        serde_wasm_bindgen::to_value(&r).map_err(|e| js_err("serde", e))
    }

    /// Self-hosted frontend path with explicit artifact source text.
    pub fn eval_module_selfhost_with_artifact(
        &mut self,
        src: &str,
        artifact_src: &str,
    ) -> Result<JsValue, JsValue> {
        self.eval_module_selfhost_with_artifact_and_gates(src, artifact_src, false, false, false)
    }

    /// Self-hosted artifact path with optional Stage-1/Stage-2 gate enforcement.
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

    fn eval_module_internal(
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

    fn eval_module_selfhost_internal(
        &mut self,
        src: &str,
        artifact_src: Option<&str>,
        stage1_pipeline: bool,
        stage1_gate: bool,
        stage2_gate: bool,
    ) -> Result<StepResult, JsValue> {
        self.reset_selfhost(artifact_src)?;

        // Keep parse/canonicalize out of user eval step budgets for parity with Rust frontend.
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
#[path = "tests.rs"]
mod tests;
