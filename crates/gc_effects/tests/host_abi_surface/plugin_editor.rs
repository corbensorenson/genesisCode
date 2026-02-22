use super::common::{allow_policy_for, mk_single_effect_program, sealed_error_code};
use gc_coreform::{hash_module, parse_module};
use gc_effects::{CapsPolicy, Decision, EffectLog, replay, run};
use gc_kernel::{EvalCtx, eval_module, value_hash};
use gc_prelude::build_prelude;

#[test]
fn editor_plugin_and_task_ops_are_replay_deterministic() {
    let ops = vec![
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
        assert_ne!(
            sealed_error_code(&run_out.value, error_tok).as_deref(),
            Some("core/caps/unknown-op"),
            "{op}: documented editor op reached unknown-op fallback"
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

#[test]
fn host_extension_plugin_ops_are_replay_deterministic() {
    let ops = vec![
        "editor/plugin::command".to_string(),
        "host/plugin::command".to_string(),
    ];
    let policy = CapsPolicy::from_toml_str(
        r#"
allow = ["editor/plugin::command", "host/plugin::command"]

[op."editor/plugin::command"]
allow_plugins = ["demo"]
allow_commands = ["run"]
wasi_bridge_profile = true
wasi_bridge_response = "{:ok true :status \"ok\" :bridge-op \"editor/plugin::command\"}"

[op."host/plugin::command"]
allow_plugins = ["demo"]
allow_commands = ["run"]
wasi_bridge_profile = true
wasi_bridge_response = "{:ok true :status \"ok\" :bridge-op \"host/plugin::command\"}"
"#,
    )
    .expect("policy");

    for op in ops {
        let src = format!(
            "
        (def prog
          (core/effect::perform
            '{op}
            {{:plugin \"demo\" :command \"run\" :payload {{:x 1}}}}
            (fn (x) (core/effect::pure x))))
        prog
    "
        );
        let forms = parse_module(&src).expect("parse module");
        let h = hash_module(&forms);

        let mut ctx = EvalCtx::new();
        let prelude = build_prelude(&mut ctx);
        let mut env = prelude.env;
        let prog = eval_module(&mut ctx, &mut env, &forms).expect("eval");
        let run_out = run(&mut ctx, &policy, prog, h, "host-abi-test".to_string()).expect("run");

        assert_eq!(
            run_out.log.entries.len(),
            1,
            "{op}: expected single log entry"
        );
        assert_eq!(run_out.log.entries[0].decision, Decision::Allow, "{op}");
        assert_eq!(run_out.log.entries[0].op, op, "{op}: op mismatch in log");

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
