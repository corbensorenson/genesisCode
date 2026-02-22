use gc_coreform::{Term, TermOrdKey, hash_module, parse_module};
use gc_effects::{CapsPolicy, EffectLog, replay, run};
use gc_kernel::{EvalCtx, Value, eval_module, value_hash};
use gc_prelude::build_prelude;
use std::time::Instant;

fn run_and_replay_hash(src: &str, policy_toml: &str) -> ([u8; 32], [u8; 32], Value) {
    let forms = parse_module(src).expect("parse module");
    let h = hash_module(&forms);
    let policy = CapsPolicy::from_toml_str(policy_toml).expect("policy");

    let mut run_ctx = EvalCtx::new();
    let mut run_env = build_prelude(&mut run_ctx).env;
    let run_prog = eval_module(&mut run_ctx, &mut run_env, &forms).expect("eval run");
    let run_out = run(
        &mut run_ctx,
        &policy,
        run_prog,
        h,
        "task-stress".to_string(),
    )
    .expect("run");
    let run_hash = value_hash(&run_out.value);
    let replay_log = EffectLog::from_term(&run_out.log.to_term()).expect("decode log");

    let mut replay_ctx = EvalCtx::new();
    let mut replay_env = build_prelude(&mut replay_ctx).env;
    let replay_prog = eval_module(&mut replay_ctx, &mut replay_env, &forms).expect("eval replay");
    let replay_value = replay(&mut replay_ctx, replay_prog, &replay_log).expect("replay");
    let replay_hash = value_hash(&replay_value);
    (run_hash, replay_hash, run_out.value)
}

#[test]
fn task_concurrency_stress_matrix_is_replay_deterministic_under_budget() {
    let iterations: usize = std::env::var("GENESIS_TASK_STRESS_ITERATIONS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(2);
    let budget_ms: u128 = std::env::var("GENESIS_TASK_STRESS_BUDGET_MS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(75_000);

    let cancel_src = r#"
      (def worker/program
        ((core/task::program-with-initial 0)
          [(core/task::step/sleep-ms 15) (core/task::step/int-add 1)]))
      (def prog
        ((core/effect::bind (((core/task::spawn-program "scope/stress") "a") worker/program))
          (fn (spawn-a)
            ((core/effect::bind (((core/task::spawn-program "scope/stress") "b") worker/program))
              (fn (spawn-b)
                (let ((tid-a ((core/map::get spawn-a) ':task-id)))
                  (let ((tid-b ((core/map::get spawn-b) ':task-id)))
                    ((core/effect::bind (core/task::cancel tid-b))
                      (fn (_cancelled)
                        ((core/effect::bind (core/task::await tid-a))
                          (fn (_await-a)
                            (core/task::await tid-b))))))))))))
      prog
    "#;
    let cancel_policy = r#"allow = ["core/task::spawn", "core/task::cancel", "core/task::await"]"#;

    let channel_src = r#"
      (def channel/demo-finish
        (fn (cid)
          ((core/effect::bind (core/task::channel-recv cid))
            (fn (r1)
              ((core/effect::bind (core/task::channel-recv cid))
                (fn (r2)
                  ((core/effect::bind (core/task::channel-recv cid))
                    (fn (r3)
                      (core/effect::pure {:r1 r1 :r2 r2 :r3 r3})))))))))
      (def channel/demo-prepare
        (fn (cid)
          ((core/effect::bind ((core/task::channel-send cid) 10))
            (fn (_s1)
              ((core/effect::bind ((core/task::channel-send cid) 20))
                (fn (_s2)
                  ((core/effect::bind (core/task::channel-close cid))
                    (fn (_closed)
                      (channel/demo-finish cid)))))))))
      (def prog
        ((core/effect::bind (core/task::channel-open 2))
          (fn (opened) (channel/demo-prepare ((core/map::get opened) ':channel-id)))))
      prog
    "#;
    let channel_policy = r#"allow = ["core/task::channel-open", "core/task::channel-send", "core/task::channel-recv", "core/task::channel-close"]"#;

    let reduce_src = r#"
      (def mk-payload
        (fn (x) {:task/sleep-ms 40 :task/result (prim int/mul x 2)}))
      (def reducer
        (fn (acc)
          (fn (resp)
            (core/effect::pure (prim int/add acc ((core/map::get resp) ':result))))))
      (def prog
        (((((((core/task::parallel-reduce-bounded "scope/stress") "reduce")
           [1 2 3 4 5 6 7 8 9 10 11 12 13 14 15 16]) 4) mk-payload) 0) reducer))
      prog
    "#;
    let reduce_policy = r#"allow = ["core/task::spawn", "core/task::await"]"#;

    let started = Instant::now();
    for _ in 0..iterations {
        let (run_hash, replay_hash, cancel_value) = run_and_replay_hash(cancel_src, cancel_policy);
        assert_eq!(run_hash, replay_hash, "cancel-path run/replay mismatch");
        let Value::Data(Term::Map(cancel_map)) = cancel_value else {
            panic!("cancel-path expected map");
        };
        assert_eq!(
            cancel_map.get(&TermOrdKey(Term::symbol(":state"))),
            Some(&Term::symbol(":cancelled")),
            "cancel-path expected cancelled state"
        );

        let (run_hash, replay_hash, channel_value) =
            run_and_replay_hash(channel_src, channel_policy);
        assert_eq!(run_hash, replay_hash, "channel-path run/replay mismatch");
        let read_entry_map = |k: &str| -> Option<&std::collections::BTreeMap<TermOrdKey, Term>> {
            match &channel_value {
                Value::Data(Term::Map(root)) => match root.get(&TermOrdKey(Term::symbol(k))) {
                    Some(Term::Map(entry)) => Some(entry),
                    _ => None,
                },
                Value::Map(root) => match root.get(&TermOrdKey(Term::symbol(k))) {
                    Some(Value::Data(Term::Map(entry))) => Some(entry),
                    _ => None,
                },
                _ => None,
            }
        };
        let read_has = |k: &str| -> Option<bool> {
            let entry = read_entry_map(k)?;
            match entry.get(&TermOrdKey(Term::symbol(":has-value"))) {
                Some(Term::Bool(v)) => Some(*v),
                _ => None,
            }
        };
        assert_eq!(
            read_has(":r1"),
            Some(true),
            "channel-path r1 must carry value"
        );
        assert_eq!(
            read_has(":r2"),
            Some(true),
            "channel-path r2 must carry value"
        );
        assert_eq!(
            read_has(":r3"),
            Some(false),
            "channel-path close-race slot must be empty"
        );

        let (run_hash, replay_hash, reduce_value) = run_and_replay_hash(reduce_src, reduce_policy);
        assert_eq!(run_hash, replay_hash, "parallel-reduce run/replay mismatch");
        let Value::Data(Term::Int(sum)) = reduce_value else {
            panic!("parallel-reduce expected int");
        };
        assert_eq!(sum, 272.into(), "parallel-reduce result mismatch");
    }

    let elapsed_ms = started.elapsed().as_millis();
    assert!(
        elapsed_ms <= budget_ms,
        "task stress suite exceeded budget: {elapsed_ms}ms > {budget_ms}ms (iterations={iterations})"
    );
}
