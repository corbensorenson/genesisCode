use super::*;
use gc_effects::CapsPolicy;
use gc_prelude::selfhost_coreform_toolchain_v1_sources;

fn build_selfhost_artifact_source() -> String {
    let modules = selfhost_coreform_toolchain_v1_sources()
        .expect("load selfhost toolchain sources")
        .iter()
        .map(|(path, src)| {
            let forms = canonicalize_module(parse_module(src).unwrap()).unwrap();
            let h = hash_module(&forms);
            Term::Map(
                [
                    (TermOrdKey(Term::symbol(":path")), Term::Str(path.clone())),
                    (TermOrdKey(Term::symbol(":source")), Term::Str(src.clone())),
                    (
                        TermOrdKey(Term::symbol(":forms")),
                        Term::Vector(forms.clone()),
                    ),
                    (
                        TermOrdKey(Term::symbol(":module-h")),
                        Term::Bytes(h.to_vec().into()),
                    ),
                    (TermOrdKey(Term::symbol(":stage1-ok")), Term::Bool(true)),
                    (
                        TermOrdKey(Term::symbol(":stage2-supported")),
                        Term::Bool(false),
                    ),
                    (TermOrdKey(Term::symbol(":stage2-ok")), Term::Bool(false)),
                ]
                .into_iter()
                .collect(),
            )
        })
        .collect();
    let artifact = Term::Map(
        [
            (
                TermOrdKey(Term::symbol(":kind")),
                Term::Str("genesis/selfhost-toolchain-artifact-v0.2".to_string()),
            ),
            (TermOrdKey(Term::symbol(":v")), Term::Int(1.into())),
            (TermOrdKey(Term::symbol(":ok")), Term::Bool(true)),
            (TermOrdKey(Term::symbol(":modules")), Term::Vector(modules)),
        ]
        .into_iter()
        .collect(),
    );
    print_term(&artifact)
}

fn eval_to_first_step(rt: &mut Runtime, src: &str) -> StepResult {
    rt.ctx = EvalCtx::with_step_limit(rt.step_limit);
    let prelude = build_prelude(&mut rt.ctx);
    rt.env = prelude.env;
    rt.cur = None;
    rt.pending = None;

    let forms = canonicalize_module(parse_module(src).unwrap()).unwrap();
    rt.module_h = Some(hash_module(&forms));
    let v = eval_module(&mut rt.ctx, &mut rt.env, &forms).unwrap();
    rt.cur = Some(v);
    rt.step_internal().unwrap()
}

fn eval_to_first_step_selfhost_with_artifact(
    rt: &mut Runtime,
    src: &str,
    artifact_src: &str,
) -> StepResult {
    rt.ctx = EvalCtx::with_step_limit(None);
    let prelude = build_prelude(&mut rt.ctx);
    rt.env = prelude.env;
    rt.cur = None;
    rt.pending = None;

    bootstrap_selfhost(&mut rt.ctx, &mut rt.env, Some(artifact_src)).unwrap();
    rt.ctx.steps = 0;
    rt.ctx.step_limit = None;
    let forms = selfhost_parse_and_canon_forms(&mut rt.ctx, &rt.env, src).unwrap();
    rt.module_h = Some(hash_module(&forms));
    rt.ctx.steps = 0;
    rt.ctx.step_limit = rt.step_limit;

    let v = eval_module(&mut rt.ctx, &mut rt.env, &forms).unwrap();
    rt.cur = Some(v);
    rt.step_internal().unwrap()
}

#[test]
fn runtime_eval_module_with_gates_matches_baseline_for_pure_scalar_program() {
    let src = r#"
          (def x (prim int/add 20 22))
          x
        "#;
    let mut rt = Runtime::new(0);
    let base = rt.eval_module_internal(src, false, false, false).unwrap();
    let gated = rt.eval_module_internal(src, true, true, true).unwrap();

    match (base, gated) {
        (StepResult::Done { value: a, .. }, StepResult::Done { value: b, .. }) => {
            assert_eq!(a, b);
        }
        (a, b) => panic!("expected done/done parity, got {:?} vs {:?}", a, b),
    }
}

#[cfg(target_arch = "wasm32")]
#[test]
fn runtime_eval_module_with_stage2_gate_rejects_unsupported_non_scalar_result() {
    let src = r#"
          (quote {a 1 b 2})
        "#;
    let mut rt = Runtime::new(0);
    let err = rt
        .eval_module_internal(src, false, false, true)
        .expect_err("stage2 gate must fail closed on unsupported module");
    let s = err.as_string().unwrap_or_default();
    assert!(s.contains("obligation/translation-validation"), "{s}");
}

