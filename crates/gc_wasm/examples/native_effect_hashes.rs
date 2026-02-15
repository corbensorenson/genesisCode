use gc_coreform::{canonicalize_module, hash_module, parse_module};
use gc_effects::CapsPolicy;
use gc_kernel::{EvalCtx, eval_module, value_hash};
use gc_prelude::build_prelude;

fn hex_lower(bytes: &[u8]) -> String {
    const LUT: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for &b in bytes {
        out.push(LUT[(b >> 4) as usize] as char);
        out.push(LUT[(b & 0x0f) as usize] as char);
    }
    out
}

fn main() {
    // Keep this source exactly aligned with scripts/wasm_* checks.
    let src = r#"
      (core/effect::perform
        'sys/time::now
        nil
        (fn (t) (core/effect::pure t)))
    "#;

    let forms = canonicalize_module(parse_module(src).expect("parse")).expect("canon");
    let program_h = hash_module(&forms);

    let mut ctx = EvalCtx::with_step_limit(None);
    let prelude = build_prelude(&mut ctx);
    let mut env = prelude.env;
    let v = eval_module(&mut ctx, &mut env, &forms).expect("eval");

    let policy = CapsPolicy::empty(); // deny everything
    let r = gc_effects::run(&mut ctx, &policy, v, program_h, "native".to_string()).expect("run");
    assert_eq!(r.log.entries.len(), 1);
    let e = &r.log.entries[0];

    let out = format!(
        concat!(
            "{{",
            "\"module_h\":\"{}\",",
            "\"payload_h\":\"{}\",",
            "\"cont_h\":\"{}\",",
            "\"req_h\":\"{}\",",
            "\"resp_h\":\"{}\",",
            "\"final_value_h\":\"{}\"",
            "}}"
        ),
        hex_lower(&program_h),
        hex_lower(&e.payload_h),
        hex_lower(&e.cont_h),
        hex_lower(&e.req_h),
        hex_lower(&e.resp_h),
        hex_lower(&value_hash(&r.value)),
    );

    print!("{out}");
}
