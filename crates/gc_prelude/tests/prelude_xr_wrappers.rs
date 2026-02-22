use gc_coreform::{Term, TermOrdKey, canonicalize_module, parse_module};
use gc_kernel::{EffectProgram, EffectRequest, Env, EvalCtx, Value, eval_module};
use gc_prelude::build_prelude;

fn get_req(v: Value) -> EffectRequest {
    let Value::EffectProgram(p) = v else {
        panic!("expected effect program, got {}", v.debug_repr());
    };
    let EffectProgram::Perform { request } = p.as_ref() else {
        panic!("expected perform");
    };
    let Value::Sealed { payload, .. } = request.as_ref() else {
        panic!("expected sealed request");
    };
    let Value::EffectRequest(req) = payload.as_ref() else {
        panic!("expected effect request payload");
    };
    req.clone()
}

fn assert_no_prelude_bootstrap_error(env: &Env) {
    if let Some(err) = env.get("core/prelude::bootstrap-error") {
        panic!("prelude bootstrap error: {}", err.debug_repr());
    }
}

#[test]
fn prelude_xr_wrappers_construct_expected_requests() {
    let src = r#"
      {
        :open (core/gfx/xr::session-open {:mode "immersive-vr" :reference-space "local-floor" :app "xr-agent"})
        :frame (core/gfx/xr::frame-poll "session-1")
        :input ((core/gfx/xr::input-poll "session-1") 4)
        :haptics ((((core/gfx/xr::haptics-pulse "session-1") "right-controller") 800) 24)
        :submit ((core/gfx/xr::submit-frame "session-1") {:frame-index 3 :views []})
        :close (core/gfx/xr::session-close "session-1")
      }
    "#;
    let forms = canonicalize_module(parse_module(src).unwrap()).unwrap();

    let mut ctx = EvalCtx::new();
    let prelude = build_prelude(&mut ctx);
    let mut env = prelude.env;
    assert_no_prelude_bootstrap_error(&env);
    let v = eval_module(&mut ctx, &mut env, &forms).unwrap();

    let Value::Map(m) = v else {
        panic!("expected map, got {}", v.debug_repr());
    };

    let open_req = get_req(
        m.get(&TermOrdKey(Term::symbol(":open")))
            .expect("missing :open")
            .clone(),
    );
    assert_eq!(open_req.op, "gfx/xr::session-open");

    let frame_req = get_req(
        m.get(&TermOrdKey(Term::symbol(":frame")))
            .expect("missing :frame")
            .clone(),
    );
    assert_eq!(frame_req.op, "gfx/xr::frame-poll");

    let input_req = get_req(
        m.get(&TermOrdKey(Term::symbol(":input")))
            .expect("missing :input")
            .clone(),
    );
    assert_eq!(input_req.op, "gfx/xr::input-poll");

    let submit_req = get_req(
        m.get(&TermOrdKey(Term::symbol(":submit")))
            .expect("missing :submit")
            .clone(),
    );
    assert_eq!(submit_req.op, "gfx/xr::submit-frame");

    let haptics_req = get_req(
        m.get(&TermOrdKey(Term::symbol(":haptics")))
            .expect("missing :haptics")
            .clone(),
    );
    assert_eq!(haptics_req.op, "gfx/xr::haptics-pulse");

    let close_req = get_req(
        m.get(&TermOrdKey(Term::symbol(":close")))
            .expect("missing :close")
            .clone(),
    );
    assert_eq!(close_req.op, "gfx/xr::session-close");
}

#[test]
fn prelude_xr_domain_kit_starts_with_session_open() {
    let src = r#"
      ((core/kit/xr::run-single-frame-cycle
         (((core/kit/xr::session-spec-v1 "immersive-vr") "local-floor") "kit-agent"))
        2)
    "#;
    let forms = canonicalize_module(parse_module(src).unwrap()).unwrap();

    let mut ctx = EvalCtx::new();
    let prelude = build_prelude(&mut ctx);
    let mut env = prelude.env;
    assert_no_prelude_bootstrap_error(&env);
    let v = eval_module(&mut ctx, &mut env, &forms).unwrap();
    let req = get_req(v);
    assert_eq!(req.op, "gfx/xr::session-open");
}