#[test]
fn runtime_eval_module_selfhost_with_artifact_and_gates_matches_rust_path() {
    let src = r#"
          (def x (prim int/add 1 2))
          (def y (prim int/mul x 7))
          y
        "#;
    let artifact = build_selfhost_artifact_source();

    let mut rt_rust = Runtime::new(0);
    let rust_step = rt_rust.eval_module_internal(src, true, true, true).unwrap();

    let mut rt_selfhost = Runtime::new(0);
    let selfhost_step = rt_selfhost
        .eval_module_selfhost_internal(src, Some(&artifact), true, true, true)
        .unwrap();

    match (rust_step, selfhost_step) {
        (StepResult::Done { value: a, .. }, StepResult::Done { value: b, .. }) => {
            assert_eq!(a, b);
        }
        (a, b) => panic!("expected done/done parity, got {:?} vs {:?}", a, b),
    }
}

#[test]
fn runtime_steps_pure_program_to_done() {
    let mut rt = Runtime::new(0);
    let step = eval_to_first_step(
        &mut rt,
        r#"
              (def m::x 1)
              m::x
            "#,
    );
    let StepResult::Done { value, .. } = step else {
        panic!("expected done, got {:?}", step);
    };
    assert_eq!(value, "1");
}

#[test]
fn runtime_steps_effect_program_and_resumes_with_data() {
    let mut rt = Runtime::new(0);
    let step = eval_to_first_step(
        &mut rt,
        r#"
              (core/effect::perform
                'sys/time::now
                nil
                (fn (t) (core/effect::pure t)))
            "#,
    );
    let StepResult::Effect {
        op,
        payload,
        payload_h,
        cont_h,
        req_h,
        ..
    } = step
    else {
        panic!("expected effect, got {:?}", step);
    };
    assert_eq!(op, "sys/time::now");
    assert_eq!(payload, "nil");

    let pending_k = rt.pending.as_ref().unwrap().k.clone();
    let expected_payload_h = hex::encode(hash_term(&Term::Nil));
    let expected_cont_h = hex::encode(value_hash(&pending_k));
    let expected_req_h = hex::encode(hash_request(
        "sys/time::now",
        hash_term(&Term::Nil),
        value_hash(&pending_k),
    ));
    assert_eq!(payload_h, expected_payload_h);
    assert_eq!(cont_h, expected_cont_h);
    assert_eq!(req_h, expected_req_h);

    let resp = Value::Data(parse_term("123").unwrap());
    let resumed = rt.respond_value_internal(resp).unwrap();
    assert_eq!(
        resumed.resp_h,
        hex::encode(value_hash(&Value::Data(Term::Int(123.into()))))
    );
    match resumed.next {
        StepResult::Done { value, .. } => assert_eq!(value, "123"),
        other => panic!("expected done after resume, got {:?}", other),
    }
}

#[test]
fn runtime_can_resume_with_denied_error() {
    let mut rt = Runtime::new(0);
    let step = eval_to_first_step(
        &mut rt,
        r#"
              (core/effect::perform
                'sys/time::now
                nil
                (fn (t) (core/effect::pure t)))
            "#,
    );
    assert!(matches!(step, StepResult::Effect { .. }));
    let error_tok = rt.ctx.protocol.unwrap().error;
    let resumed = rt
        .respond_value_internal(mk_caps_denied(error_tok, "sys/time::now"))
        .unwrap();
    match resumed.next {
        StepResult::Done { value, .. } => {
            assert!(value.contains(":error/code"));
            assert!(value.contains("core/caps/denied"));
        }
        other => panic!("expected done after denied, got {:?}", other),
    }
}

#[test]
fn wasm_runtime_hashes_match_native_effect_runner_entry() {
    let src = r#"
          (core/effect::perform
            'sys/time::now
            nil
            (fn (t) (core/effect::pure t)))
        "#;

    // WASM-side first step.
    let mut rt = Runtime::new(0);
    let step = eval_to_first_step(&mut rt, src);
    let StepResult::Effect {
        module_h,
        op,
        payload_h,
        cont_h,
        req_h,
        ..
    } = step
    else {
        panic!("expected effect");
    };
    assert_eq!(op, "sys/time::now");

    // Native runner first entry (deny-by-default, so response is deterministic).
    let forms = canonicalize_module(parse_module(src).unwrap()).unwrap();
    let program_hash = hash_module(&forms);
    assert_eq!(module_h, hex::encode(program_hash));
    let mut ctx = EvalCtx::with_step_limit(None);
    let prelude = build_prelude(&mut ctx);
    let mut env = prelude.env;
    let v = eval_module(&mut ctx, &mut env, &forms).unwrap();

    let policy = CapsPolicy::empty(); // deny everything
    let r = gc_effects::run(&mut ctx, &policy, v, program_hash, "test".to_string()).unwrap();
    assert_eq!(r.log.entries.len(), 1);
    let e = &r.log.entries[0];

    assert_eq!(hex::encode(e.payload_h), payload_h);
    assert_eq!(hex::encode(e.cont_h), cont_h);
    assert_eq!(hex::encode(e.req_h), req_h);

    // Now resume in wasm runtime with denied and compare response hash deterministically.
    let error_tok = rt.ctx.protocol.unwrap().error;
    let resumed = rt
        .respond_value_internal(mk_caps_denied(error_tok, "sys/time::now"))
        .unwrap();
    assert_eq!(hex::encode(e.resp_h), resumed.resp_h);

    match resumed.next {
        StepResult::Done { value, .. } => {
            // Deny produces a sealed ERROR; log serialization for ERROR drops the seal and records payload map.
            assert!(value.contains(":error/code"));
            assert!(value.contains("core/caps/denied"));
        }
        other => panic!("expected done after denied, got {:?}", other),
    }
}

