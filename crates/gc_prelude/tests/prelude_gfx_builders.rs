use gc_coreform::{Term, TermOrdKey, canonicalize_module, parse_module};
use gc_kernel::{EffectProgram, EffectRequest, EvalCtx, Value, eval_module};
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

#[test]
fn prelude_gfx_builders_construct_frame_graph_and_submit_request() {
    let src = r#"
      (def pass
        ((((core/gfx/frame::render-pass "main") [1]) 2)
          [{:op :draw :vertex-count 3 :instance-count 1 :first-vertex 0 :first-instance 0}]))
      (def graph ((core/gfx/frame::add-render-pass core/gfx/frame::empty) pass))
      {
        :graph graph
        :submit (core/gfx/frame::submit graph)
      }
    "#;
    let forms = canonicalize_module(parse_module(src).expect("parse")).expect("canonicalize");
    let mut ctx = EvalCtx::new();
    let prelude = build_prelude(&mut ctx);
    let mut env = prelude.env;
    let v = eval_module(&mut ctx, &mut env, &forms).expect("eval");
    let Value::Map(m) = v else {
        panic!("expected map, got {}", v.debug_repr());
    };

    let graph_v = m
        .get(&TermOrdKey(Term::symbol(":graph")))
        .expect("missing :graph");
    let Some(Term::Map(graph)) = value_to_data_term(graph_v) else {
        panic!("expected graph map");
    };
    assert_eq!(
        graph.get(&TermOrdKey(Term::symbol(":type"))),
        Some(&Term::symbol(":gfx/frame-graph"))
    );
    let Some(Term::Vector(render_passes)) = graph.get(&TermOrdKey(Term::symbol(":render-passes")))
    else {
        panic!("missing render passes");
    };
    assert_eq!(render_passes.len(), 1);
    let Term::Map(pass) = &render_passes[0] else {
        panic!("pass must be map");
    };
    assert_eq!(
        pass.get(&TermOrdKey(Term::symbol(":type"))),
        Some(&Term::symbol(":gfx/render-pass"))
    );

    let submit_v = m
        .get(&TermOrdKey(Term::symbol(":submit")))
        .expect("missing :submit")
        .clone();
    let req = get_req(submit_v);
    assert_eq!(req.op, "gfx/gpu::submit-frame-graph");
    let Term::Map(payload) = req.payload else {
        panic!("submit payload should be map");
    };
    assert!(payload.contains_key(&TermOrdKey(Term::symbol(":graph"))));
}

#[test]
fn prelude_gfx_scene_builders_construct_scene_shape() {
    let src = r#"
      (def node
        (((((((core/gfx/scene::node "root")
          core/gfx/scene::identity-transform)
          nil)
          nil)
          {:kind "perspective" :fov-milli-deg 60000 :near-mm 100 :far-mm 1000000})
          nil)
          []))
      (def scene0 (core/gfx/scene::empty "demo"))
      (def scene1 ((core/gfx/scene::add-node scene0) node))
      (def scene2 ((core/gfx/scene::set-roots scene1) [0]))
      scene2
    "#;
    let forms = canonicalize_module(parse_module(src).expect("parse")).expect("canonicalize");
    let mut ctx = EvalCtx::new();
    let prelude = build_prelude(&mut ctx);
    let mut env = prelude.env;
    let v = eval_module(&mut ctx, &mut env, &forms).expect("eval");
    let Some(Term::Map(scene)) = value_to_data_term(&v) else {
        panic!("expected scene map");
    };
    assert_eq!(
        scene.get(&TermOrdKey(Term::symbol(":type"))),
        Some(&Term::symbol(":gfx/scene"))
    );
    let Some(Term::Vector(nodes)) = scene.get(&TermOrdKey(Term::symbol(":nodes"))) else {
        panic!("missing :nodes");
    };
    assert_eq!(nodes.len(), 1);
    let Term::Map(node) = &nodes[0] else {
        panic!("node must be map");
    };
    assert_eq!(
        node.get(&TermOrdKey(Term::symbol(":name"))),
        Some(&Term::Str("root".to_string()))
    );
    let Some(Term::Vector(roots)) = scene.get(&TermOrdKey(Term::symbol(":root-nodes"))) else {
        panic!("missing :root-nodes");
    };
    assert_eq!(roots, &vec![Term::Int(0.into())]);
}
