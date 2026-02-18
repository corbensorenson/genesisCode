use gc_coreform::{Term, TermOrdKey, hash_module, parse_module};
use gc_effects::{CapsPolicy, Decision, EffectLog, EffectsError, replay, run};
use gc_kernel::{EvalCtx, Value, eval_module, value_hash};
use gc_prelude::build_prelude;

fn documented_host_abi_ops() -> Vec<String> {
    let spec = include_str!("../../../docs/spec/HOST_ABI.md");
    let mut in_block = false;
    let mut ops = Vec::new();
    for line in spec.lines() {
        if line.contains("HOST_ABI_OPS_BEGIN") {
            in_block = true;
            continue;
        }
        if line.contains("HOST_ABI_OPS_END") {
            in_block = false;
            continue;
        }
        if !in_block {
            continue;
        }
        let Some(start) = line.find('`') else {
            continue;
        };
        let rest = &line[start + 1..];
        let Some(end) = rest.find('`') else {
            continue;
        };
        let op = &rest[..end];
        if op.contains("::") {
            ops.push(op.to_string());
        }
    }
    ops
}

fn allow_policy_for(ops: &[String]) -> CapsPolicy {
    let allow = ops
        .iter()
        .map(|op| format!("\"{op}\""))
        .collect::<Vec<_>>()
        .join(", ");
    let toml = format!("allow = [{allow}]");
    CapsPolicy::from_toml_str(&toml).expect("parse policy")
}

fn mk_single_effect_program(op: &str) -> (Vec<Term>, [u8; 32]) {
    let src = format!(
        "
        (def prog
          (core/effect::perform
            '{op}
            {{}}
            (fn (x) (core/effect::pure x))))
        prog
    "
    );
    let forms = parse_module(&src).expect("parse module");
    let h = hash_module(&forms);
    (forms, h)
}

fn sealed_error_code(value: &Value, error_tok: gc_kernel::SealId) -> Option<String> {
    let Value::Sealed { token, payload } = value else {
        return None;
    };
    if *token != error_tok {
        return None;
    }
    let Value::Data(Term::Map(m)) = payload.as_ref() else {
        return None;
    };
    match m.get(&TermOrdKey(Term::symbol(":error/code"))) {
        Some(Term::Str(s)) => Some(s.clone()),
        _ => None,
    }
}

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

#[test]
fn editor_plugin_and_task_ops_are_replay_deterministic() {
    let ops = vec![
        "editor/plugin::command".to_string(),
        "editor/task::fmt-coreform".to_string(),
        "editor/task::lint-module".to_string(),
        "editor/task::optimize-module".to_string(),
        "editor/task::parse-module".to_string(),
        "editor/task::test-pkg".to_string(),
        "editor/task::typecheck-pkg".to_string(),
    ];
    let policy = allow_policy_for(&ops);

    for op in ops {
        let (forms, h) = mk_single_effect_program(&op);

        let mut ctx = EvalCtx::new();
        let prelude = build_prelude(&mut ctx);
        let error_tok = ctx.protocol.expect("protocol").error;
        let mut env = prelude.env;
        let prog = eval_module(&mut ctx, &mut env, &forms).expect("eval");
        let run_out = run(&mut ctx, &policy, prog, h, "host-abi-test".to_string()).expect("run");

        assert_eq!(
            run_out.log.entries.len(),
            1,
            "{op}: expected single log entry"
        );
        assert_eq!(run_out.log.entries[0].decision, Decision::Allow, "{op}");
        assert_eq!(
            sealed_error_code(&run_out.value, error_tok).as_deref(),
            Some("core/caps/not-supported"),
            "{op}: expected deterministic not-supported response"
        );

        let log_term = run_out.log.to_term();
        let replay_log = EffectLog::from_term(&log_term).expect("log decode");
        let run_hash = value_hash(&run_out.value);

        let mut ctx_rep = EvalCtx::new();
        let prelude_rep = build_prelude(&mut ctx_rep);
        let mut env_rep = prelude_rep.env;
        let prog_rep = eval_module(&mut ctx_rep, &mut env_rep, &forms).expect("eval replay");
        let replay_value = replay(&mut ctx_rep, prog_rep, &replay_log).expect("replay");
        let replay_hash = value_hash(&replay_value);

        assert_eq!(run_hash, replay_hash, "{op}: run/replay hash mismatch");
    }
}