#[test]
fn wasm_runtime_selfhost_hashes_match_native_effect_runner_entry() {
    let src = r#"
          (core/effect::perform
            'sys/time::now
            nil
            (fn (t) (core/effect::pure t)))
        "#;

    let artifact = build_selfhost_artifact_source();

    // WASM-side first step through selfhost frontend path.
    let mut rt = Runtime::new(0);
    let step = eval_to_first_step_selfhost_with_artifact(&mut rt, src, &artifact);
    let StepResult::Effect {
        op,
        payload_h,
        cont_h,
        req_h,
        ..
    } = step
    else {
        panic!("expected effect");
    };
    assert_eq!(op, "sys/time::now");

    // Native runner entry for the same selfhost frontend forms.
    let mut ctx = EvalCtx::with_step_limit(None);
    let prelude = build_prelude(&mut ctx);
    let mut env = prelude.env;
    bootstrap_selfhost(&mut ctx, &mut env, Some(&artifact)).unwrap();
    ctx.steps = 0;
    ctx.step_limit = None;
    let forms = selfhost_parse_and_canon_forms(&mut ctx, &env, src).unwrap();
    let program_hash = hash_module(&forms);
    ctx.steps = 0;
    ctx.step_limit = None;
    let v = eval_module(&mut ctx, &mut env, &forms).unwrap();

    let policy = CapsPolicy::empty(); // deny everything
    let r = gc_effects::run(&mut ctx, &policy, v, program_hash, "test".to_string()).unwrap();
    assert_eq!(r.log.entries.len(), 1);
    let e = &r.log.entries[0];

    assert_eq!(hex::encode(e.payload_h), payload_h);
    assert_eq!(hex::encode(e.cont_h), cont_h);
    assert_eq!(hex::encode(e.req_h), req_h);

    // Resume in wasm runtime with denied and compare response hash deterministically.
    let error_tok = rt.ctx.protocol.unwrap().error;
    let resumed = rt
        .respond_value_internal(mk_caps_denied(error_tok, "sys/time::now"))
        .unwrap();
    assert_eq!(hex::encode(e.resp_h), resumed.resp_h);

    match resumed.next {
        StepResult::Done { value, .. } => {
            assert!(value.contains(":error/code"));
            assert!(value.contains("core/caps/denied"));
        }
        other => panic!("expected done after denied, got {:?}", other),
    }
}

#[test]
fn eval_coreform_module_selfhost_matches_rust_frontend_eval() {
    let src = r#"
          (def x 19)
          (def y (prim int/add x 23))
          y
        "#;

    let artifact = build_selfhost_artifact_source();
    let rust = eval_coreform_module(src, 0).expect("rust eval");
    let selfhost =
        eval_coreform_module_selfhost_with_artifact(src, &artifact, 0).expect("selfhost eval");
    assert_eq!(rust, selfhost);
}

#[test]
fn eval_coreform_module_with_gates_matches_baseline_for_pure_scalar_program() {
    let src = r#"
          (def x (prim int/add 20 22))
          x
        "#;

    let baseline = eval_coreform_module(src, 0).expect("baseline eval");
    let gated = eval_coreform_module_with_gates(src, 0, true, true, true).expect("gated eval");
    assert_eq!(baseline, gated);
}

#[cfg(target_arch = "wasm32")]
#[test]
fn eval_coreform_module_with_stage2_gate_rejects_unsupported_non_scalar_result() {
    let src = r#"
          (quote {a 1 b 2})
        "#;
    let err = eval_coreform_module_with_gates(src, 0, false, false, true)
        .expect_err("stage2 gate must fail closed on unsupported module");
    let s = err.as_string().unwrap_or_default();
    assert!(s.contains("obligation/translation-validation"), "{s}");
}

