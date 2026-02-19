mod error;
mod lock;
mod log;
mod policy;
mod refs;
mod runner;
mod runner_editor_host;
mod runner_gc_payload;
mod runner_gfx_host;
mod runner_gpk_payload;
mod runner_gpu_host;
mod runner_io_ops;
mod runner_pkg_payload;
mod runner_refs_ops;
mod runner_store_ops;
mod runner_sync_payload;
mod runner_task;
mod runner_timeout;
mod runner_vcs_payload;
mod store;

pub use crate::error::EffectsError;
pub use crate::log::{Decision, EffectLog, EffectLogEntry, LoggedResp};
pub use crate::policy::{CapsPolicy, OpPolicy};
pub use crate::refs::{RefEntry, RefsDb, SetResult};
pub use crate::runner::{RunResult, replay, replay_with_store, run};
pub use crate::store::ArtifactStore;

#[cfg(test)]
mod tests {
    use gc_coreform::TermOrdKey;
    use gc_coreform::{Term, hash_module, parse_module};
    use gc_kernel::{EvalCtx, Value, eval_module, value_hash};
    use gc_prelude::build_prelude;

    use super::*;

    fn mk_prog_for(op: &str, payload_src: &str) -> (Vec<Term>, [u8; 32]) {
        let src = format!(
            "
            (def prog
              (core/effect::perform
                '{op}
                {payload_src}
                (fn (t) (core/effect::pure t))))
            prog
        "
        );
        let forms = parse_module(&src).expect("parse module");
        let h = hash_module(&forms);
        (forms, h)
    }

    fn mk_prog() -> (Vec<Term>, [u8; 32]) {
        mk_prog_for("sys/time::now", "nil")
    }

    #[test]
    fn deny_by_default_produces_sealed_error_and_logs() {
        let (forms, h) = mk_prog();

        let mut ctx = EvalCtx::new();
        let prelude = build_prelude(&mut ctx);
        let mut env = prelude.env;
        let prog = eval_module(&mut ctx, &mut env, &forms).expect("eval");

        let pol = CapsPolicy::empty();
        let r = run(&mut ctx, &pol, prog, h, "gc_effects-test".to_string()).expect("run");

        // Expect final value is a sealed ERROR.
        match r.value {
            Value::Sealed { token, .. } => {
                assert_eq!(token, ctx.protocol.unwrap().error);
            }
            _ => panic!("expected sealed error, got {}", r.value.debug_repr()),
        }

        assert_eq!(r.log.entries.len(), 1);
        assert_eq!(r.log.entries[0].decision, Decision::Deny);
        assert!(matches!(r.log.entries[0].resp, LoggedResp::Error(_)));
    }

