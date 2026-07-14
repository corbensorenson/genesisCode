use gc_coreform::{Term, TermOrdKey, hash_module, parse_module};
use gc_effects::{CapsPolicy, Decision, EffectLog, replay, run};
use gc_kernel::{EvalCtx, Value, eval_module, value_hash};
use gc_prelude::build_prelude;

fn sealed_error_code(value: &Value, error_tok: gc_kernel::SealId) -> Option<String> {
    let Value::Sealed { token, payload } = value else {
        return None;
    };
    if *token != error_tok {
        return None;
    }
    let Some(Term::Map(m)) = payload.as_ref().as_data() else {
        return None;
    };
    match m.get(&TermOrdKey(Term::symbol(":error/code"))) {
        Some(Term::Str(s)) => Some(s.clone()),
        _ => None,
    }
}

#[test]
fn fs_wrapper_denied_path_returns_sealed_caps_error() {
    let src = r#"
      (def prog (core/fs::read "tmp/in.txt"))
      prog
    "#;
    let forms = parse_module(src).expect("parse module");
    let h = hash_module(&forms);
    let policy = CapsPolicy::from_toml_str("allow = []").expect("policy");

    let mut ctx = EvalCtx::new();
    let prelude = build_prelude(&mut ctx);
    let error_tok = ctx.protocol.expect("protocol").error;
    let mut env = prelude.env;
    let prog = eval_module(&mut ctx, &mut env, &forms).expect("eval");

    let out = run(
        &mut ctx,
        &policy,
        prog,
        h,
        "low-level-wrapper-conformance".to_string(),
    )
    .expect("run");
    assert_eq!(out.log.entries.len(), 1);
    assert_eq!(out.log.entries[0].op, "io/fs::read");
    assert_eq!(out.log.entries[0].decision, Decision::Deny);
    assert_eq!(
        sealed_error_code(&out.value, error_tok).as_deref(),
        Some("core/caps/denied")
    );
}

#[test]
fn process_spawn_wrapper_success_replays_deterministically() {
    let src = r#"
      (def prog (((core/process::spawn "echo") ["ok"]) {}))
      prog
    "#;
    let forms = parse_module(src).expect("parse module");
    let h = hash_module(&forms);
    let policy = CapsPolicy::from_toml_str(
        r#"
allow = ["sys/process::spawn"]

[op."sys/process::spawn"]
allow_programs = ["echo"]
wasi_bridge_profile = true
wasi_bridge_response = "{:ok true :process-id \"proc-1\" :backend \"bridge-process\"}"
"#,
    )
    .expect("policy");

    let mut ctx = EvalCtx::new();
    let prelude = build_prelude(&mut ctx);
    let mut env = prelude.env;
    let prog = eval_module(&mut ctx, &mut env, &forms).expect("eval");

    let run_out = run(
        &mut ctx,
        &policy,
        prog,
        h,
        "low-level-wrapper-conformance".to_string(),
    )
    .expect("run");
    assert_eq!(run_out.log.entries.len(), 1);
    assert_eq!(run_out.log.entries[0].op, "sys/process::spawn");
    assert_eq!(run_out.log.entries[0].decision, Decision::Allow);

    let Some(Term::Map(payload)) = run_out.value.as_data() else {
        panic!("expected map payload");
    };
    assert_eq!(
        payload.get(&TermOrdKey(Term::symbol(":process-id"))),
        Some(&Term::Str("proc-1".to_string()))
    );

    let run_hash = value_hash(&run_out.value);
    let replay_log = EffectLog::from_term(&run_out.log.to_term()).expect("decode log");

    let mut ctx_rep = EvalCtx::new();
    let prelude_rep = build_prelude(&mut ctx_rep);
    let mut env_rep = prelude_rep.env;
    let prog_rep = eval_module(&mut ctx_rep, &mut env_rep, &forms).expect("eval replay");
    let replay_value = replay(&mut ctx_rep, prog_rep, &replay_log).expect("replay");
    let replay_hash = value_hash(&replay_value);

    assert_eq!(run_hash, replay_hash, "run/replay hash mismatch");
}