#[cfg(target_arch = "wasm32")]
#[test]
fn eval_coreform_module_with_stage1_gate_reports_obligation_failure() {
    let src = r#"
          (core/effect::perform
            'sys/time::now
            nil
            (fn (t) (core/effect::pure t)))
        "#;

    let err = eval_coreform_module_with_gates(src, 0, false, true, false).expect_err("must fail");
    let s = err.as_string().unwrap_or_default();
    assert!(s.contains("obligation/stage1-validation"), "{s}");
}

#[test]
fn eval_coreform_module_selfhost_with_artifact_and_gates_matches_rust_path() {
    let src = r#"
          (def x (prim int/add 1 2))
          (def y (prim int/mul x 7))
          y
        "#;
    let artifact = build_selfhost_artifact_source();

    let rust = eval_coreform_module_with_gates(src, 0, true, true, true).expect("rust gated");
    let selfhost =
        eval_coreform_module_selfhost_with_artifact_and_gates(src, &artifact, 0, true, true, true)
            .expect("selfhost artifact gated");
    assert_eq!(rust, selfhost);
}

#[test]
fn runtime_eval_module_selfhost_matches_rust_frontend_path() {
    let src = r#"
          (def x 5)
          (def y (prim int/mul x 9))
          y
        "#;
    let artifact = build_selfhost_artifact_source();

    let mut rt_rust = Runtime::new(0);
    let rust_step = eval_to_first_step(&mut rt_rust, src);

    let mut rt_selfhost = Runtime::new(0);
    let selfhost_step = eval_to_first_step_selfhost_with_artifact(&mut rt_selfhost, src, &artifact);

    match (rust_step, selfhost_step) {
        (StepResult::Done { value: a, .. }, StepResult::Done { value: b, .. }) => {
            assert_eq!(a, b);
        }
        (a, b) => panic!("expected done/done parity, got {:?} vs {:?}", a, b),
    }
}

#[test]
fn eval_coreform_module_selfhost_with_artifact_matches_rust_frontend_eval() {
    let src = r#"
          (def x 19)
          (def y (prim int/add x 23))
          y
        "#;
    let artifact = build_selfhost_artifact_source();
    let rust = eval_coreform_module(src, 0).expect("rust eval");
    let selfhost = eval_coreform_module_selfhost_with_artifact(src, &artifact, 0)
        .expect("selfhost artifact eval");
    assert_eq!(rust, selfhost);
}

#[test]
fn runtime_eval_module_selfhost_with_artifact_matches_rust_frontend_path() {
    let src = r#"
          (def x 8)
          (def y (prim int/mul x 11))
          y
        "#;
    let artifact = build_selfhost_artifact_source();

    let mut rt_rust = Runtime::new(0);
    let rust_step = eval_to_first_step(&mut rt_rust, src);

    let mut rt_selfhost = Runtime::new(0);
    let selfhost_step = eval_to_first_step_selfhost_with_artifact(&mut rt_selfhost, src, &artifact);

    match (rust_step, selfhost_step) {
        (StepResult::Done { value: a, .. }, StepResult::Done { value: b, .. }) => {
            assert_eq!(a, b);
        }
        (a, b) => panic!("expected done/done parity, got {:?} vs {:?}", a, b),
    }
}

#[test]
fn gfx_headless_hashes_match_native_renderer_output() {
    let src = r#"
          {
            :type :gfx/frame-graph
            :render-passes [
              {
                :type :gfx/render-pass
                :label "web-golden"
                :commands [
                  {
                    :op :set-pipeline
                    :pipeline 1
                  }
                  {
                    :op :draw
                    :vertex-count 3
                    :instance-count 1
                    :first-vertex 0
                    :first-instance 0
                  }
                ]
              }
            ]
            :compute-passes []
          }
        "#;

    let got = coreform_bridge::gfx_render_frame_graph_headless_hashes_inner(src, 160, 90)
        .expect("compute gfx hashes");
    assert_eq!(got.width, 160);
    assert_eq!(got.height, 90);

    let t = parse_term(src).expect("parse frame graph");
    let img = gc_gfx::render_frame_graph_headless(&t, 160, 90).expect("native render");
    assert_eq!(got.pixel_h, hex::encode(img.pixel_hash));
    assert_eq!(got.png_h, hex::encode(img.png_hash));
}

#[cfg(target_arch = "wasm32")]
#[test]
fn selfhost_artifact_loader_rejects_invalid_artifact_in_wasm_api() {
    let src = "(def x 1)\nx\n";
    let bad_artifact = "(:kind \"bad\")";
    let err = eval_coreform_module_selfhost_with_artifact(src, bad_artifact, 0)
        .expect_err("expected artifact failure");
    let s = err.as_string().unwrap_or_default();
    assert!(s.contains("selfhost/init"), "{s}");
}
