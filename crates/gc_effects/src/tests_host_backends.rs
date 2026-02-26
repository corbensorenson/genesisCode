use super::*;
mod tests_host_backends_first_party;

#[test]
fn editor_clipboard_capability_roundtrip_is_supported_and_replayable() {
    let (forms, h) = mk_prog_for("editor/clipboard::get", "{}");

    let mut ctx1 = EvalCtx::new();
    let prelude1 = build_prelude(&mut ctx1);
    let mut env1 = prelude1.env;
    let prog1 = eval_module(&mut ctx1, &mut env1, &forms).expect("eval1");

    let fixture = mk_bridge_policy(&["editor/clipboard::get"]);
    let pol = &fixture.policy;
    let r1 = run(&mut ctx1, pol, prog1, h, "gc_effects-test".to_string()).expect("run");
    match &r1.value {
        Value::Data(Term::Map(m)) => {
            assert_eq!(
                m.get(&TermOrdKey(Term::symbol(":ok"))),
                Some(&Term::Bool(true))
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
    let fixture = mk_bridge_policy(&[
        "gfx/window::create-surface",
        "gfx/window::request-redraw",
        "gfx/window::surface-info",
        "gfx/input::poll-events",
        "gfx/input::set-cursor-mode",
        "gfx/audio::set-master",
        "gfx/audio::enqueue",
    ]);
    let pol = &fixture.policy;
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
        let r1 = run(&mut ctx1, pol, prog1, h, "gc_effects-test".to_string()).expect("run");
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
    let fixture = mk_bridge_policy(&[
        "gfx/gpu::create-buffer",
        "gfx/gpu::write-buffer",
        "gfx/gpu::read-buffer",
        "gfx/gpu::create-texture",
        "gfx/gpu::write-texture",
        "gfx/gpu::read-texture",
        "gfx/gpu::submit-frame-graph",
        "gpu/compute::submit",
        "gfx/gpu::limits",
        "gfx/gpu::features",
    ]);
    let pol = &fixture.policy;
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
                                        'gpu/compute::submit
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
        let r1 = run(&mut ctx1, pol, prog1, h, "gc_effects-test".to_string()).expect("run");
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
fn gpu_compute_namespace_is_supported_and_policy_isolated() {
    let fixture = mk_bridge_policy(&[
        "gpu/compute::create-buffer",
        "gpu/compute::write-buffer",
        "gpu/compute::read-buffer",
        "gpu/compute::submit",
        "gpu/compute::limits",
        "gpu/compute::features",
    ]);
    let pol = &fixture.policy;
    let cases = [
        r#"
            (def prog
              ((core/effect::bind (core/effect::perform
                                    'gpu/compute::create-buffer
                                    {:desc {:size 8}}
                                    (fn (x) (core/effect::pure x))))
                (fn (create-resp)
                  (let ((id ((core/map::get create-resp) ':id)))
                    ((core/effect::bind (core/effect::perform
                                          'gpu/compute::write-buffer
                                          {:data b"\x01\x02\x03" :id id :offset 2}
                                          (fn (x) (core/effect::pure x))))
                      (fn (_)
                        (core/effect::perform
                          'gpu/compute::read-buffer
                          {:id id :offset 0 :size 8}
                          (fn (x) (core/effect::pure x)))))))))
            prog
            "#,
        r#"
            (def prog
              ((core/effect::bind (core/effect::perform
                                    'gpu/compute::submit
                                    {:graph {:passes []}}
                                    (fn (x) (core/effect::pure x))))
                (fn (_)
                  ((core/effect::bind (core/effect::perform
                                        'gpu/compute::limits
                                        {}
                                        (fn (x) (core/effect::pure x))))
                    (fn (_)
                      (core/effect::perform
                        'gpu/compute::features
                        {}
                        (fn (x) (core/effect::pure x))))))))
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
        let run_out = run(&mut ctx1, pol, prog1, h, "gc_effects-test".to_string()).expect("run");
        assert!(
            !matches!(run_out.value, Value::Sealed { .. }),
            "gpu compute namespace should return structured responses, got {}",
            run_out.value.debug_repr()
        );
        let mut ctx2 = EvalCtx::new();
        let prelude2 = build_prelude(&mut ctx2);
        let mut env2 = prelude2.env;
        let prog2 = eval_module(&mut ctx2, &mut env2, &forms).expect("eval2");
        let replay_v = replay(&mut ctx2, prog2, &run_out.log).expect("replay");
        assert_eq!(value_hash(&run_out.value), value_hash(&replay_v));
    }

    let (deny_forms, deny_h) = mk_prog_for("gfx/gpu::limits", "{}");
    let mut ctx3 = EvalCtx::new();
    let prelude3 = build_prelude(&mut ctx3);
    let mut env3 = prelude3.env;
    let deny_prog = eval_module(&mut ctx3, &mut env3, &deny_forms).expect("eval3");
    let deny_out = run(
        &mut ctx3,
        pol,
        deny_prog,
        deny_h,
        "gc_effects-test".to_string(),
    )
    .expect("run3");
    match deny_out.value {
        Value::Sealed { token, .. } => {
            assert_eq!(token, ctx3.protocol.unwrap().error);
        }
        other => panic!(
            "expected denied gfx/gpu op under compute-only policy, got {}",
            other.debug_repr()
        ),
    }
    assert_eq!(deny_out.log.entries.len(), 1);
    assert_eq!(deny_out.log.entries[0].decision, Decision::Deny);
    assert_eq!(deny_out.log.entries[0].op, "gfx/gpu::limits");
}
#[test]
fn gpu_compute_first_party_backend_runs_without_bridge_profile_and_replays() {
    let pol = CapsPolicy::from_toml_str(
        r#"
allow = [
  "gpu/compute::submit",
  "gpu/compute::limits",
  "gpu/compute::features"
]
"#,
    )
    .expect("parse caps");

    let src = r#"
            (def prog
              ((core/effect::bind (core/effect::perform
                                    'gpu/compute::submit
                                    {:graph {:passes []}}
                                    (fn (x) (core/effect::pure x))))
                (fn (submit)
                  ((core/effect::bind (core/effect::perform
                                        'gpu/compute::limits
                                        {}
                                        (fn (x) (core/effect::pure x))))
                    (fn (limits)
                      ((core/effect::bind (core/effect::perform
                                            'gpu/compute::features
                                            {}
                                            (fn (x) (core/effect::pure x))))
                        (fn (features)
                          (core/effect::pure
                            {:features features :limits limits :submit submit}))))))))
            prog
            "#;

    let forms = parse_module(src).expect("parse module");
    let h = hash_module(&forms);

    let mut ctx1 = EvalCtx::new();
    let prelude1 = build_prelude(&mut ctx1);
    let mut env1 = prelude1.env;
    let prog1 = eval_module(&mut ctx1, &mut env1, &forms).expect("eval1");
    let run_out = run(&mut ctx1, &pol, prog1, h, "gc_effects-test".to_string()).expect("run");

    let Value::Map(top) = &run_out.value else {
        panic!(
            "expected top-level map from first-party compute test, got {}",
            run_out.value.debug_repr()
        );
    };

    let has_backend = |v: &Value| match v {
        Value::Map(m) => matches!(
            m.get(&TermOrdKey(Term::symbol(":backend"))),
            Some(Value::Data(Term::Str(s))) if s == "first-party-runtime"
        ),
        Value::Data(Term::Map(m)) => matches!(
            m.get(&TermOrdKey(Term::symbol(":backend"))),
            Some(Term::Str(s)) if s == "first-party-runtime"
        ),
        _ => false,
    };

    let Some(submit) = top.get(&TermOrdKey(Term::symbol(":submit"))) else {
        panic!(
            "missing :submit map in first-party compute response: {}",
            run_out.value.debug_repr()
        );
    };
    assert!(
        has_backend(submit),
        "expected :submit :backend=first-party-runtime, got {}",
        run_out.value.debug_repr()
    );

    let Some(limits) = top.get(&TermOrdKey(Term::symbol(":limits"))) else {
        panic!(
            "missing :limits map in first-party compute response: {}",
            run_out.value.debug_repr()
        );
    };
    assert!(
        has_backend(limits),
        "expected :limits :backend=first-party-runtime, got {}",
        run_out.value.debug_repr()
    );

    let Some(features) = top.get(&TermOrdKey(Term::symbol(":features"))) else {
        panic!(
            "missing :features map in first-party compute response: {}",
            run_out.value.debug_repr()
        );
    };
    assert!(
        has_backend(features),
        "expected :features :backend=first-party-runtime, got {}",
        run_out.value.debug_repr()
    );

    let mut ctx2 = EvalCtx::new();
    let prelude2 = build_prelude(&mut ctx2);
    let mut env2 = prelude2.env;
    let prog2 = eval_module(&mut ctx2, &mut env2, &forms).expect("eval2");
    let replay_v = replay(&mut ctx2, prog2, &run_out.log).expect("replay");
    assert_eq!(value_hash(&run_out.value), value_hash(&replay_v));
}
#[test]
fn gpu_compute_device_backend_require_device_fails_closed_without_feature() {
    let pol = CapsPolicy::from_toml_str(
        r#"
allow = ["gpu/compute::submit"]

[op."gpu/compute::submit"]
gpu_backend = "device-runtime"
gpu_backend_policy = "require-device"
"#,
    )
    .expect("parse caps");

    let (forms, h) = mk_prog_for("gpu/compute::submit", "{:graph {:passes []}}");
    let mut ctx = EvalCtx::new();
    let prelude = build_prelude(&mut ctx);
    let mut env = prelude.env;
    let prog = eval_module(&mut ctx, &mut env, &forms).expect("eval");
    let out = run(&mut ctx, &pol, prog, h, "gc_effects-test".to_string()).expect("run");

    match out.value {
        Value::Sealed { token, .. } => {
            assert_eq!(token, ctx.protocol.expect("protocol").error);
        }
        other => panic!(
            "expected sealed error when device backend is required, got {}",
            other.debug_repr()
        ),
    }
}
#[test]
fn gpu_compute_device_backend_allow_fallback_is_replayable() {
    let pol = CapsPolicy::from_toml_str(
        r#"
allow = ["gpu/compute::submit"]

[op."gpu/compute::submit"]
gpu_backend = "device-runtime"
gpu_backend_policy = "allow-fallback"
"#,
    )
    .expect("parse caps");

    let (forms, h) = mk_prog_for("gpu/compute::submit", "{:graph {:passes []}}");
    let mut ctx1 = EvalCtx::new();
    let prelude1 = build_prelude(&mut ctx1);
    let mut env1 = prelude1.env;
    let prog1 = eval_module(&mut ctx1, &mut env1, &forms).expect("eval1");
    let out = run(&mut ctx1, &pol, prog1, h, "gc_effects-test".to_string()).expect("run");

    let Value::Data(Term::Map(map)) = &out.value else {
        panic!(
            "expected map response for fallback submit, got {}",
            out.value.debug_repr()
        );
    };
    assert_eq!(
        map.get(&TermOrdKey(Term::symbol(":backend"))),
        Some(&Term::Str("first-party-runtime".to_string()))
    );
    assert_eq!(
        map.get(&TermOrdKey(Term::symbol(":backend-fallback-from"))),
        Some(&Term::Str("device-runtime".to_string()))
    );

    let mut ctx2 = EvalCtx::new();
    let prelude2 = build_prelude(&mut ctx2);
    let mut env2 = prelude2.env;
    let prog2 = eval_module(&mut ctx2, &mut env2, &forms).expect("eval2");
    let replay_v = replay(&mut ctx2, prog2, &out.log).expect("replay");
    assert_eq!(value_hash(&out.value), value_hash(&replay_v));
}
#[test]
fn gpu_compute_device_runtime_submit_scope_keeps_lifecycle_on_first_party() {
    let pol = CapsPolicy::from_toml_str(
        r#"
allow = [
  "gpu/compute::create-buffer",
  "gpu/compute::write-buffer",
  "gpu/compute::read-buffer",
  "gpu/compute::destroy-resource"
]

[op."gpu/compute::create-buffer"]
gpu_backend = "device-runtime"

[op."gpu/compute::write-buffer"]
gpu_backend = "device-runtime"

[op."gpu/compute::read-buffer"]
gpu_backend = "device-runtime"

[op."gpu/compute::destroy-resource"]
gpu_backend = "device-runtime"
"#,
    )
    .expect("parse caps");

    let src = r#"
        (def prog
          ((core/effect::bind (core/effect::perform
                                'gpu/compute::create-buffer
                                {:desc {:size 8}}
                                (fn (x) (core/effect::pure x))))
            (fn (create-buffer)
              (let ((buffer-id ((core/map::get create-buffer) ':id)))
                ((core/effect::bind (core/effect::perform
                                      'gpu/compute::write-buffer
                                      {:data b"\x01\x02\x03" :id buffer-id :offset 2}
                                      (fn (x) (core/effect::pure x))))
                  (fn (write-buffer)
                    ((core/effect::bind (core/effect::perform
                                          'gpu/compute::read-buffer
                                          {:id buffer-id :offset 0 :size 8}
                                          (fn (x) (core/effect::pure x))))
                      (fn (read-buffer)
                        ((core/effect::bind (core/effect::perform
                                              'gpu/compute::destroy-resource
                                              {:id buffer-id}
                                              (fn (x) (core/effect::pure x))))
                          (fn (destroy-buffer)
                            (core/effect::pure
                              {:create-buffer create-buffer
                               :destroy-buffer destroy-buffer
                               :read-buffer read-buffer
                               :write-buffer write-buffer})))))))))))
        prog
    "#;
    let forms = parse_module(src).expect("parse module");
    let h = hash_module(&forms);
    let mut ctx1 = EvalCtx::new();
    let prelude1 = build_prelude(&mut ctx1);
    let mut env1 = prelude1.env;
    let prog1 = eval_module(&mut ctx1, &mut env1, &forms).expect("eval1");
    let run_out = run(&mut ctx1, &pol, prog1, h, "gc_effects-test".to_string()).expect("run");

    let Value::Map(top) = &run_out.value else {
        panic!("expected top-level map, got {}", run_out.value.debug_repr());
    };
    for key in [
        ":create-buffer",
        ":write-buffer",
        ":read-buffer",
        ":destroy-buffer",
    ] {
        let Some(Value::Data(Term::Map(entry))) = top.get(&TermOrdKey(Term::symbol(key))) else {
            panic!("missing map entry {key}");
        };
        assert_eq!(
            entry.get(&TermOrdKey(Term::symbol(":backend"))),
            Some(&Term::Str("first-party-runtime".to_string())),
            "{key} should remain on first-party backend under submit/introspection scope"
        );
        assert!(
            !entry.contains_key(&TermOrdKey(Term::symbol(":backend-fallback-from"))),
            "{key} should not carry fallback metadata under submit/introspection scope"
        );
    }

    let mut ctx2 = EvalCtx::new();
    let prelude2 = build_prelude(&mut ctx2);
    let mut env2 = prelude2.env;
    let prog2 = eval_module(&mut ctx2, &mut env2, &forms).expect("eval2");
    let replay_v = replay(&mut ctx2, prog2, &run_out.log).expect("replay");
    assert_eq!(value_hash(&run_out.value), value_hash(&replay_v));
}

#[test]
fn gpu_compute_device_runtime_full_lifecycle_require_device_fails_closed() {
    let pol = CapsPolicy::from_toml_str(
        r#"
allow = ["gpu/compute::create-buffer"]

[op."gpu/compute::create-buffer"]
gpu_backend = "device-runtime-full"
gpu_backend_policy = "require-device"
"#,
    )
    .expect("parse caps");

    let (forms, h) = mk_prog_for("gpu/compute::create-buffer", "{:desc {:size 16}}");
    let mut ctx = EvalCtx::new();
    let prelude = build_prelude(&mut ctx);
    let mut env = prelude.env;
    let prog = eval_module(&mut ctx, &mut env, &forms).expect("eval");
    let out = run(&mut ctx, &pol, prog, h, "gc_effects-test".to_string()).expect("run");

    match out.value {
        Value::Sealed { token, .. } => {
            assert_eq!(token, ctx.protocol.expect("protocol").error);
        }
        other => panic!(
            "expected sealed error when full lifecycle backend is required, got {}",
            other.debug_repr()
        ),
    }
}

#[test]
fn gpu_compute_device_runtime_full_lifecycle_allow_fallback_marks_lifecycle_ops() {
    let pol = CapsPolicy::from_toml_str(
        r#"
allow = [
  "gpu/compute::create-buffer",
  "gpu/compute::write-buffer",
  "gpu/compute::read-buffer",
  "gpu/compute::destroy-resource"
]

[op."gpu/compute::create-buffer"]
gpu_backend = "device-runtime-full"
gpu_backend_policy = "allow-fallback"

[op."gpu/compute::write-buffer"]
gpu_backend = "device-runtime-full"
gpu_backend_policy = "allow-fallback"

[op."gpu/compute::read-buffer"]
gpu_backend = "device-runtime-full"
gpu_backend_policy = "allow-fallback"

[op."gpu/compute::destroy-resource"]
gpu_backend = "device-runtime-full"
gpu_backend_policy = "allow-fallback"
"#,
    )
    .expect("parse caps");

    let src = r#"
        (def prog
          ((core/effect::bind (core/effect::perform
                                'gpu/compute::create-buffer
                                {:desc {:size 8}}
                                (fn (x) (core/effect::pure x))))
            (fn (create-buffer)
              (let ((buffer-id ((core/map::get create-buffer) ':id)))
                ((core/effect::bind (core/effect::perform
                                      'gpu/compute::write-buffer
                                      {:data b"\x01\x02\x03" :id buffer-id :offset 2}
                                      (fn (x) (core/effect::pure x))))
                  (fn (write-buffer)
                    ((core/effect::bind (core/effect::perform
                                          'gpu/compute::read-buffer
                                          {:id buffer-id :offset 0 :size 8}
                                          (fn (x) (core/effect::pure x))))
                      (fn (read-buffer)
                        ((core/effect::bind (core/effect::perform
                                              'gpu/compute::destroy-resource
                                              {:id buffer-id}
                                              (fn (x) (core/effect::pure x))))
                          (fn (destroy-buffer)
                            (core/effect::pure
                              {:create-buffer create-buffer
                               :destroy-buffer destroy-buffer
                               :read-buffer read-buffer
                               :write-buffer write-buffer})))))))))))
        prog
    "#;
    let forms = parse_module(src).expect("parse module");
    let h = hash_module(&forms);
    let mut ctx1 = EvalCtx::new();
    let prelude1 = build_prelude(&mut ctx1);
    let mut env1 = prelude1.env;
    let prog1 = eval_module(&mut ctx1, &mut env1, &forms).expect("eval1");
    let run_out = run(&mut ctx1, &pol, prog1, h, "gc_effects-test".to_string()).expect("run");

    let Value::Map(top) = &run_out.value else {
        panic!("expected top-level map, got {}", run_out.value.debug_repr());
    };
    for key in [
        ":create-buffer",
        ":write-buffer",
        ":read-buffer",
        ":destroy-buffer",
    ] {
        let Some(Value::Data(Term::Map(entry))) = top.get(&TermOrdKey(Term::symbol(key))) else {
            panic!("missing map entry {key}");
        };
        assert_eq!(
            entry.get(&TermOrdKey(Term::symbol(":backend"))),
            Some(&Term::Str("first-party-runtime".to_string()))
        );
        assert_eq!(
            entry.get(&TermOrdKey(Term::symbol(":backend-fallback-from"))),
            Some(&Term::Str("device-runtime-full".to_string())),
            "{key} should be marked as fallback from full lifecycle device backend"
        );
    }

    let mut ctx2 = EvalCtx::new();
    let prelude2 = build_prelude(&mut ctx2);
    let mut env2 = prelude2.env;
    let prog2 = eval_module(&mut ctx2, &mut env2, &forms).expect("eval2");
    let replay_v = replay(&mut ctx2, prog2, &run_out.log).expect("replay");
    assert_eq!(value_hash(&run_out.value), value_hash(&replay_v));
}

#[test]
fn gpu_compute_first_party_lifecycle_ops_are_replayable_without_bridge() {
    let pol = CapsPolicy::from_toml_str(
        r#"
allow = [
  "gpu/compute::create-buffer",
  "gpu/compute::create-kernel",
  "gpu/compute::write-buffer",
  "gpu/compute::read-buffer",
  "gpu/compute::destroy-resource"
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
            m.get(&TermOrdKey(Term::symbol(":backend"))),
            Some(Value::Data(Term::Str(s))) if s == "first-party-runtime"
        ),
        Value::Data(Term::Map(m)) => matches!(
            m.get(&TermOrdKey(Term::symbol(":backend"))),
            Some(Term::Str(s)) if s == "first-party-runtime"
        ),
        _ => false,
    };

    let buffer_src = r#"
        (def prog
          ((core/effect::bind (core/effect::perform
                                'gpu/compute::create-buffer
                                {:desc {:size 8}}
                                (fn (x) (core/effect::pure x))))
            (fn (create-buffer)
              (let ((buffer-id ((core/map::get create-buffer) ':id)))
                ((core/effect::bind (core/effect::perform
                                      'gpu/compute::write-buffer
                                      {:data b"\x01\x02\x03" :id buffer-id :offset 2}
                                      (fn (x) (core/effect::pure x))))
                  (fn (write-buffer)
                    ((core/effect::bind (core/effect::perform
                                          'gpu/compute::read-buffer
                                          {:id buffer-id :offset 0 :size 8}
                                          (fn (x) (core/effect::pure x))))
                      (fn (read-buffer)
                        ((core/effect::bind (core/effect::perform
                                              'gpu/compute::destroy-resource
                                              {:id buffer-id}
                                              (fn (x) (core/effect::pure x))))
                          (fn (destroy-buffer)
                            (core/effect::pure
                              {:create-buffer create-buffer
                               :destroy-buffer destroy-buffer
                               :read-buffer read-buffer
                               :write-buffer write-buffer})))))))))))
        prog
    "#;
    let buffer_v = run_once(buffer_src);
    let Value::Map(buffer_top) = &buffer_v else {
        panic!(
            "expected top-level map from first-party compute buffer lifecycle test, got {}",
            buffer_v.debug_repr()
        );
    };

    let Some(Value::Data(Term::Map(create_buffer))) =
        buffer_top.get(&TermOrdKey(Term::symbol(":create-buffer")))
    else {
        panic!("expected :create-buffer map response");
    };
    assert!(has_backend(&Value::Data(Term::Map(create_buffer.clone()))));
    assert_eq!(
        create_buffer.get(&TermOrdKey(Term::symbol(":kind"))),
        Some(&Term::symbol(":buffer"))
    );

    let Some(Value::Data(Term::Map(write_buffer))) =
        buffer_top.get(&TermOrdKey(Term::symbol(":write-buffer")))
    else {
        panic!("expected :write-buffer map response");
    };
    assert!(has_backend(&Value::Data(Term::Map(write_buffer.clone()))));
    assert_eq!(
        write_buffer.get(&TermOrdKey(Term::symbol(":written"))),
        Some(&Term::Int(3_i64.into()))
    );

    let Some(Value::Data(Term::Map(read_buffer))) =
        buffer_top.get(&TermOrdKey(Term::symbol(":read-buffer")))
    else {
        panic!("expected :read-buffer map response");
    };
    assert!(has_backend(&Value::Data(Term::Map(read_buffer.clone()))));
    let Some(Term::Bytes(read_bytes)) = read_buffer.get(&TermOrdKey(Term::symbol(":data"))) else {
        panic!("expected :read-buffer :data bytes");
    };
    assert_eq!(read_bytes.as_ref(), &[0_u8, 0, 1, 2, 3, 0, 0, 0]);

    let Some(Value::Data(Term::Map(destroy_buffer))) =
        buffer_top.get(&TermOrdKey(Term::symbol(":destroy-buffer")))
    else {
        panic!("expected :destroy-buffer map response");
    };
    assert!(has_backend(&Value::Data(Term::Map(destroy_buffer.clone()))));
    assert_eq!(
        destroy_buffer.get(&TermOrdKey(Term::symbol(":destroyed"))),
        Some(&Term::Bool(true))
    );

    let kernel_src = r#"
        (def prog
          ((core/effect::bind (core/effect::perform
                                'gpu/compute::create-kernel
                                {:entry "main"}
                                (fn (x) (core/effect::pure x))))
            (fn (create-kernel)
              (let ((kernel-id ((core/map::get create-kernel) ':id)))
                ((core/effect::bind (core/effect::perform
                                      'gpu/compute::destroy-resource
                                      {:id kernel-id}
                                      (fn (x) (core/effect::pure x))))
                  (fn (destroy-kernel)
                    (core/effect::pure
                      {:create-kernel create-kernel
                       :destroy-kernel destroy-kernel})))))))
        prog
    "#;
    let kernel_v = run_once(kernel_src);
    let Value::Map(kernel_top) = &kernel_v else {
        panic!(
            "expected top-level map from first-party compute kernel lifecycle test, got {}",
            kernel_v.debug_repr()
        );
    };

    let Some(Value::Data(Term::Map(create_kernel))) =
        kernel_top.get(&TermOrdKey(Term::symbol(":create-kernel")))
    else {
        panic!("expected :create-kernel map response");
    };
    assert!(has_backend(&Value::Data(Term::Map(create_kernel.clone()))));
    assert_eq!(
        create_kernel.get(&TermOrdKey(Term::symbol(":kind"))),
        Some(&Term::Str("compute-pipeline".to_string()))
    );
    assert!(
        !create_kernel.contains_key(&TermOrdKey(Term::symbol(":payload-h"))),
        "first-party kernel creation must not rely on payload hash placeholders"
    );
    assert!(
        create_kernel.contains_key(&TermOrdKey(Term::symbol(":descriptor"))),
        "first-party kernel creation must keep structured descriptor metadata"
    );

    let Some(Value::Data(Term::Map(destroy_kernel))) =
        kernel_top.get(&TermOrdKey(Term::symbol(":destroy-kernel")))
    else {
        panic!("expected :destroy-kernel map response");
    };
    assert!(has_backend(&Value::Data(Term::Map(destroy_kernel.clone()))));
    assert_eq!(
        destroy_kernel.get(&TermOrdKey(Term::symbol(":destroyed"))),
        Some(&Term::Bool(true))
    );
    assert!(
        !destroy_kernel.contains_key(&TermOrdKey(Term::symbol(":payload-h"))),
        "first-party kernel destroy response must not leak placeholder payload hashes"
    );
}

#[cfg(feature = "gpu-device-backend")]
#[test]
fn gpu_compute_device_runtime_full_create_buffer_is_device_backed_and_replayable() {
    let pol = CapsPolicy::from_toml_str(
        r#"
allow = ["gpu/compute::create-buffer"]

[op."gpu/compute::create-buffer"]
gpu_backend = "device-runtime-full"
gpu_backend_policy = "require-device"
"#,
    )
    .expect("parse caps");
    let (forms, h) = mk_prog_for("gpu/compute::create-buffer", "{:desc {:size 8}}");

    let mut ctx1 = EvalCtx::new();
    let prelude1 = build_prelude(&mut ctx1);
    let mut env1 = prelude1.env;
    let prog1 = eval_module(&mut ctx1, &mut env1, &forms).expect("eval1");
    let run_out = run(&mut ctx1, &pol, prog1, h, "gc_effects-test".to_string()).expect("run");

    let Value::Data(Term::Map(entry)) = &run_out.value else {
        panic!(
            "expected create-buffer map for device-runtime-full test, got {}",
            run_out.value.debug_repr()
        );
    };
    assert!(
        !entry.contains_key(&TermOrdKey(Term::symbol(":backend-fallback-from"))),
        "create-buffer should not carry fallback metadata"
    );
    assert_eq!(
        entry.get(&TermOrdKey(Term::symbol(":backend"))),
        Some(&Term::Str("device-runtime-full".to_string()))
    );
    assert_eq!(
        entry.get(&TermOrdKey(Term::symbol(":kind"))),
        Some(&Term::symbol(":buffer"))
    );

    let mut ctx2 = EvalCtx::new();
    let prelude2 = build_prelude(&mut ctx2);
    let mut env2 = prelude2.env;
    let prog2 = eval_module(&mut ctx2, &mut env2, &forms).expect("eval2");
    let replay_v = replay(&mut ctx2, prog2, &run_out.log).expect("replay");
    assert_eq!(value_hash(&run_out.value), value_hash(&replay_v));
}

#[cfg(feature = "gpu-device-backend")]
#[test]
fn gpu_compute_device_runtime_full_buffer_interop_is_replayable() {
    let pol = CapsPolicy::from_toml_str(
        r#"
allow = [
  "gpu/compute::create-buffer",
  "gpu/compute::write-buffer",
  "gpu/compute::submit",
  "gpu/compute::read-buffer"
]

[op."gpu/compute::create-buffer"]
gpu_backend = "device-runtime-full"
gpu_backend_policy = "require-device"

[op."gpu/compute::write-buffer"]
gpu_backend = "device-runtime-full"
gpu_backend_policy = "require-device"

[op."gpu/compute::submit"]
gpu_backend = "device-runtime-full"
gpu_backend_policy = "require-device"
"#,
    )
    .expect("parse caps");
    let term_map = |entries: Vec<(&str, Term)>| -> Term {
        Term::Map(
            entries
                .into_iter()
                .map(|(key, value)| (TermOrdKey(Term::symbol(key)), value))
                .collect(),
        )
    };
    let map_get_string = |term: &Term, key: &str| -> String {
        let Term::Map(map) = term else {
            panic!("expected map response, got {term:?}");
        };
        let Some(Term::Str(id)) = map.get(&TermOrdKey(Term::symbol(key))) else {
            panic!("missing string key `{key}` in response: {term:?}");
        };
        id.clone()
    };

    let create_payload = term_map(vec![(
        ":desc",
        term_map(vec![(":size", Term::Int(16_i64.into()))]),
    )]);
    let create_in = crate::runner_gpu_device_backend::call_device_backend(
        "gpu/compute::create-buffer",
        &create_payload,
    )
    .expect("create input buffer");
    let create_out = crate::runner_gpu_device_backend::call_device_backend(
        "gpu/compute::create-buffer",
        &create_payload,
    )
    .expect("create output buffer");
    let in_id = map_get_string(&create_in, ":id");
    let out_id = map_get_string(&create_out, ":id");

    let write_payload = term_map(vec![
        (":id", Term::Str(in_id.clone())),
        (":offset", Term::Int(0_i64.into())),
        (
            ":data",
            Term::Bytes(vec![1_u8, 0, 0, 0, 2, 0, 0, 0, 3, 0, 0, 0, 4, 0, 0, 0].into()),
        ),
    ]);
    crate::runner_gpu_device_backend::call_device_backend(
        "gpu/compute::write-buffer",
        &write_payload,
    )
    .expect("seed input buffer");

    let submit_payload =
        format!("{{:in-buffer-id \"{in_id}\" :out-buffer-id \"{out_id}\" :count 4}}");
    let (forms, h) = mk_prog_for("gpu/compute::submit", &submit_payload);

    let mut ctx1 = EvalCtx::new();
    let prelude1 = build_prelude(&mut ctx1);
    let mut env1 = prelude1.env;
    let prog1 = eval_module(&mut ctx1, &mut env1, &forms).expect("eval1");
    let run_out = run(&mut ctx1, &pol, prog1, h, "gc_effects-test".to_string()).expect("run");

    let Value::Data(Term::Map(submit)) = &run_out.value else {
        panic!(
            "expected submit map in interop response, got {}",
            run_out.value.debug_repr()
        );
    };
    assert_eq!(
        submit.get(&TermOrdKey(Term::symbol(":interop"))),
        Some(&Term::Bool(true))
    );
    assert_eq!(
        submit.get(&TermOrdKey(Term::symbol(":backend"))),
        Some(&Term::Str("device-runtime".to_string()))
    );

    let mut ctx2 = EvalCtx::new();
    let prelude2 = build_prelude(&mut ctx2);
    let mut env2 = prelude2.env;
    let prog2 = eval_module(&mut ctx2, &mut env2, &forms).expect("eval2");
    let replay_v = replay(&mut ctx2, prog2, &run_out.log).expect("replay");
    assert_eq!(value_hash(&run_out.value), value_hash(&replay_v));

    let read_out = crate::runner_gpu_device_backend::call_device_backend(
        "gpu/compute::read-buffer",
        &term_map(vec![
            (":id", Term::Str(out_id.clone())),
            (":offset", Term::Int(0_i64.into())),
            (":size", Term::Int(16_i64.into())),
        ]),
    )
    .expect("read transformed output");
    let Term::Map(read_map) = &read_out else {
        panic!("expected read-buffer map, got {read_out:?}");
    };
    let Some(Term::Bytes(bytes)) = read_map.get(&TermOrdKey(Term::symbol(":data"))) else {
        panic!("expected :read-buffer :data bytes");
    };
    assert_eq!(
        bytes.as_ref(),
        &[3_u8, 0, 0, 0, 5, 0, 0, 0, 7, 0, 0, 0, 9, 0, 0, 0]
    );

    let _ = crate::runner_gpu_device_backend::call_device_backend(
        "gpu/compute::destroy-resource",
        &term_map(vec![(":id", Term::Str(in_id))]),
    );
    let _ = crate::runner_gpu_device_backend::call_device_backend(
        "gpu/compute::destroy-resource",
        &term_map(vec![(":id", Term::Str(out_id))]),
    );
}

#[cfg(feature = "gpu-device-backend")]
#[test]
fn gfx_gpu_device_runtime_full_submit_interop_is_replayable() {
    let pol = CapsPolicy::from_toml_str(
        r#"
allow = [
  "gfx/gpu::create-buffer",
  "gfx/gpu::write-buffer",
  "gfx/gpu::submit-frame-graph",
  "gfx/gpu::read-buffer"
]

[op."gfx/gpu::create-buffer"]
gpu_backend = "device-runtime-full"
gpu_backend_policy = "require-device"

[op."gfx/gpu::write-buffer"]
gpu_backend = "device-runtime-full"
gpu_backend_policy = "require-device"

[op."gfx/gpu::submit-frame-graph"]
gpu_backend = "device-runtime-full"
gpu_backend_policy = "require-device"
"#,
    )
    .expect("parse caps");
    let term_map = |entries: Vec<(&str, Term)>| -> Term {
        Term::Map(
            entries
                .into_iter()
                .map(|(key, value)| (TermOrdKey(Term::symbol(key)), value))
                .collect(),
        )
    };
    let map_get_string = |term: &Term, key: &str| -> String {
        let Term::Map(map) = term else {
            panic!("expected map response, got {term:?}");
        };
        let Some(Term::Str(id)) = map.get(&TermOrdKey(Term::symbol(key))) else {
            panic!("missing string key `{key}` in response: {term:?}");
        };
        id.clone()
    };

    let create_payload = term_map(vec![(
        ":desc",
        term_map(vec![(":size", Term::Int(16_i64.into()))]),
    )]);
    let create_in = crate::runner_gpu_device_backend::call_device_backend(
        "gfx/gpu::create-buffer",
        &create_payload,
    )
    .expect("create gfx input buffer");
    let create_out = crate::runner_gpu_device_backend::call_device_backend(
        "gfx/gpu::create-buffer",
        &create_payload,
    )
    .expect("create gfx output buffer");
    let in_id = map_get_string(&create_in, ":id");
    let out_id = map_get_string(&create_out, ":id");

    let write_payload = term_map(vec![
        (":id", Term::Str(in_id.clone())),
        (":offset", Term::Int(0_i64.into())),
        (
            ":data",
            Term::Bytes(vec![1_u8, 0, 0, 0, 3, 0, 0, 0, 5, 0, 0, 0, 7, 0, 0, 0].into()),
        ),
    ]);
    crate::runner_gpu_device_backend::call_device_backend("gfx/gpu::write-buffer", &write_payload)
        .expect("seed gfx input buffer");

    let submit_payload =
        format!("{{:graph {{}} :in-buffer-id \"{in_id}\" :out-buffer-id \"{out_id}\" :count 4}}");
    let (forms, h) = mk_prog_for("gfx/gpu::submit-frame-graph", &submit_payload);

    let mut ctx1 = EvalCtx::new();
    let prelude1 = build_prelude(&mut ctx1);
    let mut env1 = prelude1.env;
    let prog1 = eval_module(&mut ctx1, &mut env1, &forms).expect("eval1");
    let run_out = run(&mut ctx1, &pol, prog1, h, "gc_effects-test".to_string()).expect("run");

    let Value::Data(Term::Map(submit)) = &run_out.value else {
        panic!(
            "expected submit map in gfx interop response, got {}",
            run_out.value.debug_repr()
        );
    };
    assert_eq!(
        submit.get(&TermOrdKey(Term::symbol(":kind"))),
        Some(&Term::Str("gfx-frame-submit".to_string()))
    );
    assert_eq!(
        submit.get(&TermOrdKey(Term::symbol(":interop"))),
        Some(&Term::Bool(true))
    );

    let mut ctx2 = EvalCtx::new();
    let prelude2 = build_prelude(&mut ctx2);
    let mut env2 = prelude2.env;
    let prog2 = eval_module(&mut ctx2, &mut env2, &forms).expect("eval2");
    let replay_v = replay(&mut ctx2, prog2, &run_out.log).expect("replay");
    assert_eq!(value_hash(&run_out.value), value_hash(&replay_v));

    let read_out = crate::runner_gpu_device_backend::call_device_backend(
        "gfx/gpu::read-buffer",
        &term_map(vec![
            (":id", Term::Str(out_id.clone())),
            (":offset", Term::Int(0_i64.into())),
            (":size", Term::Int(16_i64.into())),
        ]),
    )
    .expect("read gfx transformed output");
    let Term::Map(read_map) = &read_out else {
        panic!("expected gfx read-buffer map, got {read_out:?}");
    };
    let Some(Term::Bytes(bytes)) = read_map.get(&TermOrdKey(Term::symbol(":data"))) else {
        panic!("expected gfx :read-buffer :data bytes");
    };
    assert_eq!(
        bytes.as_ref(),
        &[3_u8, 0, 0, 0, 7, 0, 0, 0, 11, 0, 0, 0, 15, 0, 0, 0]
    );

    let _ = crate::runner_gpu_device_backend::call_device_backend(
        "gfx/gpu::destroy-resource",
        &term_map(vec![(":id", Term::Str(in_id))]),
    );
    let _ = crate::runner_gpu_device_backend::call_device_backend(
        "gfx/gpu::destroy-resource",
        &term_map(vec![(":id", Term::Str(out_id))]),
    );
}
