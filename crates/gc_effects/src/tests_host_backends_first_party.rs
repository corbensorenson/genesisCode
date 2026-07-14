use super::*;
use std::collections::BTreeMap;

#[test]
fn gfx_gpu_first_party_backend_runs_without_bridge_profile_and_replays() {
    let pol = CapsPolicy::from_toml_str(
        r#"
allow = [
  "gfx/gpu::create-buffer",
  "gfx/gpu::write-buffer",
  "gfx/gpu::read-buffer",
  "gfx/gpu::create-texture",
  "gfx/gpu::write-texture",
  "gfx/gpu::read-texture",
  "gfx/gpu::submit-frame-graph",
  "gfx/gpu::limits",
  "gfx/gpu::features"
]
"#,
    )
    .expect("parse caps");

    let run_once = |src: &str| -> Value {
        let forms = parse_module(src).expect("parse module");
        let h = hash_module(&forms);

        let mut ctx1 = EvalCtx::new();
        let prelude1 = build_prelude(&mut ctx1);
        let mut env1 = prelude1.env;
        let prog1 = eval_module(&mut ctx1, &mut env1, &forms).expect("eval1");
        let run_out = run(&mut ctx1, &pol, prog1, h, "gc_effects-test".to_string()).expect("run");

        let mut ctx2 = EvalCtx::new();
        let prelude2 = build_prelude(&mut ctx2);
        let mut env2 = prelude2.env;
        let prog2 = eval_module(&mut ctx2, &mut env2, &forms).expect("eval2");
        let replay_v = replay(&mut ctx2, prog2, &run_out.log).expect("replay");
        assert_eq!(value_hash(&run_out.value), value_hash(&replay_v));
        run_out.value
    };

    let has_backend = |v: &Value| match v {
        Value::Map(m) => matches!(
            m.get(&TermOrdKey(Term::symbol(":backend"))).and_then(Value::as_data),
            Some(Term::Str(s)) if s == "first-party-runtime"
        ),
        Value::Data(t) => match t.as_ref() {
            Term::Map(m) => matches!(
                m.get(&TermOrdKey(Term::symbol(":backend"))),
                Some(Term::Str(s)) if s == "first-party-runtime"
            ),
            _ => false,
        },
        _ => false,
    };

    let buffer_src = r#"
        (def prog
          ((core/effect::bind (core/effect::perform
                                'gfx/gpu::create-buffer
                                {:desc {:size 8}}
                                (fn (x) (core/effect::pure x))))
            (fn (create-buffer)
              (let ((buffer-id ((core/map::get create-buffer) ':id)))
                ((core/effect::bind (core/effect::perform
                                      'gfx/gpu::write-buffer
                                      {:data b"\x01\x02\x03" :id buffer-id :offset 2}
                                      (fn (x) (core/effect::pure x))))
                  (fn (write-buffer)
                    ((core/effect::bind (core/effect::perform
                                          'gfx/gpu::read-buffer
                                          {:id buffer-id :offset 0 :size 8}
                                          (fn (x) (core/effect::pure x))))
                      (fn (read-buffer)
                        (core/effect::pure
                          {:create-buffer create-buffer
                           :read-buffer read-buffer
                           :write-buffer write-buffer})))))))))
        prog
    "#;
    let buffer_v = run_once(buffer_src);
    let Value::Map(buffer_top) = &buffer_v else {
        panic!(
            "expected top-level map from first-party gfx/gpu buffer test, got {}",
            buffer_v.debug_repr()
        );
    };
    for key in [":create-buffer", ":write-buffer", ":read-buffer"] {
        let Some(value) = buffer_top.get(&TermOrdKey(Term::symbol(key))) else {
            panic!("missing {key} in gfx/gpu buffer response map");
        };
        assert!(
            has_backend(value),
            "{key} should report first-party backend"
        );
    }
    let Some(Term::Map(read_buffer)) = buffer_top
        .get(&TermOrdKey(Term::symbol(":read-buffer")))
        .and_then(Value::as_data)
    else {
        panic!("expected :read-buffer map response");
    };
    let Some(Term::Bytes(read_buffer_bytes)) = read_buffer.get(&TermOrdKey(Term::symbol(":data")))
    else {
        panic!("expected :read-buffer :data bytes");
    };
    assert_eq!(read_buffer_bytes.as_ref(), &[0_u8, 0, 1, 2, 3, 0, 0, 0]);

    let texture_src = r#"
        (def prog
          ((core/effect::bind (core/effect::perform
                                'gfx/gpu::create-texture
                                {:desc {:byte-size 6}}
                                (fn (x) (core/effect::pure x))))
            (fn (create-texture)
              (let ((texture-id ((core/map::get create-texture) ':id)))
                ((core/effect::bind (core/effect::perform
                                      'gfx/gpu::write-texture
                                      {:data b"\xAA\xBB\xCC\xDD\xEE\xFF" :id texture-id}
                                      (fn (x) (core/effect::pure x))))
                  (fn (write-texture)
                    ((core/effect::bind (core/effect::perform
                                          'gfx/gpu::read-texture
                                          {:id texture-id :region {:offset 1 :size 3}}
                                          (fn (x) (core/effect::pure x))))
                      (fn (read-texture)
                        (core/effect::pure
                          {:create-texture create-texture
                           :read-texture read-texture
                           :write-texture write-texture})))))))))
        prog
    "#;
    let texture_v = run_once(texture_src);
    let Value::Map(texture_top) = &texture_v else {
        panic!(
            "expected top-level map from first-party gfx/gpu texture test, got {}",
            texture_v.debug_repr()
        );
    };
    for key in [":create-texture", ":write-texture", ":read-texture"] {
        let Some(value) = texture_top.get(&TermOrdKey(Term::symbol(key))) else {
            panic!("missing {key} in gfx/gpu texture response map");
        };
        assert!(
            has_backend(value),
            "{key} should report first-party backend"
        );
    }
    let Some(Term::Map(read_texture)) = texture_top
        .get(&TermOrdKey(Term::symbol(":read-texture")))
        .and_then(Value::as_data)
    else {
        panic!("expected :read-texture map response");
    };
    let Some(Term::Bytes(read_texture_bytes)) =
        read_texture.get(&TermOrdKey(Term::symbol(":data")))
    else {
        panic!("expected :read-texture :data bytes");
    };
    assert_eq!(read_texture_bytes.as_ref(), &[0xBB, 0xCC, 0xDD]);

    let submit_src = r#"
        (def prog
          ((core/effect::bind (core/effect::perform
                                'gfx/gpu::submit-frame-graph
                                {:graph {:render-passes []}}
                                (fn (x) (core/effect::pure x))))
            (fn (submit)
              ((core/effect::bind (core/effect::perform
                                    'gfx/gpu::limits
                                    {}
                                    (fn (x) (core/effect::pure x))))
                (fn (limits)
                  ((core/effect::bind (core/effect::perform
                                        'gfx/gpu::features
                                        {}
                                        (fn (x) (core/effect::pure x))))
                    (fn (features)
                      (core/effect::pure
                        {:features features
                         :limits limits
                         :submit submit}))))))))
        prog
    "#;
    let submit_v = run_once(submit_src);
    let Value::Map(submit_top) = &submit_v else {
        panic!(
            "expected top-level map from first-party gfx/gpu submit test, got {}",
            submit_v.debug_repr()
        );
    };
    for key in [":submit", ":limits", ":features"] {
        let Some(value) = submit_top.get(&TermOrdKey(Term::symbol(key))) else {
            panic!("missing {key} in gfx/gpu submit response map");
        };
        assert!(
            has_backend(value),
            "{key} should report first-party backend"
        );
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
    let fixture = mk_bridge_policy(&[
        "editor/watch::subscribe",
        "editor/watch::poll",
        "editor/task::spawn",
        "editor/task::poll",
    ]);
    let pol = &fixture.policy;
    let r1 = run(&mut ctx1, pol, prog1, h, "gc_effects-test".to_string()).expect("run");
    assert_eq!(r1.log.entries.len(), 4);

    let mut ctx2 = EvalCtx::new();
    let prelude2 = build_prelude(&mut ctx2);
    let mut env2 = prelude2.env;
    let prog2 = eval_module(&mut ctx2, &mut env2, &forms).expect("eval2");
    let v2 = replay(&mut ctx2, prog2, &r1.log).expect("replay");
    assert_eq!(value_hash(&r1.value), value_hash(&v2));
}

#[test]
fn gfx_window_input_audio_first_party_profiles_are_replayable_without_bridge() {
    let run_once = |pol: &CapsPolicy, src: &str| -> Value {
        let forms = parse_module(src).expect("parse module");
        let h = hash_module(&forms);
        let mut ctx1 = EvalCtx::new();
        let prelude1 = build_prelude(&mut ctx1);
        let mut env1 = prelude1.env;
        let prog1 = eval_module(&mut ctx1, &mut env1, &forms).expect("eval1");
        let r1 = run(&mut ctx1, pol, prog1, h, "gc_effects-test".to_string()).expect("run");
        let mut ctx2 = EvalCtx::new();
        let prelude2 = build_prelude(&mut ctx2);
        let mut env2 = prelude2.env;
        let prog2 = eval_module(&mut ctx2, &mut env2, &forms).expect("eval2");
        let v2 = replay(&mut ctx2, prog2, &r1.log).expect("replay");
        assert_eq!(value_hash(&r1.value), value_hash(&v2));
        r1.value
    };

    let headless_pol = CapsPolicy::from_toml_str(
        r#"
allow = [
  "gfx/window::create-surface",
  "gfx/window::request-redraw",
  "gfx/window::surface-info",
  "gfx/input::poll-events",
  "gfx/audio::set-master",
  "gfx/audio::enqueue"
]
"#,
    )
    .expect("headless caps");

    let interactive_pol = CapsPolicy::from_toml_str(
        r#"
allow = [
  "gfx/window::create-surface",
  "gfx/window::request-redraw",
  "gfx/window::surface-info",
  "gfx/input::poll-events",
  "gfx/audio::set-master",
  "gfx/audio::enqueue"
]

[op."gfx/window::create-surface"]
first_party_profile = "interactive"

[op."gfx/window::request-redraw"]
first_party_profile = "interactive"

[op."gfx/window::surface-info"]
first_party_profile = "interactive"

[op."gfx/input::poll-events"]
first_party_profile = "interactive"

[op."gfx/audio::set-master"]
first_party_profile = "interactive"

[op."gfx/audio::enqueue"]
first_party_profile = "interactive"
"#,
    )
    .expect("interactive caps");

    let desktop_pol = CapsPolicy::from_toml_str(
        r#"
allow = [
  "gfx/window::create-surface",
  "gfx/window::request-redraw",
  "gfx/window::surface-info",
  "gfx/input::poll-events",
  "gfx/audio::set-master",
  "gfx/audio::enqueue"
]

[op."gfx/window::create-surface"]
first_party_profile = "desktop"

[op."gfx/window::request-redraw"]
first_party_profile = "desktop"

[op."gfx/window::surface-info"]
first_party_profile = "desktop"

[op."gfx/input::poll-events"]
first_party_profile = "desktop"

[op."gfx/audio::set-master"]
first_party_profile = "desktop"

[op."gfx/audio::enqueue"]
first_party_profile = "desktop"
"#,
    )
    .expect("desktop caps");

    let browser_pol = CapsPolicy::from_toml_str(
        r#"
allow = [
  "gfx/window::create-surface",
  "gfx/window::request-redraw",
  "gfx/window::surface-info",
  "gfx/input::poll-events",
  "gfx/audio::set-master",
  "gfx/audio::enqueue"
]

[op."gfx/window::create-surface"]
first_party_profile = "browser"

[op."gfx/window::request-redraw"]
first_party_profile = "browser"

[op."gfx/window::surface-info"]
first_party_profile = "browser"

[op."gfx/input::poll-events"]
first_party_profile = "browser"

[op."gfx/audio::set-master"]
first_party_profile = "browser"

[op."gfx/audio::enqueue"]
first_party_profile = "browser"
"#,
    )
    .expect("browser caps");

    #[cfg(not(target_os = "wasi"))]
    let expected_interactive_backend = "terminal-host";
    #[cfg(target_os = "wasi")]
    let expected_interactive_backend = "first-party-runtime";

    #[cfg(not(target_os = "wasi"))]
    let expected_interactive_adapter = "terminal-host";
    #[cfg(target_os = "wasi")]
    let expected_interactive_adapter = "noop";

    let create_src = r#"
        (def prog
          (core/effect::perform
            'gfx/window::create-surface
            {:opts {:height 600 :title "main" :width 800}}
            (fn (x) (core/effect::pure x))))
        prog
    "#;
    let create_v = run_once(&headless_pol, create_src);
    let Some(Term::Map(create_resp)) = create_v.as_data() else {
        panic!("expected create-surface map response");
    };
    assert_eq!(
        create_resp.get(&TermOrdKey(Term::symbol(":backend"))),
        Some(&Term::Str("first-party-runtime".to_string()))
    );
    assert_eq!(
        create_resp.get(&TermOrdKey(Term::symbol(":adapter"))),
        Some(&Term::Str("headless-sim".to_string()))
    );

    let desktop_create_v = run_once(&desktop_pol, create_src);
    let Some(Term::Map(desktop_create_resp)) = desktop_create_v.as_data() else {
        panic!("expected desktop create-surface map response");
    };
    assert_eq!(
        desktop_create_resp.get(&TermOrdKey(Term::symbol(":backend"))),
        Some(&Term::Str("first-party-runtime".to_string()))
    );
    assert_eq!(
        desktop_create_resp.get(&TermOrdKey(Term::symbol(":adapter"))),
        Some(&Term::Str("desktop-host".to_string()))
    );
    #[cfg(feature = "gfx-desktop-backend")]
    assert_eq!(
        desktop_create_resp.get(&TermOrdKey(Term::symbol(":created"))),
        Some(&Term::Bool(true))
    );
    #[cfg(not(feature = "gfx-desktop-backend"))]
    assert_eq!(
        desktop_create_resp.get(&TermOrdKey(Term::symbol(":created"))),
        Some(&Term::Bool(false))
    );

    let browser_create_v = run_once(&browser_pol, create_src);
    let Some(Term::Map(browser_create_resp)) = browser_create_v.as_data() else {
        panic!("expected browser create-surface map response");
    };
    assert_eq!(
        browser_create_resp.get(&TermOrdKey(Term::symbol(":backend"))),
        Some(&Term::Str("browser-first-party-runtime".to_string()))
    );
    assert_eq!(
        browser_create_resp.get(&TermOrdKey(Term::symbol(":adapter"))),
        Some(&Term::Str("browser-host".to_string()))
    );
    assert_eq!(
        browser_create_resp.get(&TermOrdKey(Term::symbol(":created"))),
        Some(&Term::Bool(true))
    );

    let audio_src = r#"
        (def prog
          (core/effect::perform
            'gfx/audio::set-master
            {:gain 1}
            (fn (x) (core/effect::pure x))))
        prog
    "#;
    let audio_v = run_once(&headless_pol, audio_src);
    let Some(Term::Map(audio_resp)) = audio_v.as_data() else {
        panic!("expected audio set-master map response");
    };
    assert_eq!(
        audio_resp.get(&TermOrdKey(Term::symbol(":backend"))),
        Some(&Term::Str("first-party-runtime".to_string()))
    );
    assert_eq!(
        audio_resp.get(&TermOrdKey(Term::symbol(":adapter"))),
        Some(&Term::Str("headless-sim".to_string()))
    );

    let desktop_audio_v = run_once(&desktop_pol, audio_src);
    let Some(Term::Map(desktop_audio_resp)) = desktop_audio_v.as_data() else {
        panic!("expected desktop audio set-master map response");
    };
    assert_eq!(
        desktop_audio_resp.get(&TermOrdKey(Term::symbol(":backend"))),
        Some(&Term::Str("first-party-runtime".to_string()))
    );
    assert_eq!(
        desktop_audio_resp.get(&TermOrdKey(Term::symbol(":adapter"))),
        Some(&Term::Str("desktop-host".to_string()))
    );

    let browser_audio_v = run_once(&browser_pol, audio_src);
    let Some(Term::Map(browser_audio_resp)) = browser_audio_v.as_data() else {
        panic!("expected browser audio set-master map response");
    };
    assert_eq!(
        browser_audio_resp.get(&TermOrdKey(Term::symbol(":backend"))),
        Some(&Term::Str("browser-first-party-runtime".to_string()))
    );
    assert_eq!(
        browser_audio_resp.get(&TermOrdKey(Term::symbol(":adapter"))),
        Some(&Term::Str("browser-host".to_string()))
    );

    let poll_src = r#"
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
    "#;

    let headless_poll_v = run_once(&headless_pol, poll_src);
    let interactive_poll_v = run_once(&interactive_pol, poll_src);
    let desktop_poll_v = run_once(&desktop_pol, poll_src);
    let browser_poll_v = run_once(&browser_pol, poll_src);

    let Some(Term::Map(headless_poll_resp)) = headless_poll_v.as_data() else {
        panic!("expected headless poll map response");
    };
    let Some(Term::Map(interactive_poll_resp)) = interactive_poll_v.as_data() else {
        panic!("expected interactive poll map response");
    };
    let Some(Term::Map(desktop_poll_resp)) = desktop_poll_v.as_data() else {
        panic!("expected desktop poll map response");
    };
    let Some(Term::Map(browser_poll_resp)) = browser_poll_v.as_data() else {
        panic!("expected browser poll map response");
    };
    assert_eq!(
        headless_poll_resp.get(&TermOrdKey(Term::symbol(":backend"))),
        Some(&Term::Str("first-party-runtime".to_string()))
    );
    assert_eq!(
        interactive_poll_resp.get(&TermOrdKey(Term::symbol(":backend"))),
        Some(&Term::Str(expected_interactive_backend.to_string()))
    );
    assert_eq!(
        interactive_poll_resp.get(&TermOrdKey(Term::symbol(":adapter"))),
        Some(&Term::Str(expected_interactive_adapter.to_string()))
    );
    assert_eq!(
        desktop_poll_resp.get(&TermOrdKey(Term::symbol(":backend"))),
        Some(&Term::Str("first-party-runtime".to_string()))
    );
    assert_eq!(
        desktop_poll_resp.get(&TermOrdKey(Term::symbol(":adapter"))),
        Some(&Term::Str("desktop-host".to_string()))
    );
    assert_eq!(
        browser_poll_resp.get(&TermOrdKey(Term::symbol(":backend"))),
        Some(&Term::Str("browser-first-party-runtime".to_string()))
    );
    assert_eq!(
        browser_poll_resp.get(&TermOrdKey(Term::symbol(":adapter"))),
        Some(&Term::Str("browser-host".to_string()))
    );

    let Some(Term::Vector(headless_events)) =
        headless_poll_resp.get(&TermOrdKey(Term::symbol(":events")))
    else {
        panic!("headless poll :events missing");
    };
    assert!(
        headless_events.is_empty(),
        "headless first-party profile should emit no events"
    );

    let Some(Term::Vector(interactive_events)) =
        interactive_poll_resp.get(&TermOrdKey(Term::symbol(":events")))
    else {
        panic!("interactive poll :events missing");
    };
    assert!(
        !interactive_events.is_empty(),
        "interactive first-party profile should emit events"
    );

    let contains_redraw = interactive_events.iter().any(|evt| match evt {
        Term::Map(m) => m.get(&TermOrdKey(Term::symbol(":kind"))) == Some(&Term::symbol(":redraw")),
        _ => false,
    });
    assert!(
        contains_redraw,
        "interactive first-party profile should include a redraw event after request-redraw"
    );

    let Some(Term::Vector(desktop_events)) =
        desktop_poll_resp.get(&TermOrdKey(Term::symbol(":events")))
    else {
        panic!("desktop poll :events missing");
    };
    assert!(
        !desktop_events.is_empty(),
        "desktop first-party profile should include at least redraw event after request-redraw"
    );

    let Some(Term::Vector(browser_events)) =
        browser_poll_resp.get(&TermOrdKey(Term::symbol(":events")))
    else {
        panic!("browser poll :events missing");
    };
    assert!(
        !browser_events.is_empty(),
        "browser first-party profile should include at least redraw event after request-redraw"
    );
}

#[test]
fn gfx_production_profile_defaults_to_non_headless_adapter() {
    let pol = CapsPolicy::from_toml_str(
        r#"
allow = ["gfx/window::create-surface"]

[op."gfx/window::create-surface"]
runtime_profile = "production"
"#,
    )
    .expect("policy");

    let src = r#"
        (def prog
          (core/effect::perform
            'gfx/window::create-surface
            {:opts {:height 600 :title "prod-surface" :width 800}}
            (fn (x) (core/effect::pure x))))
        prog
    "#;
    let forms = parse_module(src).expect("parse module");
    let h = hash_module(&forms);
    let mut ctx1 = EvalCtx::new();
    let prelude1 = build_prelude(&mut ctx1);
    let mut env1 = prelude1.env;
    let prog1 = eval_module(&mut ctx1, &mut env1, &forms).expect("eval1");
    let run_out = run(&mut ctx1, &pol, prog1, h, "gc_effects-test".to_string()).expect("run");

    let mut ctx2 = EvalCtx::new();
    let prelude2 = build_prelude(&mut ctx2);
    let mut env2 = prelude2.env;
    let prog2 = eval_module(&mut ctx2, &mut env2, &forms).expect("eval2");
    let replay_v = replay(&mut ctx2, prog2, &run_out.log).expect("replay");
    assert_eq!(value_hash(&run_out.value), value_hash(&replay_v));

    let Some(Term::Map(resp)) = run_out.value.as_data() else {
        panic!("expected create-surface map response");
    };
    let Some(Term::Str(adapter)) = resp.get(&TermOrdKey(Term::symbol(":adapter"))) else {
        panic!("expected :adapter in create-surface response");
    };
    assert_ne!(adapter, "headless-sim");
    assert_ne!(adapter, "noop");
    let Some(Term::Str(profile)) = resp.get(&TermOrdKey(Term::symbol(":profile"))) else {
        panic!("expected :profile in create-surface response");
    };
    assert_ne!(profile, "headless");

    #[cfg(target_os = "wasi")]
    {
        assert_eq!(adapter, "browser-host");
        assert_eq!(profile, "browser");
        assert_eq!(
            resp.get(&TermOrdKey(Term::symbol(":backend"))),
            Some(&Term::Str("browser-first-party-runtime".to_string()))
        );
    }

    #[cfg(all(not(target_os = "wasi"), feature = "gfx-desktop-backend"))]
    {
        assert_eq!(adapter, "desktop-host");
        assert_eq!(profile, "desktop");
    }

    #[cfg(all(not(target_os = "wasi"), not(feature = "gfx-desktop-backend")))]
    {
        assert_eq!(adapter, "terminal-host");
        assert_eq!(profile, "interactive");
    }
}

#[test]
fn editor_first_party_core_ops_are_replayable_without_bridge() {
    let td = tempfile::tempdir().expect("tempdir");
    let root = td.path();
    std::fs::write(
        root.join("package.toml"),
        r#"
name = "editor-test"
version = "0.0.1"
obligations = []
dependencies = []

[[modules]]
path = "a.gc"
"#,
    )
    .expect("write package.toml");
    std::fs::write(
        root.join("a.gc"),
        "(def ::meta (quote {:exports [pkg/a::x]}))\n(def pkg/a::x 1)\n",
    )
    .expect("write a.gc");

    let pol = CapsPolicy::from_toml_str(&format!(
        r#"
allow = [
  "editor/clipboard::set",
  "editor/clipboard::get",
  "editor/dialog::open",
  "editor/dialog::save",
  "editor/watch::subscribe",
  "editor/watch::poll",
  "io/fs::write",
  "editor/task::spawn",
  "editor/task::poll",
  "editor/task::typecheck-pkg",
  "editor/task::test-pkg"
]

[op."io/fs::write"]
base_dir = "{}"
create_dirs = true
"#,
        root.display()
    ))
    .expect("caps");
    let run_once = |src: &str| -> Value {
        let forms = parse_module(src).expect("parse module");
        let h = hash_module(&forms);
        let mut ctx1 = EvalCtx::new();
        let prelude1 = build_prelude(&mut ctx1);
        let mut env1 = prelude1.env;
        let prog1 = eval_module(&mut ctx1, &mut env1, &forms).expect("eval1");
        let run_out = run(&mut ctx1, &pol, prog1, h, "gc_effects-test".to_string()).expect("run");
        let mut ctx2 = EvalCtx::new();
        let prelude2 = build_prelude(&mut ctx2);
        let mut env2 = prelude2.env;
        let prog2 = eval_module(&mut ctx2, &mut env2, &forms).expect("eval2");
        let replay_v = replay(&mut ctx2, prog2, &run_out.log).expect("replay");
        assert_eq!(value_hash(&run_out.value), value_hash(&replay_v));
        run_out.value
    };

    let clip_src = r#"
        (def prog
          ((core/effect::bind (core/effect::perform
                                'editor/clipboard::set
                                {:mime "text/plain" :data "abc"}
                                (fn (x) (core/effect::pure x))))
            (fn (_)
              (core/effect::perform
                'editor/clipboard::get
                {}
                (fn (x) (core/effect::pure x))))))
        prog
    "#;
    let clip_v = run_once(clip_src);
    let Some(Term::Map(clip_resp)) = clip_v.as_data() else {
        panic!("expected clipboard map response");
    };
    assert_eq!(
        clip_resp.get(&TermOrdKey(Term::symbol(":backend"))),
        Some(&Term::Str("first-party-runtime".to_string()))
    );
    assert_eq!(
        clip_resp.get(&TermOrdKey(Term::symbol(":mime"))),
        Some(&Term::Str("text/plain".to_string()))
    );
    assert_eq!(
        clip_resp.get(&TermOrdKey(Term::symbol(":data"))),
        Some(&Term::Str("abc".to_string()))
    );

    let dialog_src = r#"
        (def prog
          (core/effect::perform
            'editor/dialog::open
            {:default-name "opened.gc"}
            (fn (x) (core/effect::pure x))))
        prog
    "#;
    let dialog_v = run_once(dialog_src);
    let Some(Term::Map(dialog_resp)) = dialog_v.as_data() else {
        panic!("expected dialog map response");
    };
    assert_eq!(
        dialog_resp.get(&TermOrdKey(Term::symbol(":backend"))),
        Some(&Term::Str("first-party-runtime".to_string()))
    );
    let Some(Term::Vector(paths)) = dialog_resp.get(&TermOrdKey(Term::symbol(":paths"))) else {
        panic!("dialog open expected :paths vector");
    };
    assert!(!paths.is_empty(), "dialog open expected at least one path");

    let watch_src = format!(
        r#"
        (def prog
          ((core/effect::bind (core/effect::perform
                                'editor/watch::subscribe
                                {{:globs ["*.gc"] :root "{}"}}
                                (fn (x) (core/effect::pure x))))
            (fn (watch-resp)
              (let ((watch-id ((core/map::get watch-resp) ':watch-id)))
                ((core/effect::bind (core/effect::perform
                                      'io/fs::write
                                      {{:path "{}/new.gc" :data "(def z 1)\n"}}
                                      (fn (x) (core/effect::pure x))))
                  (fn (_)
                    (core/effect::perform
                      'editor/watch::poll
                      {{:watch-id watch-id}}
                      (fn (x) (core/effect::pure x)))))))))
        prog
    "#,
        root.display(),
        root.display()
    );
    let watch_v = run_once(&watch_src);
    let Some(Term::Map(watch_resp)) = watch_v.as_data() else {
        panic!("expected watch map response");
    };
    assert_eq!(
        watch_resp.get(&TermOrdKey(Term::symbol(":backend"))),
        Some(&Term::Str("first-party-runtime".to_string()))
    );
    let Some(Term::Vector(events)) = watch_resp.get(&TermOrdKey(Term::symbol(":events"))) else {
        panic!("watch :events missing");
    };
    assert!(
        !events.is_empty(),
        "first-party watch poll should emit filesystem-derived delta event"
    );
    let Term::Map(first_event) = &events[0] else {
        panic!("watch event should be map");
    };
    assert_eq!(
        first_event.get(&TermOrdKey(Term::symbol(":kind"))),
        Some(&Term::symbol(":create"))
    );
    assert!(
        first_event.contains_key(&TermOrdKey(Term::symbol(":stamp"))),
        "watch event should include :stamp"
    );

    let task_src = r#"
        (def prog
          ((core/effect::bind (core/effect::perform
                                'editor/task::spawn
                                {:input {:source "(def x 1)"}
                                 :task-kind 'editor/task::parse-module}
                                (fn (x) (core/effect::pure x))))
            (fn (spawn-resp)
              (let ((task-id ((core/map::get spawn-resp) ':task-id)))
                (core/effect::perform
                  'editor/task::poll
                  {:task-id task-id}
                  (fn (x) (core/effect::pure x)))))))
        prog
    "#;
    let task_v = run_once(task_src);
    let Some(Term::Map(task_resp)) = task_v.as_data() else {
        panic!("expected task poll map response");
    };
    assert_eq!(
        task_resp.get(&TermOrdKey(Term::symbol(":backend"))),
        Some(&Term::Str("first-party-runtime".to_string()))
    );
    let Some(Term::Map(task_result)) = task_resp.get(&TermOrdKey(Term::symbol(":result"))) else {
        panic!("task poll should contain :result map");
    };
    assert_eq!(
        task_result.get(&TermOrdKey(Term::symbol(":ok"))),
        Some(&Term::Bool(true))
    );
    assert!(
        task_result.contains_key(&TermOrdKey(Term::symbol(":module-h"))),
        "parse-module result should include :module-h"
    );

    let typecheck_src = format!(
        r#"
        (def prog
          ((core/effect::bind (core/effect::perform
                                'editor/task::typecheck-pkg
                                {{:pkg "{}/package.toml"}}
                                (fn (x) (core/effect::pure x))))
            (fn (spawn-resp)
              (let ((task-id ((core/map::get spawn-resp) ':task-id)))
                (core/effect::perform
                  'editor/task::poll
                  {{:task-id task-id}}
                  (fn (x) (core/effect::pure x)))))))
        prog
    "#,
        root.display()
    );
    let typecheck_v = run_once(&typecheck_src);
    let Some(Term::Map(typecheck_resp)) = typecheck_v.as_data() else {
        panic!("expected typecheck map response");
    };
    let Some(Term::Map(typecheck_result)) =
        typecheck_resp.get(&TermOrdKey(Term::symbol(":result")))
    else {
        panic!("typecheck poll should include :result map");
    };
    assert_eq!(
        typecheck_result.get(&TermOrdKey(Term::symbol(":ok"))),
        Some(&Term::Bool(true))
    );
    assert!(
        typecheck_result.contains_key(&TermOrdKey(Term::symbol(":module-count"))),
        "typecheck result should include :module-count"
    );

    let test_src = format!(
        r#"
        (def prog
          ((core/effect::bind (core/effect::perform
                                'editor/task::test-pkg
                                {{:pkg "{}/package.toml"}}
                                (fn (x) (core/effect::pure x))))
            (fn (spawn-resp)
              (let ((task-id ((core/map::get spawn-resp) ':task-id)))
                (core/effect::perform
                  'editor/task::poll
                  {{:task-id task-id}}
                  (fn (x) (core/effect::pure x)))))))
        prog
    "#,
        root.display()
    );
    let test_v = run_once(&test_src);
    let Some(Term::Map(test_resp)) = test_v.as_data() else {
        panic!("expected test-pkg map response");
    };
    let Some(Term::Map(test_result)) = test_resp.get(&TermOrdKey(Term::symbol(":result"))) else {
        panic!("test-pkg poll should include :result map");
    };
    assert_eq!(
        test_result.get(&TermOrdKey(Term::symbol(":ok"))),
        Some(&Term::Bool(true))
    );
    assert_eq!(
        test_result.get(&TermOrdKey(Term::symbol(":passed"))),
        Some(&Term::Bool(true))
    );

    fn value_to_term(value: &Value) -> Term {
        value
            .to_plain_term()
            .unwrap_or_else(|| Term::Str(value.debug_repr()))
    }

    let run_spawn_with_three_polls = |task_kind: &str,
                                      task_input: String|
     -> BTreeMap<TermOrdKey, Term> {
        let src = format!(
            r#"
        (def prog
          ((core/effect::bind (core/effect::perform
                                'editor/task::spawn
                                {{:task-kind '{task_kind}
                                  :input {task_input}}}
                                (fn (x) (core/effect::pure x))))
            (fn (spawn-resp)
              (let ((task-id ((core/map::get spawn-resp) ':task-id)))
                ((core/effect::bind (core/effect::perform
                                      'editor/task::poll
                                      {{:task-id task-id}}
                                      (fn (x) (core/effect::pure x))))
                  (fn (poll-1)
                    ((core/effect::bind (core/effect::perform
                                          'editor/task::poll
                                          {{:task-id task-id}}
                                          (fn (x) (core/effect::pure x))))
                      (fn (poll-2)
                        (core/effect::perform
                          'editor/task::poll
                          {{:task-id task-id}}
                          (fn (poll-3)
                            (core/effect::pure {{:spawn spawn-resp :poll-1 poll-1 :poll-2 poll-2 :poll-3 poll-3}})))))))))))
        prog
    "#
        );
        let value = run_once(&src);
        match value {
            Value::Data(t) => match t.as_ref() {
                Term::Map(resp) => resp.clone(),
                _ => panic!(
                    "expected workflow response map, got {}",
                    Value::Data(t).debug_repr()
                ),
            },
            Value::Map(resp) => resp
                .iter()
                .map(|(key, value)| (key.clone(), value_to_term(value)))
                .collect(),
            _ => panic!("expected workflow response map, got {}", value.debug_repr()),
        }
    };

    let build_resp = run_spawn_with_three_polls(
        "editor/task::build-pkg",
        format!("{{:pkg \"{}/package.toml\"}}", root.display()),
    );
    let Some(Term::Map(build_spawn)) = build_resp.get(&TermOrdKey(Term::symbol(":spawn"))) else {
        panic!("build workflow missing :spawn");
    };
    assert_eq!(
        build_spawn.get(&TermOrdKey(Term::symbol(":state"))),
        Some(&Term::symbol(":running"))
    );
    let Some(Term::Map(build_poll_1)) = build_resp.get(&TermOrdKey(Term::symbol(":poll-1"))) else {
        panic!("build workflow missing :poll-1");
    };
    assert_eq!(
        build_poll_1.get(&TermOrdKey(Term::symbol(":partial-emitted"))),
        Some(&Term::Bool(true))
    );
    assert_eq!(
        build_poll_1.get(&TermOrdKey(Term::symbol(":state"))),
        Some(&Term::symbol(":running"))
    );
    assert_eq!(
        build_poll_1.get(&TermOrdKey(Term::symbol(":result"))),
        Some(&Term::Nil)
    );
    let Some(Term::Map(build_poll_3)) = build_resp.get(&TermOrdKey(Term::symbol(":poll-3"))) else {
        panic!("build workflow missing :poll-3");
    };
    assert_eq!(
        build_poll_3.get(&TermOrdKey(Term::symbol(":state"))),
        Some(&Term::symbol(":done"))
    );
    let Some(Term::Map(build_result)) = build_poll_3.get(&TermOrdKey(Term::symbol(":result")))
    else {
        panic!("build workflow missing final result");
    };
    assert!(
        build_result.contains_key(&TermOrdKey(Term::symbol(":build/targets"))),
        "build result should include :build/targets"
    );
    let Some(Term::Map(build_contract)) =
        build_poll_3.get(&TermOrdKey(Term::symbol(":task-contract")))
    else {
        panic!("build workflow missing :task-contract");
    };
    let Some(Term::Vector(build_required)) =
        build_contract.get(&TermOrdKey(Term::symbol(":schema/required")))
    else {
        panic!("build contract missing :schema/required");
    };
    assert!(
        build_required
            .iter()
            .any(|item| item == &Term::symbol(":pkg")),
        "build contract should require :pkg"
    );

    let run_resp = run_spawn_with_three_polls(
        "editor/task::run-pkg",
        format!(
            "{{:pkg \"{}/package.toml\" :entry \"pkg/a::x\" :args [\"--smoke\"]}}",
            root.display()
        ),
    );
    let Some(Term::Map(run_poll_3)) = run_resp.get(&TermOrdKey(Term::symbol(":poll-3"))) else {
        panic!("run workflow missing :poll-3");
    };
    assert_eq!(
        run_poll_3.get(&TermOrdKey(Term::symbol(":state"))),
        Some(&Term::symbol(":done"))
    );
    let Some(Term::Map(run_result)) = run_poll_3.get(&TermOrdKey(Term::symbol(":result"))) else {
        panic!("run workflow missing final result");
    };
    assert_eq!(
        run_result.get(&TermOrdKey(Term::symbol(":ok"))),
        Some(&Term::Bool(true))
    );
    assert!(
        run_result.contains_key(&TermOrdKey(Term::symbol(":run/launch-contract"))),
        "run result should include :run/launch-contract"
    );

    let debug_resp = run_spawn_with_three_polls(
        "editor/task::debug-pkg",
        format!(
            "{{:pkg \"{}/package.toml\" :entry \"pkg/a::x\" :breakpoints [\"pkg/a::x\"]}}",
            root.display()
        ),
    );
    let Some(Term::Map(debug_poll_3)) = debug_resp.get(&TermOrdKey(Term::symbol(":poll-3"))) else {
        panic!("debug workflow missing :poll-3");
    };
    let Some(Term::Map(debug_result)) = debug_poll_3.get(&TermOrdKey(Term::symbol(":result")))
    else {
        panic!("debug workflow missing final result");
    };
    assert!(
        debug_result.contains_key(&TermOrdKey(Term::symbol(":debug/session-id"))),
        "debug result should include :debug/session-id"
    );

    let refactor_resp = run_spawn_with_three_polls(
        "editor/task::refactor-module",
        "{:source \"(def old/name 1)\\n(def pkg/a::y old/name)\\n\" :from \"old/name\" :to \"new/name\"}".to_string(),
    );
    let Some(Term::Map(refactor_poll_3)) = refactor_resp.get(&TermOrdKey(Term::symbol(":poll-3")))
    else {
        panic!("refactor workflow missing :poll-3");
    };
    let Some(Term::Map(refactor_result)) =
        refactor_poll_3.get(&TermOrdKey(Term::symbol(":result")))
    else {
        panic!("refactor workflow missing final result");
    };
    let Some(Term::Str(updated_source)) =
        refactor_result.get(&TermOrdKey(Term::symbol(":updated")))
    else {
        panic!("refactor workflow missing :updated");
    };
    assert!(
        updated_source.contains("new/name"),
        "refactor result should rewrite old symbol references"
    );

    let index_resp = run_spawn_with_three_polls(
        "editor/task::index-workspace",
        format!(
            "{{:root \"{}\" :max-packages 8 :max-partials 4}}",
            root.display()
        ),
    );
    let Some(Term::Map(index_poll_3)) = index_resp.get(&TermOrdKey(Term::symbol(":poll-3"))) else {
        panic!("index workflow missing :poll-3");
    };
    let Some(Term::Map(index_result)) = index_poll_3.get(&TermOrdKey(Term::symbol(":result")))
    else {
        panic!("index workflow missing final result");
    };
    let Some(Term::Int(pkg_count)) = index_result.get(&TermOrdKey(Term::symbol(":package-count")))
    else {
        panic!("index workflow missing :package-count");
    };
    assert!(
        pkg_count >= &1_i64.into(),
        "index workspace result should include at least one package"
    );
}
