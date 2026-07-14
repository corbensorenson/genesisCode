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
    req.as_ref().clone()
}

fn value_to_data_term(v: &Value) -> Option<Term> {
    v.to_plain_term()
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

#[test]
fn prelude_gfx_descriptor_and_command_builders_match_schema_shapes() {
    let src = r#"
      (def buffer ((((core/gfx/desc::buffer "vb") 1024) 7) false))
      (def texture (((((((core/gfx/desc::texture "albedo") 512) 256) 1) 1) "rgba8unorm") 31))
      (def sampler (((((((core/gfx/desc::sampler "linear") "linear") "linear") "linear") "repeat") "repeat") "repeat"))
      (def shader (((core/gfx/desc::shader-module "vs-main") "artifact:shader/vs-main") "vertex"))
      (def render-pipeline ((((((((core/gfx/desc::render-pipeline "pbr-main") 10) 11) "pos3-nrm-uv") "rgba8unorm") "depth24plus") "back") "ccw"))
      (def compute-pipeline ((core/gfx/desc::compute-pipeline "cull") 12))
      (def draw ((((core/gfx/frame::cmd-draw 3) 1) 0) 0))
      (def set-vb ((((core/gfx/frame::cmd-set-vertex-buffer 0) 1) 0))
      )
      (def dispatch (((core/gfx/frame::cmd-dispatch 8) 8) 1))
      (def pass0 (core/gfx/frame::render-pass-empty "main"))
      (def pass1 ((core/gfx/frame::render-pass-add-color-attachment pass0) 7))
      (def pass2 ((core/gfx/frame::render-pass-add-command pass1) set-vb))
      (def pass3 ((core/gfx/frame::render-pass-add-command pass2) draw))
      (def cpass0 (core/gfx/frame::compute-pass-empty "cull"))
      (def cpass1 ((core/gfx/frame::compute-pass-add-command cpass0) dispatch))
      {
        :buffer buffer
        :texture texture
        :sampler sampler
        :shader shader
        :render-pipeline render-pipeline
        :compute-pipeline compute-pipeline
        :render-pass pass3
        :compute-pass cpass1
      }
    "#;
    let forms = canonicalize_module(parse_module(src).expect("parse")).expect("canonicalize");
    let mut ctx = EvalCtx::new();
    let prelude = build_prelude(&mut ctx);
    let mut env = prelude.env;
    let v = eval_module(&mut ctx, &mut env, &forms).expect("eval");
    let Some(Term::Map(m)) = value_to_data_term(&v) else {
        panic!("expected map");
    };

    let Term::Map(buffer) = m
        .get(&TermOrdKey(Term::symbol(":buffer")))
        .expect(":buffer")
    else {
        panic!("buffer must be map");
    };
    assert_eq!(
        buffer.get(&TermOrdKey(Term::symbol(":type"))),
        Some(&Term::symbol(":gfx/buffer-desc"))
    );

    let Term::Map(rp) = m
        .get(&TermOrdKey(Term::symbol(":render-pipeline")))
        .expect(":render-pipeline")
    else {
        panic!("render pipeline must be map");
    };
    assert_eq!(
        rp.get(&TermOrdKey(Term::symbol(":type"))),
        Some(&Term::symbol(":gfx/render-pipeline-desc"))
    );

    let Term::Map(pass) = m
        .get(&TermOrdKey(Term::symbol(":render-pass")))
        .expect(":render-pass")
    else {
        panic!("render pass must be map");
    };
    let Some(Term::Vector(commands)) = pass.get(&TermOrdKey(Term::symbol(":commands"))) else {
        panic!("render pass commands missing");
    };
    assert_eq!(commands.len(), 2);
    let Term::Map(draw_cmd) = &commands[1] else {
        panic!("draw command must be map");
    };
    assert_eq!(
        draw_cmd.get(&TermOrdKey(Term::symbol(":op"))),
        Some(&Term::symbol(":draw"))
    );

    let Term::Map(cpass) = m
        .get(&TermOrdKey(Term::symbol(":compute-pass")))
        .expect(":compute-pass")
    else {
        panic!("compute pass must be map");
    };
    let Some(Term::Vector(ccommands)) = cpass.get(&TermOrdKey(Term::symbol(":commands"))) else {
        panic!("compute pass commands missing");
    };
    assert_eq!(ccommands.len(), 1);
    let Term::Map(dispatch_cmd) = &ccommands[0] else {
        panic!("dispatch command must be map");
    };
    assert_eq!(
        dispatch_cmd.get(&TermOrdKey(Term::symbol(":op"))),
        Some(&Term::symbol(":dispatch"))
    );
}

#[test]
fn prelude_gfx_2d_3d_and_ui_builders_construct_expected_shapes() {
    let src = r#"
      (def tint ((((core/gfx/math::rgba 1000000) 950000) 900000) 1000000))
      (def sprite
        (((((((((core/gfx/2d::sprite-node "hero")
          "artifact:mesh/quad")
          "artifact:tex/hero")
          0)
          0)
          0)
          1000000)
          1000000)
          tint))
      (def camera (((core/gfx/scene::camera-perspective 60000) 100) 1000000))
      (def light (((core/gfx/scene::light-point (((core/gfx/math::rgb 1000000) 900000) 800000)) 1500) 100000))
      (def cam-node
        (((((((core/gfx/scene::node "camera")
          core/gfx/scene::identity-transform)
          nil)
          nil)
          camera)
          light)
          []))
      (def scene0 (core/gfx/scene::empty "demo"))
      (def scene1 ((core/gfx/scene::add-node scene0) sprite))
      (def scene2 ((core/gfx/scene::add-node scene1) cam-node))
      (def scene3 ((core/gfx/scene::add-root scene2) 1))
      (def button-style
        ((((core/gfx/ui::style {:axis "horizontal"}) {:w 200 :h 40}) {:gap 8})
          {:bg [0 0 0 1000000]}))
      (def button ((((core/gfx/ui::button "btn/start") "Start") button-style) "action/start"))
      (def ui (((core/gfx/ui::vertical "root") 12) [button]))
      {:scene scene3 :ui ui}
    "#;
    let forms = canonicalize_module(parse_module(src).expect("parse")).expect("canonicalize");
    let mut ctx = EvalCtx::new();
    let prelude = build_prelude(&mut ctx);
    let mut env = prelude.env;
    let v = eval_module(&mut ctx, &mut env, &forms).expect("eval");
    let Some(Term::Map(m)) = value_to_data_term(&v) else {
        panic!("expected map");
    };

    let Term::Map(scene) = m.get(&TermOrdKey(Term::symbol(":scene"))).expect(":scene") else {
        panic!("scene must be map");
    };
    assert_eq!(
        scene.get(&TermOrdKey(Term::symbol(":type"))),
        Some(&Term::symbol(":gfx/scene"))
    );
    let Some(Term::Vector(roots)) = scene.get(&TermOrdKey(Term::symbol(":root-nodes"))) else {
        panic!("scene root-nodes missing");
    };
    assert_eq!(roots, &vec![Term::Int(1.into())]);
    let Some(Term::Vector(nodes)) = scene.get(&TermOrdKey(Term::symbol(":nodes"))) else {
        panic!("scene nodes missing");
    };
    assert_eq!(nodes.len(), 2);
    let Term::Map(sprite_node) = &nodes[0] else {
        panic!("sprite node must be map");
    };
    let Term::Map(material) = sprite_node
        .get(&TermOrdKey(Term::symbol(":material")))
        .expect("sprite material")
    else {
        panic!("sprite material must be map");
    };
    assert_eq!(
        material.get(&TermOrdKey(Term::symbol(":type"))),
        Some(&Term::symbol(":gfx/material-pbr"))
    );

    let Term::Map(ui) = m.get(&TermOrdKey(Term::symbol(":ui"))).expect(":ui") else {
        panic!("ui must be map");
    };
    assert_eq!(
        ui.get(&TermOrdKey(Term::symbol(":type"))),
        Some(&Term::symbol(":gfx/ui-node"))
    );
    assert_eq!(
        ui.get(&TermOrdKey(Term::symbol(":kind"))),
        Some(&Term::Str("container".to_string()))
    );
    let Some(Term::Vector(ui_children)) = ui.get(&TermOrdKey(Term::symbol(":children"))) else {
        panic!("ui children missing");
    };
    assert_eq!(ui_children.len(), 1);
}

#[path = "prelude_gfx_builders/runtime_planners.rs"]
mod prelude_gfx_builders_runtime_planners;
