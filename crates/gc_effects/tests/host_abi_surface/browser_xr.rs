use gc_coreform::{Term, TermOrdKey, hash_module, parse_module};
use gc_effects::{CapsPolicy, EffectLog, replay, run};
use gc_kernel::{EvalCtx, Value, eval_module, value_hash};
use gc_prelude::build_prelude;

#[test]
fn browser_host_ops_are_replay_deterministic_without_bridge() {
    let src = r#"
        (def prog
          (core/effect::bind
            (core/effect::perform
              'browser/window::open
              {:opts {:height 720 :title "agent-browser" :width 1280}}
              (fn (open-resp) (core/effect::pure open-resp)))
            (fn (open-resp)
              (let ((wid ((core/map::get open-resp) ':window-id)))
                (core/effect::bind
                  (core/effect::perform
                    'browser/storage::set
                    {:key "scene" :value {:name "intro"}}
                    (fn (set-resp) (core/effect::pure set-resp)))
                  (fn (_)
                    (core/effect::bind
                      (core/effect::perform
                        'browser/input::poll
                        {:max-events 4 :window-id wid}
                        (fn (poll-resp) (core/effect::pure poll-resp)))
                      (fn (poll-resp)
                        (core/effect::bind
                          (core/effect::perform
                            'browser/storage::get
                            {:key "scene"}
                            (fn (get-resp) (core/effect::pure get-resp)))
                          (fn (get-resp)
                            (core/effect::pure
                              {:get get-resp :open open-resp :poll poll-resp})))))))))))
        prog
    "#;
    let forms = parse_module(src).expect("parse module");
    let h = hash_module(&forms);
    let policy = CapsPolicy::from_toml_str(
        r#"
allow = [
  "browser/window::open",
  "browser/storage::set",
  "browser/input::poll",
  "browser/storage::get"
]
"#,
    )
    .expect("policy");

    let mut ctx = EvalCtx::new();
    let prelude = build_prelude(&mut ctx);
    let mut env = prelude.env;
    let prog = eval_module(&mut ctx, &mut env, &forms).expect("eval");
    let run_out = run(&mut ctx, &policy, prog, h, "host-abi-test".to_string()).expect("run");
    assert_eq!(run_out.log.entries.len(), 4);
    assert_eq!(run_out.log.entries[0].op, "browser/window::open");
    assert_eq!(run_out.log.entries[1].op, "browser/storage::set");
    assert_eq!(run_out.log.entries[2].op, "browser/input::poll");
    assert_eq!(run_out.log.entries[3].op, "browser/storage::get");

    let Value::Map(top) = &run_out.value else {
        panic!("expected run output map");
    };
    let Some(Value::Data(Term::Map(open_map))) = top.get(&TermOrdKey(Term::symbol(":open"))) else {
        panic!("expected :open map");
    };
    assert_eq!(
        open_map.get(&TermOrdKey(Term::symbol(":backend"))),
        Some(&Term::Str("browser-first-party-runtime".to_string()))
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

    assert_eq!(run_hash, replay_hash, "run/replay hash mismatch");
}

#[test]
fn browser_host_ops_are_replay_deterministic_with_wasi_bridge_profile() {
    let src = r#"
        (def prog
          ((core/effect::bind
             (core/effect::perform
               'browser/window::open
               {:opts {:height 720 :title "bridge-browser" :width 1280}}
               (fn (x) (core/effect::pure x))))
            (fn (_open)
              ((core/effect::bind
                 (core/effect::perform
                   'browser/storage::set
                   {:key "scene" :value {:name "bridge"}}
                   (fn (x) (core/effect::pure x))))
               (fn (_set)
                 (core/effect::perform
                   'browser/storage::get
                   {:key "scene"}
                   (fn (x) (core/effect::pure x))))))))
        prog
    "#;
    let forms = parse_module(src).expect("parse module");
    let h = hash_module(&forms);
    let policy = CapsPolicy::from_toml_str(
        r#"
allow = [
  "browser/window::open",
  "browser/storage::set",
  "browser/storage::get"
]

[op."browser/window::open"]
wasi_bridge_profile = true
wasi_bridge_response = "{:ok true :backend \"bridge-browser\" :window-id \"w-1\"}"

[op."browser/storage::set"]
wasi_bridge_profile = true
wasi_bridge_response = "{:ok true :stored true}"

[op."browser/storage::get"]
wasi_bridge_profile = true
wasi_bridge_response = "{:ok true :found true :value {:name \"bridge\"}}"
"#,
    )
    .expect("policy");

    let mut ctx = EvalCtx::new();
    let prelude = build_prelude(&mut ctx);
    let mut env = prelude.env;
    let prog = eval_module(&mut ctx, &mut env, &forms).expect("eval");
    let run_out = run(&mut ctx, &policy, prog, h, "host-abi-test".to_string()).expect("run");
    assert_eq!(run_out.log.entries.len(), 3);
    assert_eq!(run_out.log.entries[0].op, "browser/window::open");
    assert_eq!(run_out.log.entries[1].op, "browser/storage::set");
    assert_eq!(run_out.log.entries[2].op, "browser/storage::get");

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
fn xr_host_ops_are_replay_deterministic_without_bridge() {
    let src = r#"
        (def prog
          (core/effect::bind
            (core/effect::perform
              'gfx/xr::session-open
              {:opts {:app "agent-xr" :mode "immersive-vr" :reference-space "local-floor"}}
              (fn (open-resp) (core/effect::pure open-resp)))
            (fn (open-resp)
              (let ((sid ((core/map::get open-resp) ':session-id)))
                (core/effect::bind
                  (core/effect::perform
                    'gfx/xr::frame-poll
                    {:session-id sid}
                    (fn (frame-resp) (core/effect::pure frame-resp)))
                  (fn (frame-resp)
                    (core/effect::bind
                      (core/effect::perform
                        'gfx/xr::input-poll
                        {:max-inputs 2 :session-id sid}
                        (fn (input-resp) (core/effect::pure input-resp)))
                      (fn (input-resp)
                        (core/effect::bind
                          (core/effect::perform
                            'gfx/xr::haptics-pulse
                            {:session-id sid :input-id "right-controller" :amplitude 800 :duration-ms 24}
                            (fn (haptics-resp) (core/effect::pure haptics-resp)))
                          (fn (haptics-resp)
                            (core/effect::bind
                              (core/effect::perform
                                'gfx/xr::submit-frame
                                {:session-id sid :frame ((core/map::get frame-resp) ':frame)}
                                (fn (submit-resp) (core/effect::pure submit-resp)))
                              (fn (submit-resp)
                                (core/effect::bind
                                  (core/effect::perform
                                    'gfx/xr::session-close
                                    {:session-id sid}
                                    (fn (close-resp) (core/effect::pure close-resp)))
                                  (fn (close-resp)
                                    (core/effect::pure
                                      {:close close-resp
                                       :frame frame-resp
                                       :haptics haptics-resp
                                       :input input-resp
                                       :open open-resp
                                       :submit submit-resp})))))))))))))))
        prog
    "#;
    let forms = parse_module(src).expect("parse module");
    let h = hash_module(&forms);
    let policy = CapsPolicy::from_toml_str(
        r#"
allow = [
  "gfx/xr::session-open",
  "gfx/xr::frame-poll",
  "gfx/xr::input-poll",
  "gfx/xr::haptics-pulse",
  "gfx/xr::submit-frame",
  "gfx/xr::session-close"
]

[op."gfx/xr::haptics-pulse"]
allow_haptics_inputs = ["left-controller", "right-controller"]
max_haptics_amplitude = 1000
max_haptics_duration_ms = 64
"#,
    )
    .expect("policy");

    let mut ctx = EvalCtx::new();
    let prelude = build_prelude(&mut ctx);
    let mut env = prelude.env;
    let prog = eval_module(&mut ctx, &mut env, &forms).expect("eval");
    let run_out = run(&mut ctx, &policy, prog, h, "host-abi-test".to_string()).expect("run");
    assert_eq!(run_out.log.entries.len(), 6);
    assert_eq!(run_out.log.entries[0].op, "gfx/xr::session-open");
    assert_eq!(run_out.log.entries[1].op, "gfx/xr::frame-poll");
    assert_eq!(run_out.log.entries[2].op, "gfx/xr::input-poll");
    assert_eq!(run_out.log.entries[3].op, "gfx/xr::haptics-pulse");
    assert_eq!(run_out.log.entries[4].op, "gfx/xr::submit-frame");
    assert_eq!(run_out.log.entries[5].op, "gfx/xr::session-close");

    let Value::Map(top) = &run_out.value else {
        panic!("expected run output map");
    };
    let Some(Value::Data(Term::Map(open_map))) = top.get(&TermOrdKey(Term::symbol(":open"))) else {
        panic!("expected :open map");
    };
    assert_eq!(
        open_map.get(&TermOrdKey(Term::symbol(":backend"))),
        Some(&Term::Str("xr-first-party-runtime".to_string()))
    );
    let Some(Value::Data(Term::Map(input_map))) = top.get(&TermOrdKey(Term::symbol(":input")))
    else {
        panic!("expected :input map");
    };
    let Some(Term::Vector(inputs)) = input_map.get(&TermOrdKey(Term::symbol(":inputs"))) else {
        panic!("expected :input :inputs vector");
    };
    assert_eq!(inputs.len(), 2);
    let Some(Value::Data(Term::Map(haptics_map))) = top.get(&TermOrdKey(Term::symbol(":haptics")))
    else {
        panic!("expected :haptics map");
    };
    assert_eq!(
        haptics_map.get(&TermOrdKey(Term::symbol(":pulse-id"))),
        Some(&Term::Str("xr-haptic-1".to_string()))
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

    assert_eq!(run_hash, replay_hash, "run/replay hash mismatch");
}

#[test]
fn xr_host_ops_are_replay_deterministic_with_wasi_bridge_profile() {
    let src = r#"
        (def prog
          (core/effect::bind
            (core/effect::perform
              'gfx/xr::session-open
              {:opts {:app "bridge-xr" :mode "immersive-vr" :reference-space "local-floor"}}
              (fn (open-resp) (core/effect::pure open-resp)))
            (fn (open-resp)
              (let ((sid ((core/map::get open-resp) ':session-id)))
                (core/effect::bind
                  (core/effect::perform
                    'gfx/xr::frame-poll
                    {:session-id sid}
                    (fn (frame-resp) (core/effect::pure frame-resp)))
                  (fn (frame-resp)
                    (core/effect::bind
                      (core/effect::perform
                        'gfx/xr::haptics-pulse
                        {:session-id sid :input-id "right-controller" :amplitude 700 :duration-ms 20}
                        (fn (haptics-resp) (core/effect::pure haptics-resp)))
                      (fn (haptics-resp)
                        (core/effect::bind
                          (core/effect::perform
                            'gfx/xr::session-close
                            {:session-id sid}
                            (fn (close-resp) (core/effect::pure close-resp)))
                          (fn (close-resp)
                            (core/effect::pure
                              {:close close-resp :frame frame-resp :haptics haptics-resp :open open-resp})))))))))))
        prog
    "#;
    let forms = parse_module(src).expect("parse module");
    let h = hash_module(&forms);
    let policy = CapsPolicy::from_toml_str(
        r#"
allow = [
  "gfx/xr::session-open",
  "gfx/xr::frame-poll",
  "gfx/xr::haptics-pulse",
  "gfx/xr::session-close"
]

[op."gfx/xr::session-open"]
wasi_bridge_profile = true
wasi_bridge_response = "{:ok true :backend \"bridge-xr\" :adapter \"bridge\" :session-id \"xr-bridge-1\"}"

[op."gfx/xr::frame-poll"]
wasi_bridge_profile = true
wasi_bridge_response = "{:ok true :backend \"bridge-xr\" :adapter \"bridge\" :session-id \"xr-bridge-1\" :frame {:frame-index 3 :predicted-display-time-ms 33 :views []}}"

[op."gfx/xr::haptics-pulse"]
wasi_bridge_profile = true
allow_haptics_inputs = ["right-controller"]
max_haptics_amplitude = 900
max_haptics_duration_ms = 40
wasi_bridge_response = "{:ok true :backend \"bridge-xr\" :adapter \"bridge\" :session-id \"xr-bridge-1\" :input-id \"right-controller\" :pulse-id \"bridge-pulse-1\" :accepted true}"

[op."gfx/xr::session-close"]
wasi_bridge_profile = true
wasi_bridge_response = "{:ok true :backend \"bridge-xr\" :adapter \"bridge\" :session-id \"xr-bridge-1\" :closed true}"
"#,
    )
    .expect("policy");

    let mut ctx = EvalCtx::new();
    let prelude = build_prelude(&mut ctx);
    let mut env = prelude.env;
    let prog = eval_module(&mut ctx, &mut env, &forms).expect("eval");
    let run_out = run(&mut ctx, &policy, prog, h, "host-abi-test".to_string()).expect("run");
    assert_eq!(run_out.log.entries.len(), 4);
    assert_eq!(run_out.log.entries[0].op, "gfx/xr::session-open");
    assert_eq!(run_out.log.entries[1].op, "gfx/xr::frame-poll");
    assert_eq!(run_out.log.entries[2].op, "gfx/xr::haptics-pulse");
    assert_eq!(run_out.log.entries[3].op, "gfx/xr::session-close");

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
fn xr_webxr_device_backend_ops_are_replay_deterministic_with_wasi_bridge_profile() {
    let src = r#"
        (def prog
          (core/effect::bind
            (core/effect::perform
              'gfx/xr::session-open
              {:opts {:app "webxr-device" :mode "immersive-vr" :reference-space "local-floor"}}
              (fn (open-resp) (core/effect::pure open-resp)))
            (fn (open-resp)
              (let ((sid ((core/map::get open-resp) ':session-id)))
                (core/effect::bind
                  (core/effect::perform
                    'gfx/xr::frame-poll
                    {:session-id sid}
                    (fn (frame-resp) (core/effect::pure frame-resp)))
                  (fn (frame-resp)
                    (core/effect::bind
                      (core/effect::perform
                        'gfx/xr::input-poll
                        {:session-id sid :max-inputs 2}
                        (fn (input-resp) (core/effect::pure input-resp)))
                      (fn (input-resp)
                        (core/effect::bind
                          (core/effect::perform
                            'gfx/xr::haptics-pulse
                            {:session-id sid :input-id "right-controller" :amplitude 640 :duration-ms 16}
                            (fn (haptics-resp) (core/effect::pure haptics-resp)))
                          (fn (haptics-resp)
                            (core/effect::bind
                              (core/effect::perform
                                'gfx/xr::submit-frame
                                {:session-id sid :frame ((core/map::get frame-resp) ':frame)}
                                (fn (submit-resp) (core/effect::pure submit-resp)))
                              (fn (submit-resp)
                                (core/effect::bind
                                  (core/effect::perform
                                    'gfx/xr::session-close
                                    {:session-id sid}
                                    (fn (close-resp) (core/effect::pure close-resp)))
                                  (fn (close-resp)
                                    (core/effect::pure
                                      {:open open-resp
                                       :frame frame-resp
                                       :input input-resp
                                       :haptics haptics-resp
                                       :submit submit-resp
                                       :close close-resp})))))))))))))))
        prog
    "#;
    let forms = parse_module(src).expect("parse module");
    let h = hash_module(&forms);
    let policy = CapsPolicy::from_toml_str(
        r#"
allow = [
  "gfx/xr::session-open",
  "gfx/xr::frame-poll",
  "gfx/xr::input-poll",
  "gfx/xr::haptics-pulse",
  "gfx/xr::submit-frame",
  "gfx/xr::session-close"
]

[op."gfx/xr::session-open"]
xr_backend = "webxr-device"
wasi_bridge_profile = true
wasi_bridge_response = "{:ok true :session-id \"xr-webxr-1\" :mode \"immersive-vr\" :reference-space \"local-floor\"}"

[op."gfx/xr::frame-poll"]
xr_backend = "webxr-device"
wasi_bridge_profile = true
wasi_bridge_response = "{:ok true :session-id \"xr-webxr-1\" :frame {:frame-index 8 :predicted-display-time-ms 88 :views []}}"

[op."gfx/xr::input-poll"]
xr_backend = "webxr-device"
wasi_bridge_profile = true
wasi_bridge_response = "{:ok true :session-id \"xr-webxr-1\" :inputs [{:id \"right-controller\" :kind :controller :select true}]}"

[op."gfx/xr::haptics-pulse"]
xr_backend = "webxr-device"
wasi_bridge_profile = true
wasi_bridge_response = "{:ok true :session-id \"xr-webxr-1\" :input-id \"right-controller\" :pulse-id \"webxr-pulse-1\" :accepted true}"

[op."gfx/xr::submit-frame"]
xr_backend = "webxr-device"
wasi_bridge_profile = true
wasi_bridge_response = "{:ok true :session-id \"xr-webxr-1\" :accepted true :frame-index 8}"

[op."gfx/xr::session-close"]
xr_backend = "webxr-device"
wasi_bridge_profile = true
wasi_bridge_response = "{:ok true :session-id \"xr-webxr-1\" :closed true}"
"#,
    )
    .expect("policy");

    let mut ctx = EvalCtx::new();
    let prelude = build_prelude(&mut ctx);
    let mut env = prelude.env;
    let prog = eval_module(&mut ctx, &mut env, &forms).expect("eval");
    let run_out = run(&mut ctx, &policy, prog, h, "host-abi-test".to_string()).expect("run");
    assert_eq!(run_out.log.entries.len(), 6);
    assert_eq!(run_out.log.entries[0].op, "gfx/xr::session-open");
    assert_eq!(run_out.log.entries[1].op, "gfx/xr::frame-poll");
    assert_eq!(run_out.log.entries[2].op, "gfx/xr::input-poll");
    assert_eq!(run_out.log.entries[3].op, "gfx/xr::haptics-pulse");
    assert_eq!(run_out.log.entries[4].op, "gfx/xr::submit-frame");
    assert_eq!(run_out.log.entries[5].op, "gfx/xr::session-close");

    let Value::Map(top) = &run_out.value else {
        panic!("expected run output map");
    };
    let Some(Value::Data(Term::Map(open_map))) = top.get(&TermOrdKey(Term::symbol(":open"))) else {
        panic!("expected :open map");
    };
    assert_eq!(
        open_map.get(&TermOrdKey(Term::symbol(":backend"))),
        Some(&Term::Str("xr-webxr-device-runtime".to_string()))
    );
    let Some(Term::Map(open_env)) = open_map.get(&TermOrdKey(Term::symbol(":replay-envelope")))
    else {
        panic!("expected :open :replay-envelope map");
    };
    assert_eq!(
        open_env.get(&TermOrdKey(Term::symbol(":capture-seq"))),
        Some(&Term::Int(1.into()))
    );
    assert_eq!(
        open_env.get(&TermOrdKey(Term::symbol(":source"))),
        Some(&Term::symbol(":webxr-device"))
    );
    let Some(Value::Data(Term::Map(close_map))) = top.get(&TermOrdKey(Term::symbol(":close")))
    else {
        panic!("expected :close map");
    };
    let Some(Term::Map(close_env)) = close_map.get(&TermOrdKey(Term::symbol(":replay-envelope")))
    else {
        panic!("expected :close :replay-envelope map");
    };
    assert_eq!(
        close_env.get(&TermOrdKey(Term::symbol(":capture-seq"))),
        Some(&Term::Int(6.into()))
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

    assert_eq!(run_hash, replay_hash, "run/replay hash mismatch");
}

#[test]
fn xr_production_profile_defaults_to_webxr_device_when_bridge_is_present() {
    let src = r#"
        (def prog
          (core/effect::perform
            'gfx/xr::session-open
            {:opts {:app "prod-webxr" :mode "immersive-vr" :reference-space "local-floor"}}
            (fn (x) (core/effect::pure x))))
        prog
    "#;
    let forms = parse_module(src).expect("parse module");
    let h = hash_module(&forms);
    let policy = CapsPolicy::from_toml_str(
        r#"
allow = ["gfx/xr::session-open"]

[op."gfx/xr::session-open"]
runtime_profile = "production"
wasi_bridge_profile = true
wasi_bridge_response = "{:ok true :session-id \"xr-prod-1\" :mode \"immersive-vr\" :reference-space \"local-floor\"}"
"#,
    )
    .expect("policy");

    let mut ctx = EvalCtx::new();
    let prelude = build_prelude(&mut ctx);
    let mut env = prelude.env;
    let prog = eval_module(&mut ctx, &mut env, &forms).expect("eval");
    let run_out = run(&mut ctx, &policy, prog, h, "host-abi-test".to_string()).expect("run");
    assert_eq!(run_out.log.entries.len(), 1);
    assert_eq!(run_out.log.entries[0].op, "gfx/xr::session-open");

    let Value::Data(Term::Map(open_map)) = &run_out.value else {
        panic!("expected session-open map");
    };
    assert_eq!(
        open_map.get(&TermOrdKey(Term::symbol(":backend"))),
        Some(&Term::Str("xr-webxr-device-runtime".to_string()))
    );
    assert_eq!(
        open_map.get(&TermOrdKey(Term::symbol(":adapter"))),
        Some(&Term::Str("webxr-device".to_string()))
    );
    let Some(Term::Map(envelope)) = open_map.get(&TermOrdKey(Term::symbol(":replay-envelope")))
    else {
        panic!("expected replay envelope");
    };
    assert_eq!(
        envelope.get(&TermOrdKey(Term::symbol(":source"))),
        Some(&Term::symbol(":webxr-device"))
    );

    let log_term = run_out.log.to_term();
    let replay_log = EffectLog::from_term(&log_term).expect("log decode");
    let mut ctx_rep = EvalCtx::new();
    let prelude_rep = build_prelude(&mut ctx_rep);
    let mut env_rep = prelude_rep.env;
    let prog_rep = eval_module(&mut ctx_rep, &mut env_rep, &forms).expect("eval replay");
    let replay_value = replay(&mut ctx_rep, prog_rep, &replay_log).expect("replay");
    assert_eq!(value_hash(&run_out.value), value_hash(&replay_value));
}

#[test]
fn xr_production_profile_without_bridge_is_policy_disabled() {
    let (forms, h) = {
        let src = r#"
            (def prog
              (core/effect::perform
                'gfx/xr::session-open
                {:opts {:app "prod-webxr" :mode "immersive-vr" :reference-space "local-floor"}}
                (fn (x) (core/effect::pure x))))
            prog
        "#;
        let forms = parse_module(src).expect("parse module");
        let h = hash_module(&forms);
        (forms, h)
    };

    let policy = CapsPolicy::from_toml_str(
        r#"
allow = ["gfx/xr::session-open"]

[op."gfx/xr::session-open"]
runtime_profile = "production"
"#,
    )
    .expect("policy");

    let mut ctx = EvalCtx::new();
    let prelude = build_prelude(&mut ctx);
    let mut env = prelude.env;
    let prog = eval_module(&mut ctx, &mut env, &forms).expect("eval");
    let run_out = run(&mut ctx, &policy, prog, h, "host-abi-test".to_string()).expect("run");

    let Value::Data(Term::Map(err)) = &run_out.value else {
        panic!("expected policy-disabled map");
    };
    assert_eq!(
        err.get(&TermOrdKey(Term::symbol(":ok"))),
        Some(&Term::Bool(false))
    );
    assert_eq!(
        err.get(&TermOrdKey(Term::symbol(":error/code"))),
        Some(&Term::Str("gfx/xr-policy-disabled".to_string()))
    );
    assert_eq!(
        err.get(&TermOrdKey(Term::symbol(":schema"))),
        Some(&Term::symbol(":core/host-policy-disabled.v1"))
    );
    assert_eq!(
        err.get(&TermOrdKey(Term::symbol(":policy-disabled"))),
        Some(&Term::Bool(true))
    );
}

#[test]
fn xr_advanced_ops_are_replay_deterministic_without_bridge() {
    let src = r#"
        (def prog
          (core/effect::bind
            (core/effect::perform
              'gfx/xr::session-open
              {:opts {:app "advanced-xr" :mode "immersive-ar" :reference-space "local-floor"}}
              (fn (open-resp) (core/effect::pure open-resp)))
            (fn (open-resp)
              (let ((sid ((core/map::get open-resp) ':session-id)))
                (core/effect::bind
                  (core/effect::perform
                    'gfx/xr::hands-poll
                    {:session-id sid :max-joints 21}
                    (fn (hands-resp) (core/effect::pure hands-resp)))
                  (fn (hands-resp)
                    (core/effect::bind
                      (core/effect::perform
                        'gfx/xr::hit-test
                        {:session-id sid :ray {:origin [0 1 0] :direction [0 0 -1]} :max-hits 2}
                        (fn (hit-resp) (core/effect::pure hit-resp)))
                      (fn (hit-resp)
                        (core/effect::bind
                          (core/effect::perform
                            'gfx/xr::spatial-mesh-poll
                            {:session-id sid :max-meshes 2 :lod "medium"}
                            (fn (mesh-resp) (core/effect::pure mesh-resp)))
                          (fn (mesh-resp)
                            (core/effect::bind
                              (core/effect::perform
                                'gfx/xr::anchor-create
                                {:session-id sid :space "local-floor" :label "root-anchor" :pose {:position [0 1 -1]}}
                                (fn (anchor-create-resp) (core/effect::pure anchor-create-resp)))
                              (fn (anchor-create-resp)
                                (let ((aid ((core/map::get anchor-create-resp) ':anchor-id)))
                                  (core/effect::bind
                                    (core/effect::perform
                                      'gfx/xr::layer-create
                                      {:session-id sid :type "quad" :layout "stereo" :opacity 1000 :transform {:position [0 1 -2]}}
                                      (fn (layer-create-resp) (core/effect::pure layer-create-resp)))
                                    (fn (layer-create-resp)
                                      (let ((lid ((core/map::get layer-create-resp) ':layer-id)))
                                        (core/effect::bind
                                          (core/effect::perform
                                            'gfx/xr::anchor-destroy
                                            {:session-id sid :anchor-id aid}
                                            (fn (anchor-destroy-resp) (core/effect::pure anchor-destroy-resp)))
                                          (fn (anchor-destroy-resp)
                                            (core/effect::bind
                                              (core/effect::perform
                                                'gfx/xr::layer-destroy
                                                {:session-id sid :layer-id lid}
                                                (fn (layer-destroy-resp) (core/effect::pure layer-destroy-resp)))
                                              (fn (layer-destroy-resp)
                                                (core/effect::bind
                                                  (core/effect::perform
                                                    'gfx/xr::session-close
                                                    {:session-id sid}
                                                    (fn (close-resp) (core/effect::pure close-resp)))
                                                  (fn (close-resp)
                                                    (core/effect::pure
                                                      {
                                                        :open open-resp
                                                        :hands hands-resp
                                                        :hit hit-resp
                                                        :mesh mesh-resp
                                                        :anchor-create anchor-create-resp
                                                        :anchor-destroy anchor-destroy-resp
                                                        :layer-create layer-create-resp
                                                        :layer-destroy layer-destroy-resp
                                                        :close close-resp
                                                      })))))))))))))))))))))))
        prog
    "#;
    let forms = parse_module(src).expect("parse module");
    let h = hash_module(&forms);
    let policy = CapsPolicy::from_toml_str(
        r#"
allow = [
  "gfx/xr::session-open",
  "gfx/xr::hands-poll",
  "gfx/xr::hit-test",
  "gfx/xr::spatial-mesh-poll",
  "gfx/xr::anchor-create",
  "gfx/xr::anchor-update",
  "gfx/xr::anchor-destroy",
  "gfx/xr::layer-create",
  "gfx/xr::layer-update",
  "gfx/xr::layer-destroy",
  "gfx/xr::session-close"
]
"#,
    )
    .expect("policy");

    let mut ctx = EvalCtx::new();
    let prelude = build_prelude(&mut ctx);
    let mut env = prelude.env;
    let prog = eval_module(&mut ctx, &mut env, &forms).expect("eval");
    let run_out = run(&mut ctx, &policy, prog, h, "host-abi-test".to_string()).expect("run");
    assert_eq!(run_out.log.entries.len(), 9);
    assert_eq!(run_out.log.entries[0].op, "gfx/xr::session-open");
    assert_eq!(run_out.log.entries[1].op, "gfx/xr::hands-poll");
    assert_eq!(run_out.log.entries[2].op, "gfx/xr::hit-test");
    assert_eq!(run_out.log.entries[3].op, "gfx/xr::spatial-mesh-poll");
    assert_eq!(run_out.log.entries[4].op, "gfx/xr::anchor-create");
    assert_eq!(run_out.log.entries[5].op, "gfx/xr::layer-create");
    assert_eq!(run_out.log.entries[6].op, "gfx/xr::anchor-destroy");
    assert_eq!(run_out.log.entries[7].op, "gfx/xr::layer-destroy");
    assert_eq!(run_out.log.entries[8].op, "gfx/xr::session-close");

    let Value::Map(top) = &run_out.value else {
        panic!("expected run output map");
    };
    let Some(Value::Data(Term::Map(anchor_create_map))) =
        top.get(&TermOrdKey(Term::symbol(":anchor-create")))
    else {
        panic!("expected :anchor-create map");
    };
    assert!(anchor_create_map.contains_key(&TermOrdKey(Term::symbol(":anchor-id"))));

    let Some(Value::Data(Term::Map(layer_create_map))) =
        top.get(&TermOrdKey(Term::symbol(":layer-create")))
    else {
        panic!("expected :layer-create map");
    };
    assert!(layer_create_map.contains_key(&TermOrdKey(Term::symbol(":layer-id"))));

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
