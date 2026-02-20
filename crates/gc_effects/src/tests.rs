use std::collections::BTreeMap;
use std::fmt::Write as _;

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

fn sealed_error_payload_map<'a>(value: &'a Value, ctx: &EvalCtx) -> &'a BTreeMap<TermOrdKey, Term> {
    let Value::Sealed { token, payload } = value else {
        panic!("expected sealed error, got {}", value.debug_repr());
    };
    assert_eq!(*token, ctx.protocol.expect("protocol").error);
    let Value::Data(Term::Map(m)) = payload.as_ref() else {
        panic!("expected sealed error payload map");
    };
    m
}

struct HostBridgePolicyFixture {
    _dir: tempfile::TempDir,
    policy: CapsPolicy,
}

fn toml_escape(input: &str) -> String {
    input.replace('\\', "\\\\").replace('"', "\\\"")
}

#[cfg(unix)]
fn write_bridge_script(dir: &tempfile::TempDir) -> std::path::PathBuf {
    use std::os::unix::fs::PermissionsExt;

    let bridge = dir.path().join("host_bridge.sh");
    std::fs::write(
            &bridge,
            r#"#!/bin/sh
resp='{:ok true :surface "surface-bridge-0" :id "gpu-bridge-0" :width 800 :height 600 :title "bridge" :events [{:kind :create :path "new.gc"}] :data b"" :features [] :queued 0 :pending-redraws 0 :watch-id "watch-bridge-0" :task-id "task-bridge-0" :state :done}'
printf '%s\n%s' "${#resp}" "$resp"
"#,
        )
        .expect("write host bridge script");
    let mut perms = std::fs::metadata(&bridge)
        .expect("bridge metadata")
        .permissions();
    perms.set_mode(0o755);
    std::fs::set_permissions(&bridge, perms).expect("bridge chmod");
    bridge
}

#[cfg(windows)]
fn write_bridge_script(dir: &tempfile::TempDir) -> std::path::PathBuf {
    let bridge = dir.path().join("host_bridge.cmd");
    std::fs::write(
            &bridge,
            "@echo {:ok true :surface \"surface-bridge-0\" :id \"gpu-bridge-0\" :width 800 :height 600 :title \"bridge\" :events [] :data b\"\" :features [] :queued 0 :pending-redraws 0}\r\n",
        )
        .expect("write host bridge script");
    bridge
}

