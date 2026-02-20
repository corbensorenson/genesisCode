use super::*;

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
        "gfx/gpu::submit-compute-graph",
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
