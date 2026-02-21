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

#[test]
fn net_and_process_ops_are_replay_deterministic_with_wasi_bridge_profile() {
    let src = r#"
        (def prog
          (core/effect::bind
            (core/effect::perform
              'io/net::http-request
              {:method "GET" :url "https://registry.example.com/api/ping"}
              (fn (net-resp) (core/effect::pure net-resp)))
            (fn (net-resp)
              (core/effect::bind
                (core/effect::perform
                  'sys/process::exec
                  {:program "gcpm" :args ["status"]}
                  (fn (proc-resp) (core/effect::pure proc-resp)))
                (fn (proc-resp)
                  (core/effect::pure {:net net-resp :proc proc-resp}))))))
        prog
    "#;
    let forms = parse_module(src).expect("parse module");
    let h = hash_module(&forms);
    let policy = CapsPolicy::from_toml_str(
        r#"
allow = ["io/net::http-request", "sys/process::exec"]

[op."io/net::http-request"]
url_allow = ["https://registry.example.com/api/"]
wasi_network_profile = "preview2"
wasi_bridge_profile = true
wasi_bridge_response = "{:ok true :status 200 :body \"ok\"}"

[op."sys/process::exec"]
allow_programs = ["gcpm"]
wasi_bridge_profile = true
wasi_bridge_response = "{:ok true :status 0 :stdout \"done\"}"
"#,
    )
    .expect("policy");

    let mut ctx = EvalCtx::new();
    let prelude = build_prelude(&mut ctx);
    let mut env = prelude.env;
    let prog = eval_module(&mut ctx, &mut env, &forms).expect("eval");
    let run_out = run(&mut ctx, &policy, prog, h, "host-abi-test".to_string()).expect("run");
    assert_eq!(run_out.log.entries.len(), 2);
    assert_eq!(run_out.log.entries[0].op, "io/net::http-request");
    assert_eq!(run_out.log.entries[1].op, "sys/process::exec");

    let log_term = run_out.log.to_term();
    let replay_log = EffectLog::from_term(&log_term).expect("log decode");
    let run_hash = value_hash(&run_out.value);

    let mut ctx_rep = EvalCtx::new();
    let prelude_rep = build_prelude(&mut ctx_rep);
    let mut env_rep = prelude_rep.env;
    let prog_rep = eval_module(&mut ctx_rep, &mut env_rep, &forms).expect("eval replay");
    let replay_value = replay(&mut ctx_rep, prog_rep, &replay_log).expect("replay");
    let replay_hash = value_hash(&replay_value);

    assert_eq!(run_hash, replay_hash, "run/replay hash mismatch");
}

