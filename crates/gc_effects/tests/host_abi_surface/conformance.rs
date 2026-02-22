use super::common::{
    allow_policy_for, documented_host_abi_ops, mk_single_effect_program, sealed_error_code,
};
use gc_effects::{CapsPolicy, Decision, EffectsError, run};
use gc_kernel::{EvalCtx, eval_module};
use gc_prelude::build_prelude;

#[test]
fn every_documented_host_abi_op_is_dispatched_by_runner() {
    let ops = documented_host_abi_ops();
    assert!(!ops.is_empty(), "host ABI op list must not be empty");

    let policy = allow_policy_for(&ops);

    for op in ops {
        let (forms, h) = mk_single_effect_program(&op);
        let mut ctx = EvalCtx::new();
        let prelude = build_prelude(&mut ctx);
        let error_tok = ctx.protocol.expect("protocol").error;
        let mut env = prelude.env;
        let prog = eval_module(&mut ctx, &mut env, &forms).expect("eval");

        match run(&mut ctx, &policy, prog, h, "host-abi-test".to_string()) {
            Ok(result) => {
                assert_eq!(
                    result.log.entries.len(),
                    1,
                    "{op}: expected exactly one log entry"
                );
                assert_eq!(
                    result.log.entries[0].decision,
                    Decision::Allow,
                    "{op}: allowlisted op should not be denied"
                );
                assert_eq!(result.log.entries[0].op, op, "{op}: log op mismatch");

                let code = sealed_error_code(&result.value, error_tok);
                assert_ne!(
                    code.as_deref(),
                    Some("core/caps/unknown-op"),
                    "{op}: documented op reached unknown-op fallback"
                );
            }
            Err(EffectsError::BadPayload(_)) | Err(EffectsError::Log(_)) => {
                // Some capabilities validate payload shape before logging a response.
                // This still proves the op is recognized in dispatch.
            }
            Err(other) => panic!("{op}: unexpected runner error: {other}"),
        }
    }
}

#[test]
fn unknown_host_abi_op_hits_unknown_op_fallback() {
    let op = "core/unknown::nope";
    let (forms, h) = mk_single_effect_program(op);
    let policy = CapsPolicy::from_toml_str(r#"allow = ["core/unknown::nope"]"#).expect("policy");

    let mut ctx = EvalCtx::new();
    let prelude = build_prelude(&mut ctx);
    let error_tok = ctx.protocol.expect("protocol").error;
    let mut env = prelude.env;
    let prog = eval_module(&mut ctx, &mut env, &forms).expect("eval");

    let result = run(&mut ctx, &policy, prog, h, "host-abi-test".to_string()).expect("run");
    assert_eq!(result.log.entries.len(), 1);
    assert_eq!(result.log.entries[0].decision, Decision::Allow);
    assert_eq!(result.log.entries[0].op, op);
    assert_eq!(
        sealed_error_code(&result.value, error_tok).as_deref(),
        Some("core/caps/unknown-op")
    );
}
