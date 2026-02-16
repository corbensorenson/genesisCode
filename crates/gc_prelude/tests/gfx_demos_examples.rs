use std::path::PathBuf;

use gc_coreform::{Term, TermOrdKey, canonicalize_module, parse_module};
use gc_kernel::{EvalCtx, Value, eval_module};
use gc_prelude::build_prelude;

fn value_to_data_term(v: &Value) -> Option<Term> {
    match v {
        Value::Data(t) => Some(t.clone()),
        Value::Vector(xs) => Some(Term::Vector(
            xs.iter()
                .map(value_to_data_term)
                .collect::<Option<Vec<_>>>()?,
        )),
        Value::Map(m) => {
            let mut out = std::collections::BTreeMap::new();
            for (k, vv) in m {
                out.insert(k.clone(), value_to_data_term(vv)?);
            }
            Some(Term::Map(out))
        }
        _ => None,
    }
}

fn demo_path(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../examples/gfx_demos")
        .join(name)
}

fn eval_demo(name: &str) -> Term {
    let src = std::fs::read_to_string(demo_path(name)).expect("read demo");
    let forms = canonicalize_module(parse_module(&src).expect("parse")).expect("canonicalize");
    let mut ctx = EvalCtx::new();
    let prelude = build_prelude(&mut ctx);
    let mut env = prelude.env;
    let v = eval_module(&mut ctx, &mut env, &forms).expect("eval");
    value_to_data_term(&v).expect("result must be data")
}

#[test]
fn ui_app_demo_plans_deterministic_batched_ui_frame() {
    let out_a = eval_demo("ui_app.gc");
    let out_b = eval_demo("ui_app.gc");
    let Term::Map(m_a) = out_a else {
        panic!("demo output must be map");
    };
    let Term::Map(m_b) = out_b else {
        panic!("demo output must be map");
    };

    assert_eq!(
        m_a.get(&TermOrdKey(Term::symbol(":demo"))),
        Some(&Term::Str("gfx/ui-app".to_string()))
    );
    assert_eq!(
        m_a.get(&TermOrdKey(Term::symbol(":draw-count"))),
        Some(&Term::Int(4.into()))
    );
    assert_eq!(
        m_a.get(&TermOrdKey(Term::symbol(":batch-count"))),
        Some(&Term::Int(4.into()))
    );
    assert_eq!(
        m_a.get(&TermOrdKey(Term::symbol(":frame-hash"))),
        m_b.get(&TermOrdKey(Term::symbol(":frame-hash"))),
        "ui_app frame hash should be deterministic"
    );
}

#[test]
fn scene3d_demo_plans_pbr_frame_with_shadow_metadata() {
    let out = eval_demo("scene3d.gc");
    let Term::Map(m) = out else {
        panic!("demo output must be map");
    };
    assert_eq!(
        m.get(&TermOrdKey(Term::symbol(":demo"))),
        Some(&Term::Str("gfx/scene3d".to_string()))
    );
    assert_eq!(
        m.get(&TermOrdKey(Term::symbol(":render-pass-count"))),
        Some(&Term::Int(2.into()))
    );
    assert_eq!(
        m.get(&TermOrdKey(Term::symbol(":light-count"))),
        Some(&Term::Int(2.into()))
    );
    assert_eq!(
        m.get(&TermOrdKey(Term::symbol(":shadow-light-count"))),
        Some(&Term::Int(1.into()))
    );
}

#[test]
fn hybrid_web_demo_merges_3d_and_ui_passes() {
    let out = eval_demo("hybrid_web.gc");
    let Term::Map(m) = out else {
        panic!("demo output must be map");
    };
    assert_eq!(
        m.get(&TermOrdKey(Term::symbol(":demo"))),
        Some(&Term::Str("gfx/hybrid-web".to_string()))
    );
    assert_eq!(
        m.get(&TermOrdKey(Term::symbol(":render-pass-count"))),
        Some(&Term::Int(3.into()))
    );
}