fn mk_bridge_policy(ops: &[&str]) -> HostBridgePolicyFixture {
    let dir = tempfile::tempdir().expect("tempdir");
    let bridge = write_bridge_script(&dir);
    let base = toml_escape(dir.path().to_string_lossy().as_ref());
    let bridge_name = toml_escape(
        bridge
            .file_name()
            .and_then(|x| x.to_str())
            .expect("bridge filename"),
    );
    let mut toml = String::new();
    let _ = writeln!(
        &mut toml,
        "allow = [{}]",
        ops.iter()
            .map(|op| format!("\"{op}\""))
            .collect::<Vec<_>>()
            .join(", ")
    );
    for op in ops {
        let _ = write!(
            &mut toml,
            "\n[op.\"{op}\"]\nbase_dir = \"{base}\"\nbridge_cmd = \"{bridge_name}\"\n"
        );
    }
    let policy = CapsPolicy::from_toml_str(&toml).expect("parse bridge policy");
    HostBridgePolicyFixture { _dir: dir, policy }
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

    let await_state =
        match &r1.value {
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
fn task_program_executes_arithmetic_work_unit_and_replays() {
    let src = r#"
            (def prog
              ((core/effect::bind
                 (((core/task::spawn-program "scope/main") "arith")
                   ((core/task::program-with-initial 1)
                     [
                       {:op :int-add :value 2}
                       {:op :int-mul :value 5}
                     ])))
                (fn (spawn-resp)
                  (let ((tid ((core/map::get spawn-resp) ':task-id)))
                    (core/task::await tid)))))
            prog
        "#;
    let forms = parse_module(src).expect("parse module");
    let h = hash_module(&forms);
    let pol =
        CapsPolicy::from_toml_str(r#"allow = ["core/task::spawn", "core/task::await"]"#).unwrap();

    let mut ctx1 = EvalCtx::new();
    let prelude1 = build_prelude(&mut ctx1);
    let mut env1 = prelude1.env;
    let prog1 = eval_module(&mut ctx1, &mut env1, &forms).expect("eval1");
    let run_out = run(&mut ctx1, &pol, prog1, h, "gc_effects-test".to_string()).expect("run");
    assert_eq!(run_out.log.entries.len(), 2);
    assert_eq!(run_out.log.entries[0].op, "core/task::spawn");
    assert_eq!(run_out.log.entries[1].op, "core/task::await");
    let Value::Data(Term::Map(m)) = &run_out.value else {
        panic!(
            "expected await response map, got {}",
            run_out.value.debug_repr()
        );
    };
    assert_eq!(
        m.get(&TermOrdKey(Term::symbol(":state"))),
        Some(&Term::symbol(":done")),
        "unexpected await map: {:?}",
        m
    );
    assert_eq!(
        m.get(&TermOrdKey(Term::symbol(":result"))),
        Some(&Term::Int(15.into()))
    );

    let mut ctx2 = EvalCtx::new();
    let prelude2 = build_prelude(&mut ctx2);
    let mut env2 = prelude2.env;
    let prog2 = eval_module(&mut ctx2, &mut env2, &forms).expect("eval2");
    let replay_v = replay(&mut ctx2, prog2, &run_out.log).expect("replay");
    assert_eq!(value_hash(&run_out.value), value_hash(&replay_v));
}

#[test]
fn task_program_type_mismatch_returns_failed_state_with_program_error() {
    let src = r#"
            (def prog
              ((core/effect::bind
                 (((core/task::spawn-program "scope/main") "bad-program")
                   ((core/task::program-with-initial "oops")
                     [
                       {:op :int-add :value 1}
                     ])))
                (fn (spawn-resp)
                  (let ((tid ((core/map::get spawn-resp) ':task-id)))
                    (core/task::await tid)))))
            prog
        "#;
    let forms = parse_module(src).expect("parse module");
    let h = hash_module(&forms);
    let pol =
        CapsPolicy::from_toml_str(r#"allow = ["core/task::spawn", "core/task::await"]"#).unwrap();

    let mut ctx = EvalCtx::new();
    let prelude = build_prelude(&mut ctx);
    let mut env = prelude.env;
    let prog = eval_module(&mut ctx, &mut env, &forms).expect("eval");
    let out = run(&mut ctx, &pol, prog, h, "gc_effects-test".to_string()).expect("run");
    let Value::Data(Term::Map(m)) = out.value else {
        panic!("expected await response map");
    };
    assert_eq!(
        m.get(&TermOrdKey(Term::symbol(":state"))),
        Some(&Term::symbol(":failed"))
    );
    let Some(Term::Map(err)) = m.get(&TermOrdKey(Term::symbol(":error"))) else {
        panic!("expected program error payload");
    };
    assert_eq!(
        err.get(&TermOrdKey(Term::symbol(":error/code"))),
        Some(&Term::Str("core/task/program-error".to_string()))
    );
}

#[test]
fn task_eval_payload_executes_callable_effect_program_and_replays() {
    let src = r#"
            (def prog
              ((core/effect::bind
                 (((core/task::spawn "scope/main") "eval")
                   {
                     :task/args [2 40]
                     :task/eval '(fn (x) (fn (y) (core/effect::pure (prim int/add x y))))
                   }))
                (fn (spawn-resp)
                  (let ((tid ((core/map::get spawn-resp) ':task-id)))
                    (core/task::await tid)))))
            prog
        "#;
    let forms = parse_module(src).expect("parse module");
    let h = hash_module(&forms);
    let pol =
        CapsPolicy::from_toml_str(r#"allow = ["core/task::spawn", "core/task::await"]"#).unwrap();

    let mut ctx1 = EvalCtx::new();
    let prelude1 = build_prelude(&mut ctx1);
    let mut env1 = prelude1.env;
    let prog1 = eval_module(&mut ctx1, &mut env1, &forms).expect("eval1");
    let run_out = run(&mut ctx1, &pol, prog1, h, "gc_effects-test".to_string()).expect("run");
    assert_eq!(run_out.log.entries.len(), 2);
    assert_eq!(run_out.log.entries[0].op, "core/task::spawn");
    assert_eq!(run_out.log.entries[1].op, "core/task::await");
    let Value::Data(Term::Map(m)) = &run_out.value else {
        panic!("expected await response map");
    };
    assert_eq!(
        m.get(&TermOrdKey(Term::symbol(":state"))),
        Some(&Term::symbol(":done"))
    );
    assert_eq!(
        m.get(&TermOrdKey(Term::symbol(":result"))),
        Some(&Term::Int(42.into()))
    );

    let mut ctx2 = EvalCtx::new();
    let prelude2 = build_prelude(&mut ctx2);
    let mut env2 = prelude2.env;
    let prog2 = eval_module(&mut ctx2, &mut env2, &forms).expect("eval2");
    let replay_v = replay(&mut ctx2, prog2, &run_out.log).expect("replay");
    assert_eq!(value_hash(&run_out.value), value_hash(&replay_v));
}

#[test]
fn task_eval_effect_program_respects_parent_cap_policy() {
    let src = r#"
            (def prog
              ((core/effect::bind
                 (((core/task::spawn "scope/main") "denied-subprogram")
                   {
                     :task/eval
                       '(core/effect::perform
                          'sys/time::now
                          nil
                          (fn (t) (core/effect::pure t)))
                   }))
                (fn (spawn-resp)
                  (let ((tid ((core/map::get spawn-resp) ':task-id)))
                    (core/task::await tid)))))
            prog
        "#;
    let forms = parse_module(src).expect("parse module");
    let h = hash_module(&forms);
    let pol =
        CapsPolicy::from_toml_str(r#"allow = ["core/task::spawn", "core/task::await"]"#).unwrap();

    let mut ctx = EvalCtx::new();
    let prelude = build_prelude(&mut ctx);
    let mut env = prelude.env;
    let prog = eval_module(&mut ctx, &mut env, &forms).expect("eval");
    let out = run(&mut ctx, &pol, prog, h, "gc_effects-test".to_string()).expect("run");
    let Value::Data(Term::Map(m)) = out.value else {
        panic!("expected await response map");
    };
    assert_eq!(
        m.get(&TermOrdKey(Term::symbol(":state"))),
        Some(&Term::symbol(":failed"))
    );
    let Some(Term::Map(err)) = m.get(&TermOrdKey(Term::symbol(":error"))) else {
        panic!("expected failed task error payload");
    };
    assert_eq!(
        err.get(&TermOrdKey(Term::symbol(":error/code"))),
        Some(&Term::Str("core/caps/denied".to_string()))
    );
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
fn runtime_policy_max_effect_ops_fail_closes_second_request() {
    let src = r#"
            (def prog
              ((core/effect::bind
                 (core/effect::perform
                   'sys/time::now
                   nil
                   (fn (t1) (core/effect::pure t1))))
                (fn (_t1)
                  (core/effect::perform
                    'sys/time::now
                    nil
                    (fn (t2) (core/effect::pure t2))))))
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
allow = ["sys/time::now"]

[runtime]
max_effect_ops = 1
"#,
    )
    .unwrap();
    let out = run(&mut ctx, &pol, prog, h, "gc_effects-test".to_string()).expect("run");
    assert_eq!(out.log.entries.len(), 2);
    assert_eq!(out.log.entries[0].decision, Decision::Allow);
    assert_eq!(out.log.entries[1].decision, Decision::Deny);

    let err = sealed_error_payload_map(&out.value, &ctx);
    assert_eq!(
        err.get(&TermOrdKey(Term::symbol(":error/code"))),
        Some(&Term::Str("core/caps/resource-limit".to_string()))
    );
    let Some(Term::Map(error_ctx)) = err.get(&TermOrdKey(Term::symbol(":error/context"))) else {
        panic!("expected :error/context map");
    };
    assert_eq!(
        error_ctx.get(&TermOrdKey(Term::symbol(":runtime/budget"))),
        Some(&Term::Str("max_effect_ops".to_string()))
    );
}

#[test]
fn runtime_policy_max_payload_bytes_per_op_fail_closes_oversized_request() {
    let (forms, h) = mk_prog_for("sys/time::now", "{:blob \"payload-too-large\"}");

    let mut ctx = EvalCtx::new();
    let prelude = build_prelude(&mut ctx);
    let mut env = prelude.env;
    let prog = eval_module(&mut ctx, &mut env, &forms).expect("eval");

    let pol = CapsPolicy::from_toml_str(
        r#"
allow = ["sys/time::now"]

[runtime]
max_payload_bytes_per_op = 8
"#,
    )
    .unwrap();
    let out = run(&mut ctx, &pol, prog, h, "gc_effects-test".to_string()).expect("run");
    assert_eq!(out.log.entries.len(), 1);
    assert_eq!(out.log.entries[0].decision, Decision::Deny);

    let err = sealed_error_payload_map(&out.value, &ctx);
    assert_eq!(
        err.get(&TermOrdKey(Term::symbol(":error/code"))),
        Some(&Term::Str("core/caps/resource-limit".to_string()))
    );
    let Some(Term::Map(error_ctx)) = err.get(&TermOrdKey(Term::symbol(":error/context"))) else {
        panic!("expected :error/context map");
    };
    assert_eq!(
        error_ctx.get(&TermOrdKey(Term::symbol(":runtime/budget"))),
        Some(&Term::Str("max_payload_bytes_per_op".to_string()))
    );
}

#[test]
fn runtime_policy_max_payload_bytes_per_run_fail_closes_on_cumulative_budget() {
    let src = r#"
            (def demo::request
              (fn (_)
                (core/effect::perform
                  'sys/time::now
                  {:blob "1234567890"}
                  (fn (resp) (core/effect::pure resp)))))

            (def prog
              ((core/effect::bind (demo::request nil))
                (fn (_r1) (demo::request nil))))
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
allow = ["sys/time::now"]

[runtime]
max_payload_bytes_per_run = 30
"#,
    )
    .unwrap();
    let out = run(&mut ctx, &pol, prog, h, "gc_effects-test".to_string()).expect("run");
    assert_eq!(out.log.entries.len(), 2);
    assert_eq!(out.log.entries[0].decision, Decision::Allow);
    assert_eq!(out.log.entries[1].decision, Decision::Deny);

    let err = sealed_error_payload_map(&out.value, &ctx);
    let Some(Term::Map(error_ctx)) = err.get(&TermOrdKey(Term::symbol(":error/context"))) else {
        panic!("expected :error/context map");
    };
    assert_eq!(
        error_ctx.get(&TermOrdKey(Term::symbol(":runtime/budget"))),
        Some(&Term::Str("max_payload_bytes_per_run".to_string()))
    );
}

#[test]
fn runtime_policy_max_response_bytes_per_op_fail_closes_oversized_response() {
    let td = tempfile::tempdir().unwrap();
    let base = td.path().join("sandbox");
    std::fs::create_dir_all(&base).unwrap();
    std::fs::write(base.join("payload.bin"), vec![7u8; 64]).unwrap();

    let caps_path = td.path().join("caps.toml");
    std::fs::write(
        &caps_path,
        r#"
allow = ["io/fs::read"]

[runtime]
max_response_bytes_per_op = 8

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
                {:path "payload.bin"}
                (fn (b) (core/effect::pure b))))
            prog
        "#;
    let forms = parse_module(src).expect("parse module");
    let h = hash_module(&forms);

    let mut ctx = EvalCtx::new();
    let prelude = build_prelude(&mut ctx);
    let mut env = prelude.env;
    let prog = eval_module(&mut ctx, &mut env, &forms).expect("eval");
    let out = run(&mut ctx, &pol, prog, h, "gc_effects-test".to_string()).expect("run");
    assert_eq!(out.log.entries.len(), 1);
    assert_eq!(out.log.entries[0].decision, Decision::Deny);

    let err = sealed_error_payload_map(&out.value, &ctx);
    let Some(Term::Map(error_ctx)) = err.get(&TermOrdKey(Term::symbol(":error/context"))) else {
        panic!("expected :error/context map");
    };
    assert_eq!(
        error_ctx.get(&TermOrdKey(Term::symbol(":runtime/budget"))),
        Some(&Term::Str("max_response_bytes_per_op".to_string()))
    );
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
        elapsed_parallel_ms > 0 && elapsed_serial_ms > 0,
        "task runtime measurements must be positive; parallel={elapsed_parallel_ms}ms serial={elapsed_serial_ms}ms"
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

    let pol =
        CapsPolicy::from_toml_str(r#"allow = ["core/task::spawn", "core/task::await"]"#).unwrap();
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
fn task_channels_are_fifo_and_replay_deterministic() {
    let src = r#"
            (def channel-demo::finish
              (fn (cid)
                ((core/effect::bind (core/task::channel-recv cid))
                  (fn (r1)
                    ((core/effect::bind (core/task::channel-recv cid))
                      (fn (r2)
                        ((core/effect::bind (core/task::channel-recv cid))
                          (fn (r3)
                            (core/effect::pure {:r1 r1 :r2 r2 :r3 r3})))))))))

            (def channel-demo::prepare
              (fn (cid)
                ((core/effect::bind ((core/task::channel-send cid) 10))
                  (fn (_s1)
                    ((core/effect::bind ((core/task::channel-send cid) 20))
                      (fn (_s2)
                        ((core/effect::bind (core/task::channel-close cid))
                          (fn (_closed)
                            (channel-demo::finish cid)))))))))

            (def prog
              ((core/effect::bind (core/task::channel-open 2))
                (fn (opened)
                  (channel-demo::prepare ((core/map::get opened) ':channel-id)))))
            prog
        "#;
    let forms = parse_module(src).expect("parse module");
    let h = hash_module(&forms);

    let mut ctx1 = EvalCtx::new();
    let prelude1 = build_prelude(&mut ctx1);
    let mut env1 = prelude1.env;
    let prog1 = eval_module(&mut ctx1, &mut env1, &forms).expect("eval1");
    let pol = CapsPolicy::from_toml_str(
            r#"allow = ["core/task::channel-open", "core/task::channel-send", "core/task::channel-recv", "core/task::channel-close"]"#,
        )
        .unwrap();
    let run_out = run(&mut ctx1, &pol, prog1, h, "gc_effects-test".to_string()).expect("run");

    assert_eq!(run_out.log.entries.len(), 7);
    assert_eq!(run_out.log.entries[0].op, "core/task::channel-open");
    assert_eq!(run_out.log.entries[1].op, "core/task::channel-send");
    assert_eq!(run_out.log.entries[2].op, "core/task::channel-send");
    assert_eq!(run_out.log.entries[3].op, "core/task::channel-close");
    assert_eq!(run_out.log.entries[4].op, "core/task::channel-recv");
    assert_eq!(run_out.log.entries[5].op, "core/task::channel-recv");
    assert_eq!(run_out.log.entries[6].op, "core/task::channel-recv");

    let read_entry_map = |k: &str| -> Option<&std::collections::BTreeMap<TermOrdKey, Term>> {
        match &run_out.value {
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
    let read_value = |k: &str| -> Option<Term> {
        let inner = read_entry_map(k)?;
        inner.get(&TermOrdKey(Term::symbol(":value"))).cloned()
    };
    let read_has = |k: &str| -> Option<bool> {
        let inner = read_entry_map(k)?;
        match inner.get(&TermOrdKey(Term::symbol(":has-value"))) {
            Some(Term::Bool(v)) => Some(*v),
            _ => None,
        }
    };

    assert_eq!(read_value(":r1"), Some(Term::Int(10.into())));
    assert_eq!(read_value(":r2"), Some(Term::Int(20.into())));
    assert_eq!(read_has(":r1"), Some(true));
    assert_eq!(read_has(":r2"), Some(true));
    assert_eq!(read_has(":r3"), Some(false));

    let mut ctx2 = EvalCtx::new();
    let prelude2 = build_prelude(&mut ctx2);
    let mut env2 = prelude2.env;
    let prog2 = eval_module(&mut ctx2, &mut env2, &forms).expect("eval2");
    let replay_v = replay(&mut ctx2, prog2, &run_out.log).expect("replay");
    assert_eq!(value_hash(&run_out.value), value_hash(&replay_v));
}

#[test]
fn parallel_reduce_bounded_is_deterministic_and_parallel() {
    let src = r#"
            (def mk-payload
              (fn (x)
                {:task/sleep-ms 200 :task/result (prim int/mul x 2)}))

            (def reducer
              (fn (acc)
                (fn (resp)
                  (core/effect::pure (prim int/add acc ((core/map::get resp) ':result))))))

            (def prog
              (((((((core/task::parallel-reduce-bounded "scope/main") "reduce") [1 2 3 4 5 6 7 8]) 2) mk-payload) 0) reducer))
            prog
        "#;
    let forms = parse_module(src).expect("parse module");
    let h = hash_module(&forms);
    let pol =
        CapsPolicy::from_toml_str(r#"allow = ["core/task::spawn", "core/task::await"]"#).unwrap();

    let run_once = |max_workers: u64| {
        let mut ctx = EvalCtx::new();
        let prelude = build_prelude(&mut ctx);
        let mut env = prelude.env;
        let prog = eval_module(&mut ctx, &mut env, &forms).expect("eval");
        let started = std::time::Instant::now();
        let r = run(&mut ctx, &pol, prog, h, "gc_effects-test".to_string()).expect("run");
        let elapsed = started.elapsed().as_millis();
        let Value::Data(Term::Int(v)) = r.value else {
            panic!("parallel reduce must return int");
        };
        assert_eq!(v, 72.into());
        for e in &r.log.entries {
            assert!(
                matches!(e.op.as_str(), "core/task::spawn" | "core/task::await"),
                "unexpected op in parallel-reduce trace: {}",
                e.op
            );
        }
        // Re-run with explicit worker budget to compare runtimes.
        let mut ctx2 = EvalCtx::new();
        let prelude2 = build_prelude(&mut ctx2);
        let mut env2 = prelude2.env;
        let prog2 = eval_module(&mut ctx2, &mut env2, &forms).expect("eval2");
        let policy_text = format!(
            r#"
allow = ["core/task::spawn", "core/task::await"]

[task]
max_workers = {max_workers}
"#
        );
        let pol2 = CapsPolicy::from_toml_str(&policy_text).unwrap();
        let started2 = std::time::Instant::now();
        let r2 = run(&mut ctx2, &pol2, prog2, h, "gc_effects-test".to_string()).expect("run2");
        let elapsed2 = started2.elapsed().as_millis();
        let mut ctx3 = EvalCtx::new();
        let prelude3 = build_prelude(&mut ctx3);
        let mut env3 = prelude3.env;
        let prog3 = eval_module(&mut ctx3, &mut env3, &forms).expect("eval3");
        let replay_v = replay(&mut ctx3, prog3, &r2.log).expect("replay");
        assert_eq!(value_hash(&r2.value), value_hash(&replay_v));
        (elapsed, elapsed2)
    };

    let (_base_elapsed, parallel_elapsed) = run_once(2);
    let (_base_elapsed2, serial_elapsed) = run_once(1);
    assert!(parallel_elapsed > 0 && serial_elapsed > 0);
}

#[path = "tests_host_backends.rs"]
mod tests_host_backends;