#[test]
fn net_ws_ops_are_replay_deterministic_with_wasi_bridge_profile() {
    let src = r#"
        (def prog
          (core/effect::bind
            (core/effect::perform
              'io/net::ws-open
              {:url "wss://realtime.example.com/ws/room"}
              (fn (open-resp) (core/effect::pure open-resp)))
            (fn (open-resp)
              (let ((stream-id ((core/map::get open-resp) ':stream-id)))
                (core/effect::bind
                  (core/effect::perform
                    'io/net::ws-send
                    {:stream-id stream-id :data b"hello"}
                    (fn (send-resp) (core/effect::pure send-resp)))
                  (fn (_send-resp)
                    (core/effect::bind
                      (core/effect::perform
                        'io/net::ws-recv
                        {:stream-id stream-id}
                        (fn (recv-resp) (core/effect::pure recv-resp)))
                      (fn (recv-resp)
                        (core/effect::bind
                          (core/effect::perform
                            'io/net::ws-close
                            {:stream-id stream-id}
                            (fn (close-resp) (core/effect::pure close-resp)))
                          (fn (_close-resp)
                            (core/effect::pure recv-resp)))))))))))
        prog
    "#;
    let forms = parse_module(src).expect("parse module");
    let h = hash_module(&forms);
    let policy = CapsPolicy::from_toml_str(
        r#"
allow = ["io/net::ws-open", "io/net::ws-send", "io/net::ws-recv", "io/net::ws-close"]

[op."io/net::ws-open"]
url_allow = ["wss://realtime.example.com/ws/"]
wasi_network_profile = "preview2"
wasi_bridge_profile = true
wasi_bridge_response = "{:ok true :stream-id \"ws-1\"}"

[op."io/net::ws-send"]
wasi_bridge_profile = true
wasi_bridge_response = "{:ok true :sent-bytes 5}"

[op."io/net::ws-recv"]
wasi_bridge_profile = true
wasi_bridge_response = "{:ok true :data b\"hello\" :eof false}"

[op."io/net::ws-close"]
wasi_bridge_profile = true
wasi_bridge_response = "{:ok true :closed true}"
"#,
    )
    .expect("policy");

    let mut ctx = EvalCtx::new();
    let prelude = build_prelude(&mut ctx);
    let mut env = prelude.env;
    let prog = eval_module(&mut ctx, &mut env, &forms).expect("eval");
    let run_out = run(&mut ctx, &policy, prog, h, "host-abi-test".to_string()).expect("run");
    assert_eq!(run_out.log.entries.len(), 4);
    assert_eq!(run_out.log.entries[0].op, "io/net::ws-open");
    assert_eq!(run_out.log.entries[1].op, "io/net::ws-send");
    assert_eq!(run_out.log.entries[2].op, "io/net::ws-recv");
    assert_eq!(run_out.log.entries[3].op, "io/net::ws-close");

    let log_term = run_out.log.to_term();
    let replay_log = EffectLog::from_term(&log_term).expect("log decode");
    let run_hash = value_hash(&run_out.value);

    let mut ctx_rep = EvalCtx::new();
    let prelude_rep = build_prelude(&mut ctx_rep);
    let mut env_rep = prelude_rep.env;
    let prog_rep = eval_module(&mut ctx_rep, &mut env_rep, &forms).expect("eval replay");
    let replay_value = replay(&mut ctx_rep, prog_rep, &replay_log).expect("replay");
    let replay_hash = value_hash(&replay_value);

    assert_eq!(run_hash, replay_hash, "run/replay hash mismatch");
}

#[test]
fn net_raw_ops_are_replay_deterministic_with_wasi_bridge_profile() {
    let src = r#"
        (def prog
          (core/effect::bind
            (core/effect::perform
              'io/net::dns-resolve
              {:name "allowed.example.com"}
              (fn (dns-resp) (core/effect::pure dns-resp)))
            (fn (dns-resp)
              (core/effect::bind
                (core/effect::perform
                  'io/net::tcp-open
                  {:remote "tcp://allowed.example.com:443"}
                  (fn (tcp-resp) (core/effect::pure tcp-resp)))
                (fn (tcp-resp)
                  (core/effect::bind
                    (core/effect::perform
                      'io/net::udp-bind
                      {:local "udp://127.0.0.1:5353"}
                      (fn (udp-resp) (core/effect::pure udp-resp)))
                    (fn (udp-resp)
                      (core/effect::pure {:dns dns-resp :tcp tcp-resp :udp udp-resp}))))))))
        prog
    "#;
    let forms = parse_module(src).expect("parse module");
    let h = hash_module(&forms);
    let policy = CapsPolicy::from_toml_str(
        r#"
allow = ["io/net::dns-resolve", "io/net::tcp-open", "io/net::udp-bind"]

[op."io/net::dns-resolve"]
url_allow = ["dns://allowed.example.com"]
wasi_network_profile = "preview2"
wasi_bridge_profile = true
wasi_bridge_response = "{:ok true :records [{:type \"A\" :value \"127.0.0.1\"}]}"

[op."io/net::tcp-open"]
url_allow = ["tcp://allowed.example.com:443"]
wasi_network_profile = "preview2"
wasi_bridge_profile = true
wasi_bridge_response = "{:ok true :stream-id \"tcp-1\"}"

[op."io/net::udp-bind"]
url_allow = ["udp://127.0.0.1:5353"]
wasi_network_profile = "preview2"
wasi_bridge_profile = true
wasi_bridge_response = "{:ok true :socket-id \"udp-1\"}"
"#,
    )
    .expect("policy");

    let mut ctx = EvalCtx::new();
    let prelude = build_prelude(&mut ctx);
    let mut env = prelude.env;
    let prog = eval_module(&mut ctx, &mut env, &forms).expect("eval");
    let run_out = run(&mut ctx, &policy, prog, h, "host-abi-test".to_string()).expect("run");
    assert_eq!(run_out.log.entries.len(), 3);
    assert_eq!(run_out.log.entries[0].op, "io/net::dns-resolve");
    assert_eq!(run_out.log.entries[1].op, "io/net::tcp-open");
    assert_eq!(run_out.log.entries[2].op, "io/net::udp-bind");

    let log_term = run_out.log.to_term();
    let replay_log = EffectLog::from_term(&log_term).expect("log decode");
    let run_hash = value_hash(&run_out.value);

    let mut ctx_rep = EvalCtx::new();
    let prelude_rep = build_prelude(&mut ctx_rep);
    let mut env_rep = prelude_rep.env;
    let prog_rep = eval_module(&mut ctx_rep, &mut env_rep, &forms).expect("eval replay");
    let replay_value = replay(&mut ctx_rep, prog_rep, &replay_log).expect("replay");
    let replay_hash = value_hash(&replay_value);

    assert_eq!(run_hash, replay_hash, "run/replay hash mismatch");
}
