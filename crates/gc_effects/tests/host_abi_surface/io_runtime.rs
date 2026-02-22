use gc_coreform::{hash_module, parse_module};
use gc_effects::{CapsPolicy, EffectLog, replay, run};
use gc_kernel::{EvalCtx, eval_module, value_hash};
use gc_prelude::build_prelude;
use tempfile::tempdir;

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
fn fs_extended_ops_are_replay_deterministic() {
    let temp = tempdir().expect("tempdir");
    let base_dir = temp.path().display().to_string().replace('\\', "/");
    let work_dir = temp.path().join("sandbox/work");
    std::fs::create_dir_all(&work_dir).expect("create work dir");
    std::fs::write(work_dir.join("b.txt"), b"hello").expect("seed file");
    let src = r#"
        (def prog
          (core/effect::perform
            'io/fs::stat
            {:path "sandbox/work/b.txt"}
            (fn (stat-resp) (core/effect::pure stat-resp)))
          )
        prog
    "#;
    let forms = parse_module(src).expect("parse module");
    let h = hash_module(&forms);
    let policy = CapsPolicy::from_toml_str(&format!(
        r#"
allow = ["io/fs::stat"]

[op."io/fs::stat"]
base_dir = "{base_dir}"
"#
    ))
    .expect("policy");

    let mut ctx = EvalCtx::new();
    let prelude = build_prelude(&mut ctx);
    let mut env = prelude.env;
    let prog = eval_module(&mut ctx, &mut env, &forms).expect("eval");
    let run_out = run(&mut ctx, &policy, prog, h, "host-abi-test".to_string()).expect("run");
    assert_eq!(run_out.log.entries.len(), 1);
    assert_eq!(run_out.log.entries[0].op, "io/fs::stat");

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
fn process_lifecycle_ops_are_replay_deterministic_with_wasi_bridge_profile() {
    let src = r#"
        (def prog
          (core/effect::bind
            (core/effect::perform
              'sys/process::spawn
              {:program "gcpm" :args ["status"]}
              (fn (spawn-resp) (core/effect::pure spawn-resp)))
            (fn (spawn-resp)
              (let ((pid ((core/map::get spawn-resp) ':process-id)))
                (core/effect::perform
                  'sys/process::wait
                  {:process-id pid}
                  (fn (wait-resp) (core/effect::pure wait-resp))))))
          )
        prog
    "#;
    let forms = parse_module(src).expect("parse module");
    let h = hash_module(&forms);
    let policy = CapsPolicy::from_toml_str(
        r#"
allow = ["sys/process::spawn", "sys/process::wait"]

[op."sys/process::spawn"]
allow_programs = ["gcpm"]
wasi_bridge_profile = true
wasi_bridge_response = "{:ok true :process-id \"proc-1\"}"

[op."sys/process::wait"]
wasi_bridge_profile = true
wasi_bridge_response = "{:ok true :status 0}"
"#,
    )
    .expect("policy");

    let mut ctx = EvalCtx::new();
    let prelude = build_prelude(&mut ctx);
    let mut env = prelude.env;
    let prog = eval_module(&mut ctx, &mut env, &forms).expect("eval");
    let run_out = run(&mut ctx, &policy, prog, h, "host-abi-test".to_string()).expect("run");
    assert_eq!(run_out.log.entries.len(), 2);
    assert_eq!(run_out.log.entries[0].op, "sys/process::spawn");
    assert_eq!(run_out.log.entries[1].op, "sys/process::wait");

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