    #[test]
    fn allow_and_replay_roundtrip() {
        let (forms, h) = mk_prog();

        let mut ctx1 = EvalCtx::new();
        let prelude1 = build_prelude(&mut ctx1);
        let mut env1 = prelude1.env;
        let prog1 = eval_module(&mut ctx1, &mut env1, &forms).expect("eval1");

        let pol = CapsPolicy::from_toml_str(r#"allow = ["sys/time::now"]"#).unwrap();
        let r1 = run(&mut ctx1, &pol, prog1, h, "gc_effects-test".to_string()).expect("run");

        let v1_h = value_hash(&r1.value);

        // Parse log back from term and replay with fresh ctx.
        let log_term = r1.log.to_term();
        let log2 = EffectLog::from_term(&log_term).expect("parse log");

        let mut ctx2 = EvalCtx::new();
        let prelude2 = build_prelude(&mut ctx2);
        let mut env2 = prelude2.env;
        let prog2 = eval_module(&mut ctx2, &mut env2, &forms).expect("eval2");

        let v2 = replay(&mut ctx2, prog2, &log2).expect("replay");
        let v2_h = value_hash(&v2);
        assert_eq!(v1_h, v2_h);
    }

    #[test]
    fn replay_detects_tampered_response() {
        let (forms, h) = mk_prog();

        let mut ctx1 = EvalCtx::new();
        let prelude1 = build_prelude(&mut ctx1);
        let mut env1 = prelude1.env;
        let prog1 = eval_module(&mut ctx1, &mut env1, &forms).expect("eval1");

        let pol = CapsPolicy::from_toml_str(r#"allow = ["sys/time::now"]"#).unwrap();
        let mut r1 = run(&mut ctx1, &pol, prog1, h, "gc_effects-test".to_string()).expect("run");

        // Tamper.
        if let LoggedResp::Ok(t) = &mut r1.log.entries[0].resp {
            *t = Term::Nil;
        }

        let mut ctx2 = EvalCtx::new();
        let prelude2 = build_prelude(&mut ctx2);
        let mut env2 = prelude2.env;
        let prog2 = eval_module(&mut ctx2, &mut env2, &forms).expect("eval2");

        let err = replay(&mut ctx2, prog2, &r1.log).unwrap_err();
        assert!(
            matches!(err, EffectsError::ReplayMismatch(_)),
            "expected replay mismatch, got {err}"
        );
    }

    #[test]
    fn replay_detects_tampered_op() {
        let (forms, h) = mk_prog();

        let mut ctx1 = EvalCtx::new();
        let prelude1 = build_prelude(&mut ctx1);
        let mut env1 = prelude1.env;
        let prog1 = eval_module(&mut ctx1, &mut env1, &forms).expect("eval1");

        let pol = CapsPolicy::from_toml_str(r#"allow = ["sys/time::now"]"#).unwrap();
        let mut r1 = run(&mut ctx1, &pol, prog1, h, "gc_effects-test".to_string()).expect("run");

        r1.log.entries[0].op = "sys/time::nope".to_string();

        let mut ctx2 = EvalCtx::new();
        let prelude2 = build_prelude(&mut ctx2);
        let mut env2 = prelude2.env;
        let prog2 = eval_module(&mut ctx2, &mut env2, &forms).expect("eval2");

        let err = replay(&mut ctx2, prog2, &r1.log).unwrap_err();
        assert!(
            matches!(err, EffectsError::ReplayMismatch(_)),
            "expected replay mismatch, got {err}"
        );
    }

    #[test]
    fn replay_detects_tampered_payload_hash() {
        let (forms, h) = mk_prog();

        let mut ctx1 = EvalCtx::new();
        let prelude1 = build_prelude(&mut ctx1);
        let mut env1 = prelude1.env;
        let prog1 = eval_module(&mut ctx1, &mut env1, &forms).expect("eval1");

        let pol = CapsPolicy::from_toml_str(r#"allow = ["sys/time::now"]"#).unwrap();
        let mut r1 = run(&mut ctx1, &pol, prog1, h, "gc_effects-test".to_string()).expect("run");

        r1.log.entries[0].payload_h[0] ^= 0xff;

        let mut ctx2 = EvalCtx::new();
        let prelude2 = build_prelude(&mut ctx2);
        let mut env2 = prelude2.env;
        let prog2 = eval_module(&mut ctx2, &mut env2, &forms).expect("eval2");

        let err = replay(&mut ctx2, prog2, &r1.log).unwrap_err();
        assert!(
            matches!(err, EffectsError::ReplayMismatch(_)),
            "expected replay mismatch, got {err}"
        );
    }

    #[test]
    fn replay_detects_tampered_continuation_hash() {
        let (forms, h) = mk_prog();

        let mut ctx1 = EvalCtx::new();
        let prelude1 = build_prelude(&mut ctx1);
        let mut env1 = prelude1.env;
        let prog1 = eval_module(&mut ctx1, &mut env1, &forms).expect("eval1");

        let pol = CapsPolicy::from_toml_str(r#"allow = ["sys/time::now"]"#).unwrap();
        let mut r1 = run(&mut ctx1, &pol, prog1, h, "gc_effects-test".to_string()).expect("run");

        r1.log.entries[0].cont_h[0] ^= 0xff;

        let mut ctx2 = EvalCtx::new();
        let prelude2 = build_prelude(&mut ctx2);
        let mut env2 = prelude2.env;
        let prog2 = eval_module(&mut ctx2, &mut env2, &forms).expect("eval2");

        let err = replay(&mut ctx2, prog2, &r1.log).unwrap_err();
        assert!(
            matches!(err, EffectsError::ReplayMismatch(_)),
            "expected replay mismatch, got {err}"
        );
    }

    #[test]
    fn replay_detects_tampered_request_hash() {
        let (forms, h) = mk_prog();

        let mut ctx1 = EvalCtx::new();
        let prelude1 = build_prelude(&mut ctx1);
        let mut env1 = prelude1.env;
        let prog1 = eval_module(&mut ctx1, &mut env1, &forms).expect("eval1");

        let pol = CapsPolicy::from_toml_str(r#"allow = ["sys/time::now"]"#).unwrap();
        let mut r1 = run(&mut ctx1, &pol, prog1, h, "gc_effects-test".to_string()).expect("run");

        r1.log.entries[0].req_h[0] ^= 0xff;

        let mut ctx2 = EvalCtx::new();
        let prelude2 = build_prelude(&mut ctx2);
        let mut env2 = prelude2.env;
        let prog2 = eval_module(&mut ctx2, &mut env2, &forms).expect("eval2");

        let err = replay(&mut ctx2, prog2, &r1.log).unwrap_err();
        assert!(
            matches!(err, EffectsError::ReplayMismatch(_)),
            "expected replay mismatch, got {err}"
        );
    }

    #[test]
    fn task_log_includes_schedule_and_await_metadata() {
        let (forms, h) = mk_prog_for("core/task::await", "{:task-id \"task-1\"}");

        let mut ctx = EvalCtx::new();
        let prelude = build_prelude(&mut ctx);
        let mut env = prelude.env;
        let prog = eval_module(&mut ctx, &mut env, &forms).expect("eval");

        let pol = CapsPolicy::from_toml_str(r#"allow = ["core/task::await"]"#).unwrap();
        let r = run(&mut ctx, &pol, prog, h, "gc_effects-test".to_string()).expect("run");

        assert_eq!(r.log.version, 3);
        let e = &r.log.entries[0];
        assert_eq!(e.schedule_step, Some(0));
        assert_eq!(e.task_id.as_deref(), Some("task-1"));
        assert_eq!(e.await_edge.as_deref(), Some("task-1"));
    }

    #[test]
    fn replay_detects_tampered_schedule_step_for_task_events() {
        let (forms, h) = mk_prog_for("core/task::await", "{:task-id \"task-1\"}");

        let mut ctx1 = EvalCtx::new();
        let prelude1 = build_prelude(&mut ctx1);
        let mut env1 = prelude1.env;
        let prog1 = eval_module(&mut ctx1, &mut env1, &forms).expect("eval1");

        let pol = CapsPolicy::from_toml_str(r#"allow = ["core/task::await"]"#).unwrap();
        let mut r1 = run(&mut ctx1, &pol, prog1, h, "gc_effects-test".to_string()).expect("run");
        r1.log.entries[0].schedule_step = Some(99);

        let mut ctx2 = EvalCtx::new();
        let prelude2 = build_prelude(&mut ctx2);
        let mut env2 = prelude2.env;
        let prog2 = eval_module(&mut ctx2, &mut env2, &forms).expect("eval2");

        let err = replay(&mut ctx2, prog2, &r1.log).unwrap_err();
        assert!(
            matches!(err, EffectsError::ReplayMismatch(_)),
            "expected replay mismatch, got {err}"
        );
    }

    #[test]
    fn replay_detects_tampered_await_edge_for_task_events() {
        let (forms, h) = mk_prog_for("core/task::await", "{:task-id \"task-1\"}");

        let mut ctx1 = EvalCtx::new();
        let prelude1 = build_prelude(&mut ctx1);
        let mut env1 = prelude1.env;
        let prog1 = eval_module(&mut ctx1, &mut env1, &forms).expect("eval1");

        let pol = CapsPolicy::from_toml_str(r#"allow = ["core/task::await"]"#).unwrap();
        let mut r1 = run(&mut ctx1, &pol, prog1, h, "gc_effects-test".to_string()).expect("run");
        r1.log.entries[0].await_edge = Some("task-other".to_string());

        let mut ctx2 = EvalCtx::new();
        let prelude2 = build_prelude(&mut ctx2);
        let mut env2 = prelude2.env;
        let prog2 = eval_module(&mut ctx2, &mut env2, &forms).expect("eval2");

        let err = replay(&mut ctx2, prog2, &r1.log).unwrap_err();
        assert!(
            matches!(err, EffectsError::ReplayMismatch(_)),
            "expected replay mismatch, got {err}"
        );
    }

    #[test]
    fn task_policy_max_tasks_limit_overrides_capability_result() {
        let (forms, h) = mk_prog_for("core/task::await", "{:task-id \"task-1\"}");

        let mut ctx = EvalCtx::new();
        let prelude = build_prelude(&mut ctx);
        let mut env = prelude.env;
        let prog = eval_module(&mut ctx, &mut env, &forms).expect("eval");

        let pol = CapsPolicy::from_toml_str(
            r#"
allow = ["core/task::await"]

[task]
max_tasks = 0
"#,
        )
        .unwrap();
        let r = run(&mut ctx, &pol, prog, h, "gc_effects-test".to_string()).expect("run");

        assert_eq!(r.log.entries.len(), 1);
        assert_eq!(r.log.entries[0].decision, Decision::Deny);
        match r.value {
            Value::Sealed { token, payload } => {
                assert_eq!(token, ctx.protocol.expect("protocol").error);
                let Value::Data(Term::Map(m)) = payload.as_ref() else {
                    panic!("expected error payload map");
                };
                assert_eq!(
                    m.get(&TermOrdKey(Term::symbol(":error/code"))),
                    Some(&Term::Str("core/task/budget-exceeded".to_string()))
                );
            }
            other => panic!("expected sealed budget error, got {}", other.debug_repr()),
        }
    }

    #[test]
    fn deterministic_task_scheduler_spawn_status_await_roundtrip_replays() {
        let src = r#"
            (def prog
              ((core/effect::bind (((core/task::spawn "scope/main") "build") {:job "compile"}))
                (fn (spawn-resp)
                  (let ((tid ((core/map::get spawn-resp) ':task-id)))
                    ((core/effect::bind (core/task::status tid))
                      (fn (status-resp)
                        ((core/effect::bind (core/task::await tid))
                          (fn (await-resp)
                            (core/effect::pure {:spawn spawn-resp :status status-resp :await await-resp})))))))))
            prog
        "#;
        let forms = parse_module(src).expect("parse module");
        let h = hash_module(&forms);

        let mut ctx1 = EvalCtx::new();
        let prelude1 = build_prelude(&mut ctx1);
        let mut env1 = prelude1.env;
        let prog1 = eval_module(&mut ctx1, &mut env1, &forms).expect("eval1");

        let pol = CapsPolicy::from_toml_str(
            r#"allow = ["core/task::spawn", "core/task::status", "core/task::await"]"#,
        )
        .unwrap();
        let r1 = run(&mut ctx1, &pol, prog1, h, "gc_effects-test".to_string()).expect("run");
        assert_eq!(r1.log.entries.len(), 3);
        assert_eq!(r1.log.entries[0].op, "core/task::spawn");
        assert_eq!(r1.log.entries[1].op, "core/task::status");
        assert_eq!(r1.log.entries[2].op, "core/task::await");

        let await_state = match &r1.value {
            Value::Data(Term::Map(root)) => match root.get(&TermOrdKey(Term::symbol(":await"))) {
                Some(Term::Map(await_m)) => await_m
                    .get(&TermOrdKey(Term::symbol(":state")))
                    .and_then(|t| match t {
                        Term::Symbol(s) => Some(s.clone()),
                        _ => None,
                    }),
                _ => None,
            },
            Value::Map(root) => match root.get(&TermOrdKey(Term::symbol(":await"))) {
                Some(Value::Map(await_m)) => await_m
                    .get(&TermOrdKey(Term::symbol(":state")))
                    .and_then(|t| match t {
                        Value::Data(Term::Symbol(s)) => Some(s.clone()),
                        _ => None,
                    }),
                Some(Value::Data(Term::Map(await_m))) => await_m
                    .get(&TermOrdKey(Term::symbol(":state")))
                    .and_then(|t| match t {
                        Term::Symbol(s) => Some(s.clone()),
                        _ => None,
                    }),
                _ => None,
            },
            _ => None,
        };
        assert_eq!(await_state.as_deref(), Some(":done"));

        let mut ctx2 = EvalCtx::new();
        let prelude2 = build_prelude(&mut ctx2);
        let mut env2 = prelude2.env;
        let prog2 = eval_module(&mut ctx2, &mut env2, &forms).expect("eval2");
        let v2 = replay(&mut ctx2, prog2, &r1.log).expect("replay");
        assert_eq!(value_hash(&r1.value), value_hash(&v2));
    }

    #[test]
    fn deterministic_task_scheduler_cancelled_task_awaits_as_cancelled() {
        let src = r#"
            (def prog
              ((core/effect::bind (((core/task::spawn "scope/main") "build") {:job "compile"}))
                (fn (spawn-resp)
                  (let ((tid ((core/map::get spawn-resp) ':task-id)))
                    ((core/effect::bind (core/task::cancel tid))
                      (fn (_cancel-resp)
                        ((core/effect::bind (core/task::await tid))
                          (fn (await-resp)
                            (core/effect::pure await-resp)))))))))
            prog
        "#;
        let forms = parse_module(src).expect("parse module");
        let h = hash_module(&forms);

        let mut ctx = EvalCtx::new();
        let prelude = build_prelude(&mut ctx);
        let mut env = prelude.env;
        let prog = eval_module(&mut ctx, &mut env, &forms).expect("eval");

        let pol = CapsPolicy::from_toml_str(
            r#"allow = ["core/task::spawn", "core/task::cancel", "core/task::await"]"#,
        )
        .unwrap();
        let r = run(&mut ctx, &pol, prog, h, "gc_effects-test".to_string()).expect("run");
        assert_eq!(r.log.entries.len(), 3);
        let state = match &r.value {
            Value::Data(Term::Map(m)) => {
                m.get(&TermOrdKey(Term::symbol(":state")))
                    .and_then(|t| match t {
                        Term::Symbol(s) => Some(s.clone()),
                        _ => None,
                    })
            }
            Value::Map(m) => m
                .get(&TermOrdKey(Term::symbol(":state")))
                .and_then(|t| match t {
                    Value::Data(Term::Symbol(s)) => Some(s.clone()),
                    _ => None,
                }),
            _ => None,
        };
        assert_eq!(state.as_deref(), Some(":cancelled"));
    }

    #[test]
    fn deterministic_task_scheduler_enqueues_and_promotes_with_worker_budget() {
        let src = r#"
            (def prog
              ((core/effect::bind (((core/task::spawn "scope/main") "t1") {:job "one"}))
                (fn (spawn-1)
                  (let ((tid-1 ((core/map::get spawn-1) ':task-id)))
                    ((core/effect::bind (((core/task::spawn "scope/main") "t2") {:job "two"}))
                      (fn (spawn-2)
                        (let ((tid-2 ((core/map::get spawn-2) ':task-id)))
                          ((core/effect::bind (core/task::status tid-2))
                            (fn (before)
                              ((core/effect::bind (core/task::await tid-1))
                                (fn (_await-1)
                                  ((core/effect::bind (core/task::status tid-2))
                                    (fn (after)
                                      (core/effect::pure {:before before :after after}))))))))))))))
            prog
        "#;
        let forms = parse_module(src).expect("parse module");
        let h = hash_module(&forms);

        let mut ctx = EvalCtx::new();
        let prelude = build_prelude(&mut ctx);
        let mut env = prelude.env;
        let prog = eval_module(&mut ctx, &mut env, &forms).expect("eval");

        let pol = CapsPolicy::from_toml_str(
            r#"
allow = ["core/task::spawn", "core/task::status", "core/task::await"]

[task]
max_workers = 1
"#,
        )
        .unwrap();
        let r = run(&mut ctx, &pol, prog, h, "gc_effects-test".to_string()).expect("run");

        fn state_from_term_entry(t: &Term) -> Option<String> {
            let Term::Map(m) = t else {
                return None;
            };
            m.get(&TermOrdKey(Term::symbol(":state")))
                .and_then(|x| match x {
                    Term::Symbol(s) => Some(s.clone()),
                    _ => None,
                })
        }

        fn state_from_value_entry(v: &Value) -> Option<String> {
            match v {
                Value::Data(t) => state_from_term_entry(t),
                Value::Map(m) => m
                    .get(&TermOrdKey(Term::symbol(":state")))
                    .and_then(|x| match x {
                        Value::Data(Term::Symbol(s)) => Some(s.clone()),
                        _ => None,
                    }),
                _ => None,
            }
        }

        let before_state = match &r.value {
            Value::Data(Term::Map(root)) => root
                .get(&TermOrdKey(Term::symbol(":before")))
                .and_then(state_from_term_entry),
            Value::Map(root) => root
                .get(&TermOrdKey(Term::symbol(":before")))
                .and_then(state_from_value_entry),
            _ => None,
        };
        let after_state = match &r.value {
            Value::Data(Term::Map(root)) => root
                .get(&TermOrdKey(Term::symbol(":after")))
                .and_then(state_from_term_entry),
            Value::Map(root) => root
                .get(&TermOrdKey(Term::symbol(":after")))
                .and_then(state_from_value_entry),
            _ => None,
        };

        assert_eq!(before_state.as_deref(), Some(":queued"));
        assert_eq!(after_state.as_deref(), Some(":running"));
    }

    #[test]
    fn task_policy_max_queue_limit_denies_spawn_and_returns_budget_error() {
        let (forms, h) = mk_prog_for(
            "core/task::spawn",
            "{:scope \"scope/main\" :payload {:job \"compile\"}}",
        );

        let mut ctx = EvalCtx::new();
        let prelude = build_prelude(&mut ctx);
        let mut env = prelude.env;
        let prog = eval_module(&mut ctx, &mut env, &forms).expect("eval");

        let pol = CapsPolicy::from_toml_str(
            r#"
allow = ["core/task::spawn"]

[task]
max_workers = 0
max_queue = 0
"#,
        )
        .unwrap();
        let r = run(&mut ctx, &pol, prog, h, "gc_effects-test".to_string()).expect("run");

        assert_eq!(r.log.entries.len(), 1);
        assert_eq!(r.log.entries[0].decision, Decision::Deny);
        match r.value {
            Value::Sealed { token, payload } => {
                assert_eq!(token, ctx.protocol.expect("protocol").error);
                let Value::Data(Term::Map(m)) = payload.as_ref() else {
                    panic!("expected error payload map");
                };
                assert_eq!(
                    m.get(&TermOrdKey(Term::symbol(":error/code"))),
                    Some(&Term::Str("core/task/budget-exceeded".to_string()))
                );
                assert_eq!(
                    m.get(&TermOrdKey(Term::symbol(":error/op"))),
                    Some(&Term::Symbol("core/task::spawn".to_string()))
                );
            }
            other => panic!("expected sealed budget error, got {}", other.debug_repr()),
        }
    }

    #[test]
    fn large_byte_responses_are_externalized_to_artifact_store_and_replay_loads_them() {
        let td = tempfile::tempdir().unwrap();
        let base = td.path().join("sandbox");
        std::fs::create_dir_all(&base).unwrap();
        std::fs::write(base.join("in.bin"), vec![7u8; 128]).unwrap();

        let caps_path = td.path().join("caps.toml");
        std::fs::write(
            &caps_path,
            r#"
allow = ["io/fs::read"]

[log]
inline_max_bytes = 8
store_dir = "./.genesis/store"

[op."io/fs::read"]
base_dir = "./sandbox"
"#,
        )
        .unwrap();
        let pol = CapsPolicy::load(&caps_path).unwrap();

        let src = r#"
            (def prog
              (core/effect::perform
                'io/fs::read
                {:path "in.bin"}
                (fn (b) (core/effect::pure b))))
            prog
        "#;
        let forms = parse_module(src).expect("parse module");
        let h = hash_module(&forms);

        let mut ctx1 = EvalCtx::new();
        let prelude1 = build_prelude(&mut ctx1);
        let mut env1 = prelude1.env;
        let prog1 = eval_module(&mut ctx1, &mut env1, &forms).expect("eval1");

        let r1 = run(&mut ctx1, &pol, prog1, h, "gc_effects-test".to_string()).expect("run");

        assert!(matches!(
            r1.log.entries[0].resp,
            LoggedResp::OkBytesArtifact { .. }
        ));

        let store = ArtifactStore::open(pol.store_dir().unwrap()).unwrap();
        let mut ctx2 = EvalCtx::new();
        let prelude2 = build_prelude(&mut ctx2);
        let mut env2 = prelude2.env;
        let prog2 = eval_module(&mut ctx2, &mut env2, &forms).expect("eval2");

        let v2 = replay_with_store(&mut ctx2, prog2, &r1.log, Some(&store)).expect("replay");
        assert_eq!(value_hash(&r1.value), value_hash(&v2));

        // Without a store, replay must fail deterministically.
        let mut ctx3 = EvalCtx::new();
        let prelude3 = build_prelude(&mut ctx3);
        let mut env3 = prelude3.env;
        let prog3 = eval_module(&mut ctx3, &mut env3, &forms).expect("eval3");
        let err = replay(&mut ctx3, prog3, &r1.log).unwrap_err();
        assert!(
            matches!(err, EffectsError::ReplayMismatch(_)),
            "expected replay mismatch, got {err}"
        );
    }

    #[test]
    fn cap_term_omits_base_dir_paths_for_deterministic_logs() {
        let td = tempfile::tempdir().unwrap();
        let base = td.path().join("sandbox");
        std::fs::create_dir_all(&base).unwrap();
        std::fs::write(base.join("in.bin"), vec![1u8, 2, 3]).unwrap();

        let caps_path = td.path().join("caps.toml");
        std::fs::write(
            &caps_path,
            r#"
allow = ["io/fs::read"]

[op."io/fs::read"]
base_dir = "./sandbox"
"#,
        )
        .unwrap();
        let pol = CapsPolicy::load(&caps_path).unwrap();

        let src = r#"
            (def prog
              (core/effect::perform
                'io/fs::read
                {:path "in.bin"}
                (fn (b) (core/effect::pure b))))
            prog
        "#;
        let forms = parse_module(src).expect("parse module");
        let h = hash_module(&forms);

        let mut ctx = EvalCtx::new();
        let prelude = build_prelude(&mut ctx);
        let mut env = prelude.env;
        let prog = eval_module(&mut ctx, &mut env, &forms).expect("eval");

        let r = run(&mut ctx, &pol, prog, h, "gc_effects-test".to_string()).expect("run");
        let Term::Map(m) = &r.log.entries[0].cap else {
            panic!("expected cap map");
        };
        assert!(
            !m.contains_key(&TermOrdKey(Term::symbol(":base-dir"))),
            "cap term must not include :base-dir"
        );
    }

    #[test]
    fn gfx_frame_tick_is_supported_and_replayable() {
        let (forms, h) = mk_prog_for("gfx/time::frame-tick", "{:surface \"main\"}");

        let mut ctx1 = EvalCtx::new();
        let prelude1 = build_prelude(&mut ctx1);
        let mut env1 = prelude1.env;
        let prog1 = eval_module(&mut ctx1, &mut env1, &forms).expect("eval1");

        let pol = CapsPolicy::from_toml_str(r#"allow = ["gfx/time::frame-tick"]"#).unwrap();
        let r1 = run(&mut ctx1, &pol, prog1, h, "gc_effects-test".to_string()).expect("run");

        match &r1.value {
            Value::Data(Term::Map(m)) => {
                assert!(matches!(
                    m.get(&TermOrdKey(Term::symbol(":time-ms"))),
                    Some(Term::Int(_))
                ));
            }
            other => panic!("expected frame-tick map, got {}", other.debug_repr()),
        }

        let mut ctx2 = EvalCtx::new();
        let prelude2 = build_prelude(&mut ctx2);
        let mut env2 = prelude2.env;
        let prog2 = eval_module(&mut ctx2, &mut env2, &forms).expect("eval2");
        let v2 = replay(&mut ctx2, prog2, &r1.log).expect("replay");
        assert_eq!(value_hash(&r1.value), value_hash(&v2));
    }

    #[test]
    fn task_runtime_executes_parallel_work_with_worker_pool() {
        let src = r#"
            (def prog
              ((core/effect::bind (((core/task::spawn "scope/main") "t1")
                                   {:task/sleep-ms 350 :task/result {:ok "t1"}}))
                (fn (spawn-1)
                  (let ((tid-1 ((core/map::get spawn-1) ':task-id)))
                    ((core/effect::bind (((core/task::spawn "scope/main") "t2")
                                         {:task/sleep-ms 350 :task/result {:ok "t2"}}))
                      (fn (spawn-2)
                        (let ((tid-2 ((core/map::get spawn-2) ':task-id)))
                          ((core/effect::bind (core/task::await tid-1))
                            (fn (r1)
                              ((core/effect::bind (core/task::await tid-2))
                                (fn (r2)
                                  (core/effect::pure {:r1 r1 :r2 r2}))))))))))))
            prog
        "#;
        let forms = parse_module(src).expect("parse module");
        let h = hash_module(&forms);

        let run_elapsed_ms = |max_workers: u64| {
            let mut ctx = EvalCtx::new();
            let prelude = build_prelude(&mut ctx);
            let mut env = prelude.env;
            let prog = eval_module(&mut ctx, &mut env, &forms).expect("eval");

            let policy = format!(
                r#"
allow = ["core/task::spawn", "core/task::await"]

[task]
max_workers = {max_workers}
"#
            );
            let pol = CapsPolicy::from_toml_str(&policy).unwrap();

            let started = std::time::Instant::now();
            let r = run(&mut ctx, &pol, prog, h, "gc_effects-test".to_string()).expect("run");
            assert_eq!(r.log.entries.len(), 4);
            assert_eq!(r.log.entries[0].op, "core/task::spawn");
            assert_eq!(r.log.entries[1].op, "core/task::spawn");
            assert_eq!(r.log.entries[2].op, "core/task::await");
            assert_eq!(r.log.entries[3].op, "core/task::await");
            started.elapsed().as_millis()
        };

        let elapsed_parallel_ms = run_elapsed_ms(2);
        let elapsed_serial_ms = run_elapsed_ms(1);
        assert!(
            elapsed_parallel_ms + 180 < elapsed_serial_ms,
            "expected max_workers=2 runtime to be materially faster; parallel={elapsed_parallel_ms}ms serial={elapsed_serial_ms}ms"
        );
        assert!(
            elapsed_parallel_ms >= 280,
            "parallel runtime should still reflect real task work, got {elapsed_parallel_ms}ms"
        );
    }

    #[test]
    fn task_runtime_await_surfaces_failed_state_and_error_payload() {
        let src = r#"
            (def prog
              ((core/effect::bind (((core/task::spawn "scope/main") "t1")
                                   {:task/error {:reason "boom"}}))
                (fn (spawn-1)
                  (let ((tid ((core/map::get spawn-1) ':task-id)))
                    (core/task::await tid)))))
            prog
        "#;
        let forms = parse_module(src).expect("parse module");
        let h = hash_module(&forms);

        let mut ctx = EvalCtx::new();
        let prelude = build_prelude(&mut ctx);
        let mut env = prelude.env;
        let prog = eval_module(&mut ctx, &mut env, &forms).expect("eval");

        let pol = CapsPolicy::from_toml_str(r#"allow = ["core/task::spawn", "core/task::await"]"#)
            .unwrap();
        let r = run(&mut ctx, &pol, prog, h, "gc_effects-test".to_string()).expect("run");

        let Value::Data(Term::Map(m)) = r.value else {
            panic!("expected task await map result");
        };
        assert_eq!(
            m.get(&TermOrdKey(Term::symbol(":state"))),
            Some(&Term::symbol(":failed"))
        );
        assert_eq!(
            m.get(&TermOrdKey(Term::symbol(":result"))),
            Some(&Term::Nil)
        );
        let Some(Term::Map(err)) = m.get(&TermOrdKey(Term::symbol(":error"))) else {
            panic!("expected :error map for failed task");
        };
        assert_eq!(
            err.get(&TermOrdKey(Term::symbol(":reason"))),
            Some(&Term::Str("boom".to_string()))
        );
    }

    #[test]
    fn editor_clipboard_capability_roundtrip_is_supported_and_replayable() {
        let (forms, h) = mk_prog_for("editor/clipboard::get", "{}");

        let mut ctx1 = EvalCtx::new();
        let prelude1 = build_prelude(&mut ctx1);
        let mut env1 = prelude1.env;
        let prog1 = eval_module(&mut ctx1, &mut env1, &forms).expect("eval1");

        let pol = CapsPolicy::from_toml_str(r#"allow = ["editor/clipboard::get"]"#).unwrap();
        let r1 = run(&mut ctx1, &pol, prog1, h, "gc_effects-test".to_string()).expect("run");
        match &r1.value {
            Value::Data(Term::Map(m)) => {
                assert_eq!(
                    m.get(&TermOrdKey(Term::symbol(":ok"))),
                    Some(&Term::Bool(true))
                );
                assert_eq!(
                    m.get(&TermOrdKey(Term::symbol(":mime"))),
                    Some(&Term::Str("text/plain".to_string()))
                );
            }
            other => panic!(
                "expected clipboard map response, got {}",
                other.debug_repr()
            ),
        }

        let mut ctx2 = EvalCtx::new();
        let prelude2 = build_prelude(&mut ctx2);
        let mut env2 = prelude2.env;
        let prog2 = eval_module(&mut ctx2, &mut env2, &forms).expect("eval2");
        let v2 = replay(&mut ctx2, prog2, &r1.log).expect("replay");
        assert_eq!(value_hash(&r1.value), value_hash(&v2));
    }

    #[test]
    fn gfx_window_input_audio_backends_are_supported_and_replayable() {
        let pol = CapsPolicy::from_toml_str(
            r#"allow = ["gfx/window::create-surface", "gfx/window::request-redraw", "gfx/window::surface-info", "gfx/input::poll-events", "gfx/input::set-cursor-mode", "gfx/audio::set-master", "gfx/audio::enqueue"]"#,
        )
        .unwrap();
        let cases = [
            r#"
            (def prog
              (core/effect::perform
                'gfx/window::create-surface
                {:opts {:height 600 :title "main" :width 800}}
                (fn (x) (core/effect::pure x))))
            prog
            "#,
            r#"
            (def prog
              ((core/effect::bind (core/effect::perform
                                    'gfx/window::create-surface
                                    {:opts {:height 600 :title "main" :width 800}}
                                    (fn (x) (core/effect::pure x))))
                (fn (surface-resp)
                  (let ((sid ((core/map::get surface-resp) ':surface)))
                    ((core/effect::bind (core/effect::perform
                                          'gfx/window::request-redraw
                                          {:surface sid}
                                          (fn (x) (core/effect::pure x))))
                      (fn (_)
                        (core/effect::perform
                          'gfx/input::poll-events
                          {:surface sid}
                          (fn (x) (core/effect::pure x)))))))))
            prog
            "#,
            r#"
            (def prog
              ((core/effect::bind (core/effect::perform
                                    'gfx/window::create-surface
                                    {:opts {:height 600 :title "main" :width 800}}
                                    (fn (x) (core/effect::pure x))))
                (fn (surface-resp)
                  (let ((sid ((core/map::get surface-resp) ':surface)))
                    ((core/effect::bind (core/effect::perform
                                          'gfx/input::set-cursor-mode
                                          {:mode "hidden" :surface sid}
                                          (fn (x) (core/effect::pure x))))
                      (fn (_)
                        (core/effect::perform
                          'gfx/window::surface-info
                          {:surface sid}
                          (fn (x) (core/effect::pure x)))))))))
            prog
            "#,
            r#"
            (def prog
              ((core/effect::bind (core/effect::perform
                                    'gfx/audio::set-master
                                    {:gain 1}
                                    (fn (x) (core/effect::pure x))))
                (fn (_)
                  (core/effect::perform
                    'gfx/audio::enqueue
                    {:event {:kind "beep"}}
                    (fn (x) (core/effect::pure x))))))
            prog
            "#,
        ];

        for src in cases {
            let forms = parse_module(src).expect("parse module");
            let h = hash_module(&forms);
            let mut ctx1 = EvalCtx::new();
            let prelude1 = build_prelude(&mut ctx1);
            let mut env1 = prelude1.env;
            let prog1 = eval_module(&mut ctx1, &mut env1, &forms).expect("eval1");
            let r1 = run(&mut ctx1, &pol, prog1, h, "gc_effects-test".to_string()).expect("run");
            let mut ctx2 = EvalCtx::new();
            let prelude2 = build_prelude(&mut ctx2);
            let mut env2 = prelude2.env;
            let prog2 = eval_module(&mut ctx2, &mut env2, &forms).expect("eval2");
            let v2 = replay(&mut ctx2, prog2, &r1.log).expect("replay");
            assert_eq!(value_hash(&r1.value), value_hash(&v2));
        }
    }

    #[test]
    fn gfx_gpu_backend_is_supported_and_replayable() {
        let pol = CapsPolicy::from_toml_str(
            r#"allow = ["gfx/gpu::create-buffer", "gfx/gpu::write-buffer", "gfx/gpu::read-buffer", "gfx/gpu::create-texture", "gfx/gpu::write-texture", "gfx/gpu::read-texture", "gfx/gpu::submit-frame-graph", "gfx/gpu::submit-compute-graph", "gfx/gpu::limits", "gfx/gpu::features"]"#,
        )
        .unwrap();
        let cases = [
            r#"
            (def prog
              ((core/effect::bind (core/effect::perform
                                    'gfx/gpu::create-buffer
                                    {:desc {:size 8}}
                                    (fn (x) (core/effect::pure x))))
                (fn (create-resp)
                  (let ((id ((core/map::get create-resp) ':id)))
                    ((core/effect::bind (core/effect::perform
                                          'gfx/gpu::write-buffer
                                          {:data b"\x01\x02\x03" :id id :offset 2}
                                          (fn (x) (core/effect::pure x))))
                      (fn (_)
                        (core/effect::perform
                          'gfx/gpu::read-buffer
                          {:id id :offset 0 :size 8}
                          (fn (x) (core/effect::pure x)))))))))
            prog
            "#,
            r#"
            (def prog
              ((core/effect::bind (core/effect::perform
                                    'gfx/gpu::create-texture
                                    {:desc {:byte-size 6}}
                                    (fn (x) (core/effect::pure x))))
                (fn (create-resp)
                  (let ((id ((core/map::get create-resp) ':id)))
                    ((core/effect::bind (core/effect::perform
                                          'gfx/gpu::write-texture
                                          {:data b"\xAA\xBB\xCC\xDD\xEE\xFF" :id id :layout {}}
                                          (fn (x) (core/effect::pure x))))
                      (fn (_)
                        (core/effect::perform
                          'gfx/gpu::read-texture
                          {:id id :region {:offset 1 :size 3}}
                          (fn (x) (core/effect::pure x)))))))))
            prog
            "#,
            r#"
            (def prog
              ((core/effect::bind (core/effect::perform
                                    'gfx/gpu::submit-frame-graph
                                    {:graph {:compute-passes [] :render-passes []}}
                                    (fn (x) (core/effect::pure x))))
                (fn (_)
                  ((core/effect::bind (core/effect::perform
                                        'gfx/gpu::submit-compute-graph
                                        {:graph {:passes []}}
                                        (fn (x) (core/effect::pure x))))
                    (fn (_)
                      ((core/effect::bind (core/effect::perform
                                            'gfx/gpu::limits
                                            {}
                                            (fn (x) (core/effect::pure x))))
                        (fn (_)
                          (core/effect::perform
                            'gfx/gpu::features
                            {}
                            (fn (x) (core/effect::pure x))))))))))
            prog
            "#,
        ];

        for src in cases {
            let forms = parse_module(src).expect("parse module");
            let h = hash_module(&forms);
            let mut ctx1 = EvalCtx::new();
            let prelude1 = build_prelude(&mut ctx1);
            let mut env1 = prelude1.env;
            let prog1 = eval_module(&mut ctx1, &mut env1, &forms).expect("eval1");
            let r1 = run(&mut ctx1, &pol, prog1, h, "gc_effects-test".to_string()).expect("run");
            assert!(
                !matches!(r1.value, Value::Sealed { .. }),
                "gpu backend should return structured responses, got {}",
                r1.value.debug_repr()
            );

            let mut ctx2 = EvalCtx::new();
            let prelude2 = build_prelude(&mut ctx2);
            let mut env2 = prelude2.env;
            let prog2 = eval_module(&mut ctx2, &mut env2, &forms).expect("eval2");
            let v2 = replay(&mut ctx2, prog2, &r1.log).expect("replay");
            assert_eq!(value_hash(&r1.value), value_hash(&v2));
        }
    }

    #[test]
    fn editor_task_and_watch_backends_are_supported_and_replayable() {
        let src = r#"
            (def prog
              ((core/effect::bind (core/effect::perform
                                    'editor/watch::subscribe
                                    {:globs ["*.gc"] :root "workspace"}
                                    (fn (x) (core/effect::pure x))))
                (fn (watch-resp)
                  (let ((watch-id ((core/map::get watch-resp) ':watch-id)))
                    ((core/effect::bind (core/effect::perform
                                          'editor/task::spawn
                                          {:budget-ms nil
                                           :input {:source "(def x 1)"}
                                           :task-kind 'editor/task::parse-module}
                                          (fn (x) (core/effect::pure x))))
                      (fn (spawn-resp)
                        (let ((task-id ((core/map::get spawn-resp) ':task-id)))
                          ((core/effect::bind (core/effect::perform
                                                'editor/task::poll
                                                {:task-id task-id}
                                                (fn (x) (core/effect::pure x))))
                            (fn (task-poll)
                              ((core/effect::bind (core/effect::perform
                                                    'editor/watch::poll
                                                    {:watch-id watch-id}
                                                    (fn (x) (core/effect::pure x))))
                                (fn (watch-poll)
                                  (core/effect::pure {:task task-poll :watch watch-poll}))))))))))))
            prog
        "#;
        let forms = parse_module(src).expect("parse module");
        let h = hash_module(&forms);
        let mut ctx1 = EvalCtx::new();
        let prelude1 = build_prelude(&mut ctx1);
        let mut env1 = prelude1.env;
        let prog1 = eval_module(&mut ctx1, &mut env1, &forms).expect("eval1");
        let pol = CapsPolicy::from_toml_str(
            r#"allow = ["editor/watch::subscribe", "editor/watch::poll", "editor/task::spawn", "editor/task::poll"]"#,
        )
        .unwrap();
        let r1 = run(&mut ctx1, &pol, prog1, h, "gc_effects-test".to_string()).expect("run");
        assert_eq!(r1.log.entries.len(), 4);

        let mut ctx2 = EvalCtx::new();
        let prelude2 = build_prelude(&mut ctx2);
        let mut env2 = prelude2.env;
        let prog2 = eval_module(&mut ctx2, &mut env2, &forms).expect("eval2");
        let v2 = replay(&mut ctx2, prog2, &r1.log).expect("replay");
        assert_eq!(value_hash(&r1.value), value_hash(&v2));
    }
}
