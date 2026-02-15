use wasm_bindgen::prelude::*;

use gc_coreform::{canonicalize_module, hash_module, parse_module, print_module, print_term};
use gc_kernel::{EvalCtx, Value, eval_module};
use gc_prelude::build_prelude;

fn js_err(code: &str, msg: impl ToString) -> JsValue {
    JsValue::from_str(&format!("{code}: {}", msg.to_string()))
}

#[wasm_bindgen]
pub fn fmt_coreform_module(src: &str) -> Result<String, JsValue> {
    let forms = parse_module(src).map_err(|e| js_err("parse", e))?;
    let forms = canonicalize_module(forms).map_err(|e| js_err("canon", e))?;
    Ok(print_module(&forms) + "\n")
}

#[wasm_bindgen]
pub fn hash_coreform_module(src: &str) -> Result<String, JsValue> {
    let forms = parse_module(src).map_err(|e| js_err("parse", e))?;
    let forms = canonicalize_module(forms).map_err(|e| js_err("canon", e))?;
    Ok(hex::encode(hash_module(&forms)))
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
