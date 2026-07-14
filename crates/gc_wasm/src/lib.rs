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
mod runtime;

use coreform_bridge::{
    bootstrap_selfhost, extract_protocol_error_string, gate_eval_forms, js_err,
    selfhost_parse_and_canon_forms,
};
pub use runtime::Runtime;
#[cfg(test)]
pub(crate) use runtime::{StepResult, hash_request, mk_caps_denied};

fn require_wasm_selfhost_artifact(api: &str) -> Result<(), JsValue> {
    if cfg!(target_arch = "wasm32") {
        return Err(js_err(
            "selfhost/artifact-required",
            format!(
                "{api} requires explicit selfhost artifact input in wasm; use the corresponding *_with_artifact API"
            ),
        ));
    }
    Ok(())
}

#[wasm_bindgen]
pub fn fmt_coreform_module(src: &str) -> Result<String, JsValue> {
    fmt_coreform_module_native(src)
}

fn fmt_coreform_module_native(src: &str) -> Result<String, JsValue> {
    let forms = parse_module(src).map_err(|e| js_err("parse", e))?;
    let forms = canonicalize_module(forms).map_err(|e| js_err("canon", e))?;
    Ok(print_module(&forms))
}

#[cfg(feature = "parity-harness")]
#[wasm_bindgen]
pub fn fmt_coreform_module_rust(src: &str) -> Result<String, JsValue> {
    fmt_coreform_module_native(src)
}

#[wasm_bindgen]
pub fn hash_coreform_module(src: &str) -> Result<String, JsValue> {
    hash_coreform_module_native(src)
}

fn hash_coreform_module_native(src: &str) -> Result<String, JsValue> {
    let forms = parse_module(src).map_err(|e| js_err("parse", e))?;
    let forms = canonicalize_module(forms).map_err(|e| js_err("canon", e))?;
    Ok(hex::encode(hash_module(&forms)))
}

#[cfg(feature = "parity-harness")]
#[wasm_bindgen]
pub fn hash_coreform_module_rust(src: &str) -> Result<String, JsValue> {
    hash_coreform_module_native(src)
}

#[wasm_bindgen]
pub fn fmt_coreform_module_selfhost(src: &str, step_limit: u32) -> Result<String, JsValue> {
    require_wasm_selfhost_artifact("fmt_coreform_module_selfhost")?;
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
        .apply(&mut ctx, Value::data(Term::Str(src.to_owned())))
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
        .apply(&mut ctx, Value::data(Term::Str(src.to_owned())))
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
    require_wasm_selfhost_artifact("hash_coreform_module_selfhost")?;
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
        .apply(&mut ctx, Value::data(Term::Str(src.to_owned())))
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
        .apply(&mut ctx, Value::data(Term::Str(src.to_owned())))
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
    eval_coreform_module_with_gates_native(
        src,
        step_limit,
        stage1_pipeline,
        stage1_gate,
        stage2_gate,
    )
}

fn eval_coreform_module_with_gates_native(
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

#[cfg(feature = "parity-harness")]
#[wasm_bindgen]
pub fn eval_coreform_module_rust(src: &str, step_limit: u32) -> Result<String, JsValue> {
    eval_coreform_module_with_gates_native(src, step_limit, false, false, false)
}

#[cfg(feature = "parity-harness")]
#[wasm_bindgen]
pub fn eval_coreform_module_with_gates_rust(
    src: &str,
    step_limit: u32,
    stage1_pipeline: bool,
    stage1_gate: bool,
    stage2_gate: bool,
) -> Result<String, JsValue> {
    eval_coreform_module_with_gates_native(
        src,
        step_limit,
        stage1_pipeline,
        stage1_gate,
        stage2_gate,
    )
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
    require_wasm_selfhost_artifact("eval_coreform_module_selfhost_with_gates")?;
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
