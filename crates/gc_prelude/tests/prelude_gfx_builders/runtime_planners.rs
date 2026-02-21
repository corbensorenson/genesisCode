use super::*;

#[test]
fn prelude_gfx_runtime_plan_frame_2d_is_deterministic_and_renderable_aware() {
    let src = r#"
      (def mat (((((core/gfx/scene::material-pbr [1000000 1000000 1000000 1000000]) nil) nil) nil) nil))
      (def sprite
        (((((core/gfx/scene::node-basic "sprite")
          core/gfx/scene::identity-transform)
          (core/gfx/scene::mesh-ref "artifact:mesh/quad"))
          mat)
          []))
      (def cam-node
        (((((((core/gfx/scene::node "camera")
          core/gfx/scene::identity-transform)
          nil)
          nil)
          (((core/gfx/scene::camera-perspective 60000) 100) 1000000))
          nil)
          []))
      (def scene0 (core/gfx/scene::empty "demo"))
      (def scene1 ((core/gfx/scene::add-node scene0) sprite))
      (def scene2 ((core/gfx/scene::add-node scene1) cam-node))
      (def frame-a (((core/gfx/runtime::plan-frame-2d scene2) "scene-pass") 101))
      (def frame-b (((core/gfx/runtime::plan-frame-2d scene2) "scene-pass") 101))
      {
        :frame frame-a
        :hash-a (core/gfx/runtime::hash-frame-graph frame-a)
        :hash-b (core/gfx/runtime::hash-frame-graph frame-b)
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

    let hash_a = m
        .get(&TermOrdKey(Term::symbol(":hash-a")))
        .expect(":hash-a");
    let hash_b = m
        .get(&TermOrdKey(Term::symbol(":hash-b")))
        .expect(":hash-b");
    assert_eq!(hash_a, hash_b, "planned frame hash should be deterministic");

    let Term::Map(frame) = m.get(&TermOrdKey(Term::symbol(":frame"))).expect(":frame") else {
        panic!("frame must be map");
    };
    let Some(Term::Vector(rpasses)) = frame.get(&TermOrdKey(Term::symbol(":render-passes"))) else {
        panic!("render passes missing");
    };
    assert_eq!(rpasses.len(), 1);
    let Term::Map(pass) = &rpasses[0] else {
        panic!("render pass must be map");
    };
    let Some(Term::Vector(commands)) = pass.get(&TermOrdKey(Term::symbol(":commands"))) else {
        panic!("commands missing");
    };
    // One set-pipeline command + one draw (camera-only node is not renderable).
    assert_eq!(commands.len(), 2);
    let Term::Map(draw_cmd) = &commands[1] else {
        panic!("draw command must be map");
    };
    assert_eq!(
        draw_cmd.get(&TermOrdKey(Term::symbol(":op"))),
        Some(&Term::symbol(":draw"))
    );
}

#[test]
fn prelude_gfx_runtime_plan_frame_2d_ui_adds_ui_pass_with_node_count_draws() {
    let src = r#"
      (def ui-style ((((core/gfx/ui::style {:axis "vertical"}) {:w nil :h nil}) {:gap 8}) {:bg [0 0 0 1000000]}))
      (def title (((core/gfx/ui::text "txt/title") "GenesisCode") ui-style))
      (def button ((((core/gfx/ui::button "btn/run") "Run") ui-style) "action/run"))
      (def ui-root (((core/gfx/ui::container "root") ui-style) [title button]))
      (def scene (core/gfx/scene::empty "demo"))
      (def frame ((((((core/gfx/runtime::plan-frame-2d+ui scene ui-root) "scene-pass") 7) "ui-pass") 9)))
      {
        :ui-count (core/gfx/runtime::ui-node-count ui-root)
        :frame frame
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

    assert_eq!(
        m.get(&TermOrdKey(Term::symbol(":ui-count"))),
        Some(&Term::Int(3.into()))
    );

    let Term::Map(frame) = m.get(&TermOrdKey(Term::symbol(":frame"))).expect(":frame") else {
        panic!("frame must be map");
    };
    let Some(Term::Vector(rpasses)) = frame.get(&TermOrdKey(Term::symbol(":render-passes"))) else {
        panic!("render passes missing");
    };
    assert_eq!(rpasses.len(), 2, "scene pass + ui pass");

    let Term::Map(ui_pass) = &rpasses[1] else {
        panic!("ui pass must be map");
    };
    let Some(Term::Vector(ui_commands)) = ui_pass.get(&TermOrdKey(Term::symbol(":commands")))
    else {
        panic!("ui commands missing");
    };
    // one set-pipeline + ui-node-count draws (3 nodes: root + title + button).
    assert_eq!(ui_commands.len(), 4);
    let Term::Map(last_cmd) = &ui_commands[3] else {
        panic!("ui draw command must be map");
    };
    assert_eq!(
        last_cmd.get(&TermOrdKey(Term::symbol(":op"))),
        Some(&Term::symbol(":draw"))
    );
}

#[test]
fn prelude_gfx_runtime_plan_frame_3d_uses_attachments_and_renderable_filtering() {
    let src = r#"
      (def mat (((((core/gfx/scene::material-pbr [1000000 1000000 1000000 1000000]) nil) nil) nil) nil))
      (def mesh-node
        (((((core/gfx/scene::node-basic "mesh")
          core/gfx/scene::identity-transform)
          (core/gfx/scene::mesh-ref "artifact:mesh/cube"))
          mat)
          []))
      (def light-node
        (((((((core/gfx/scene::node "light")
          core/gfx/scene::identity-transform)
          nil)
          nil)
          nil)
          (((core/gfx/scene::light-point (((core/gfx/math::rgb 1000000) 1000000) 1000000)) 500) 50000))
          []))
      (def scene0 (core/gfx/scene::empty "demo-3d"))
      (def scene1 ((core/gfx/scene::add-node scene0) mesh-node))
      (def scene2 ((core/gfx/scene::add-node scene1) light-node))
      (def color-att ((((core/gfx/frame::color-attachment "swapchain-view") [0 0 0 1000000]) "clear") "store"))
      (def depth-att (((((core/gfx/frame::depth-attachment "depth-view") 1000000) 0) "clear") "store"))
      (def color-atts ((core/vec::push []) color-att))
      (def frame (((((core/gfx/runtime::plan-frame-3d scene2) "main-3d") 55) color-atts) depth-att))
      frame
    "#;
    let forms = canonicalize_module(parse_module(src).expect("parse")).expect("canonicalize");
    let mut ctx = EvalCtx::new();
    let prelude = build_prelude(&mut ctx);
    let mut env = prelude.env;
    let v = eval_module(&mut ctx, &mut env, &forms).expect("eval");
    let Some(Term::Map(frame)) = value_to_data_term(&v) else {
        panic!("frame must be map");
    };
    let Some(Term::Vector(rpasses)) = frame.get(&TermOrdKey(Term::symbol(":render-passes"))) else {
        panic!("render passes missing");
    };
    assert_eq!(rpasses.len(), 1);
    let Term::Map(pass) = &rpasses[0] else {
        panic!("render pass must be map");
    };

    let Some(Term::Vector(color_attachments)) =
        pass.get(&TermOrdKey(Term::symbol(":color-attachments")))
    else {
        panic!("color attachments missing");
    };
    assert_eq!(color_attachments.len(), 1);
    let Term::Map(color_att) = &color_attachments[0] else {
        panic!("color attachment must be map");
    };
    assert_eq!(
        color_att.get(&TermOrdKey(Term::symbol(":type"))),
        Some(&Term::symbol(":gfx/color-attachment"))
    );

    let Some(Term::Map(depth_att)) = pass.get(&TermOrdKey(Term::symbol(":depth-attachment")))
    else {
        panic!("depth attachment missing");
    };
    assert_eq!(
        depth_att.get(&TermOrdKey(Term::symbol(":type"))),
        Some(&Term::symbol(":gfx/depth-attachment"))
    );

    let Some(Term::Vector(commands)) = pass.get(&TermOrdKey(Term::symbol(":commands"))) else {
        panic!("commands missing");
    };
    // set-pipeline + one draw (light node is not renderable).
    assert_eq!(commands.len(), 2);
    let Term::Map(draw_cmd) = &commands[1] else {
        panic!("draw command must be map");
    };
    assert_eq!(
        draw_cmd.get(&TermOrdKey(Term::symbol(":op"))),
        Some(&Term::symbol(":draw"))
    );
}

#[test]
fn prelude_gfx_runtime_plan_frame_2d_batched_groups_sprite_text_rect_draws() {
    let src = r#"
      (def white [1000000 1000000 1000000 1000000])
      (def blue [200000 300000 1000000 1000000])
      (def sprite-a1 (((((((core/gfx/2d::draw-sprite "artifact:tex/a") white) 0) 0) 0) 1000000) 1000000))
      (def sprite-a2 (((((((core/gfx/2d::draw-sprite "artifact:tex/a") white) 1000000) 0) 0) 1000000) 1000000))
      (def text-1 (((((((core/gfx/2d::draw-text "artifact:font/ui") "Genesis") white) 0) 0) 0) 16000))
      (def rect-1 ((((((core/gfx/2d::draw-rect blue) 0) 1000000) 0) 2000000) 500000))
      (def rect-2 ((((((core/gfx/2d::draw-rect blue) 2000000) 1000000) 0) 1000000) 500000))
      (def draws0 [])
      (def draws1 ((core/vec::push draws0) sprite-a1))
      (def draws2 ((core/vec::push draws1) sprite-a2))
      (def draws3 ((core/vec::push draws2) text-1))
      (def draws4 ((core/vec::push draws3) rect-1))
      (def draws5 ((core/vec::push draws4) rect-2))
      (def scene0 (core/gfx/2d::scene-empty "ui-demo"))
      (def scene1 ((core/gfx/2d::scene-add-draw scene0) sprite-a1))
      (def scene2 ((core/gfx/2d::scene-add-draw scene1) sprite-a2))
      (def scene3 ((core/gfx/2d::scene-add-draw scene2) text-1))
      (def scene4 ((core/gfx/2d::scene-add-draw scene3) rect-1))
      (def scene5 ((core/gfx/2d::scene-add-draw scene4) rect-2))
      (def batches (core/gfx/runtime::2d-batches draws5))
      (def frame-a (((core/gfx/runtime::plan-frame-2d-batched draws5) "ui-pass") 501))
      (def frame-b (((core/gfx/runtime::plan-frame-2d-scene-batched scene5) "ui-pass") 501))
      {
        :batches batches
        :frame-a frame-a
        :frame-b frame-b
        :hash-a (core/gfx/runtime::hash-frame-graph frame-a)
        :hash-b (core/gfx/runtime::hash-frame-graph frame-b)
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

    assert_eq!(
        m.get(&TermOrdKey(Term::symbol(":hash-a"))),
        m.get(&TermOrdKey(Term::symbol(":hash-b"))),
        "scene-batched and vector-batched planners should match"
    );

    let Term::Vector(batches) = m
        .get(&TermOrdKey(Term::symbol(":batches")))
        .expect(":batches")
    else {
        panic!("batches must be vector");
    };
    assert_eq!(batches.len(), 3, "sprite run, text run, rect run");

    let Term::Map(batch0) = &batches[0] else {
        panic!("batch0 must be map");
    };
    assert_eq!(
        batch0.get(&TermOrdKey(Term::symbol(":count"))),
        Some(&Term::Int(2.into()))
    );
    let Term::Map(batch1) = &batches[1] else {
        panic!("batch1 must be map");
    };
    assert_eq!(
        batch1.get(&TermOrdKey(Term::symbol(":count"))),
        Some(&Term::Int(1.into()))
    );
    let Term::Map(batch2) = &batches[2] else {
        panic!("batch2 must be map");
    };
    assert_eq!(
        batch2.get(&TermOrdKey(Term::symbol(":count"))),
        Some(&Term::Int(2.into()))
    );

    let Term::Map(frame) = m
        .get(&TermOrdKey(Term::symbol(":frame-a")))
        .expect(":frame-a")
    else {
        panic!("frame-a must be map");
    };
    let Some(Term::Vector(rpasses)) = frame.get(&TermOrdKey(Term::symbol(":render-passes"))) else {
        panic!("render passes missing");
    };
    assert_eq!(rpasses.len(), 1);
    let Term::Map(pass) = &rpasses[0] else {
        panic!("render pass must be map");
    };
    let Some(Term::Vector(commands)) = pass.get(&TermOrdKey(Term::symbol(":commands"))) else {
        panic!("commands missing");
    };
    // set-pipeline + one draw per batch
    assert_eq!(commands.len(), 4);

    let Term::Map(cmd1) = &commands[1] else {
        panic!("cmd1 must be map");
    };
    assert_eq!(
        cmd1.get(&TermOrdKey(Term::symbol(":instance-count"))),
        Some(&Term::Int(2.into()))
    );
    let Term::Map(cmd2) = &commands[2] else {
        panic!("cmd2 must be map");
    };
    assert_eq!(
        cmd2.get(&TermOrdKey(Term::symbol(":instance-count"))),
        Some(&Term::Int(1.into()))
    );
    let Term::Map(cmd3) = &commands[3] else {
        panic!("cmd3 must be map");
    };
    assert_eq!(
        cmd3.get(&TermOrdKey(Term::symbol(":instance-count"))),
        Some(&Term::Int(2.into()))
    );
}

#[test]
fn prelude_gfx_runtime_plan_frame_3d_pbr_emits_shadow_passes_and_metadata() {
    let src = r#"
      (def mat (((((core/gfx/scene::material-pbr [1000000 1000000 1000000 1000000]) nil) nil) nil) nil))
      (def mesh-node
        (((((core/gfx/scene::node-basic "mesh")
          core/gfx/scene::identity-transform)
          (core/gfx/scene::mesh-ref "artifact:mesh/cube"))
          mat)
          []))
      (def camera (((core/gfx/scene::camera-perspective 60000) 100) 1000000))
      (def cam-node
        (((((((core/gfx/scene::node "camera")
          core/gfx/scene::identity-transform)
          nil)
          nil)
          camera)
          nil)
          []))
      (def light-a0 ((core/gfx/scene::light-directional (((core/gfx/math::rgb 1000000) 950000) 900000)) 1200))
      (def light-a (((core/map::put light-a0) (quote :casts-shadow)) true))
      (def light-b0 (((core/gfx/scene::light-point (((core/gfx/math::rgb 800000) 850000) 900000)) 900) 50000))
      (def light-b (((core/map::put light-b0) (quote :casts-shadow)) false))
      (def light-a-node
        (((((((core/gfx/scene::node "light-a")
          core/gfx/scene::identity-transform)
          nil)
          nil)
          nil)
          light-a)
          []))
      (def light-b-node
        (((((((core/gfx/scene::node "light-b")
          core/gfx/scene::identity-transform)
          nil)
          nil)
          nil)
          light-b)
          []))
      (def scene0 (core/gfx/scene::empty "demo-3d-pbr"))
      (def scene1 ((core/gfx/scene::add-node scene0) mesh-node))
      (def scene2 ((core/gfx/scene::add-node scene1) cam-node))
      (def scene3 ((core/gfx/scene::add-node scene2) light-a-node))
      (def scene4 ((core/gfx/scene::add-node scene3) light-b-node))
      (def color-att ((((core/gfx/frame::color-attachment "swapchain-view") [0 0 0 1000000]) "clear") "store"))
      (def color-atts ((core/vec::push []) color-att))
      (def depth-main (((((core/gfx/frame::depth-attachment "main-depth") 1000000) 0) "clear") "store"))
      (def frame
        ((((((((core/gfx/runtime::plan-frame-3d-pbr scene4)
          "main")
          301)
          color-atts)
          depth-main)
          "shadow-")
          302)
          "shadow-depth-"))
      frame
    "#;
    let forms = canonicalize_module(parse_module(src).expect("parse")).expect("canonicalize");
    let mut ctx = EvalCtx::new();
    let prelude = build_prelude(&mut ctx);
    let mut env = prelude.env;
    let v = eval_module(&mut ctx, &mut env, &forms).expect("eval");
    let Some(Term::Map(frame)) = value_to_data_term(&v) else {
        panic!("frame must be map");
    };

    let Some(Term::Map(meta)) = frame.get(&TermOrdKey(Term::symbol(":meta"))) else {
        panic!("meta missing");
    };
    assert_eq!(
        meta.get(&TermOrdKey(Term::symbol(":light-count"))),
        Some(&Term::Int(2.into()))
    );
    assert_eq!(
        meta.get(&TermOrdKey(Term::symbol(":shadow-light-count"))),
        Some(&Term::Int(1.into()))
    );
    let Some(Term::Map(camera)) = meta.get(&TermOrdKey(Term::symbol(":camera"))) else {
        panic!("camera missing");
    };
    assert_eq!(
        camera.get(&TermOrdKey(Term::symbol(":kind"))),
        Some(&Term::Str("perspective".to_string()))
    );

    let Some(Term::Vector(rpasses)) = frame.get(&TermOrdKey(Term::symbol(":render-passes"))) else {
        panic!("render passes missing");
    };
    assert_eq!(rpasses.len(), 2, "main + one shadow pass");

    let Term::Map(main_pass) = &rpasses[0] else {
        panic!("main pass must be map");
    };
    let Some(Term::Vector(main_colors)) =
        main_pass.get(&TermOrdKey(Term::symbol(":color-attachments")))
    else {
        panic!("main color attachments missing");
    };
    assert_eq!(main_colors.len(), 1);
    let Some(Term::Map(_main_depth)) =
        main_pass.get(&TermOrdKey(Term::symbol(":depth-attachment")))
    else {
        panic!("main depth attachment missing");
    };

    let Term::Map(shadow_pass) = &rpasses[1] else {
        panic!("shadow pass must be map");
    };
    let Some(Term::Str(shadow_label)) = shadow_pass.get(&TermOrdKey(Term::symbol(":label"))) else {
        panic!("shadow label missing");
    };
    assert_eq!(shadow_label, "shadow-0");
    let Some(Term::Vector(shadow_colors)) =
        shadow_pass.get(&TermOrdKey(Term::symbol(":color-attachments")))
    else {
        panic!("shadow color attachments missing");
    };
    assert_eq!(shadow_colors.len(), 0, "shadow pass should be depth-only");
    let Some(Term::Map(shadow_depth)) =
        shadow_pass.get(&TermOrdKey(Term::symbol(":depth-attachment")))
    else {
        panic!("shadow depth attachment missing");
    };
    assert_eq!(
        shadow_depth.get(&TermOrdKey(Term::symbol(":view"))),
        Some(&Term::Str("shadow-depth-0".to_string()))
    );
}

#[test]
fn prelude_gfx_ui_runtime_projects_to_2d_and_plans_batched_frame() {
    let src = r#"
      (def root-style ((((core/gfx/ui::style {:axis "vertical"}) {:w 800000 :h 600000}) {:gap 12000}) {:bg [10000 10000 10000 1000000]}))
      (def text-style ((((core/gfx/ui::style {:axis "vertical"}) {:w 760000 :h 40000}) {:gap 0}) {}))
      (def button-style ((((core/gfx/ui::style {:axis "horizontal"}) {:w 760000 :h 56000}) {:gap 0}) {:bg [300000 350000 1000000 1000000]}))
      (def title (((core/gfx/ui::text "title") "GenesisCode Editor") text-style))
      (def button ((((core/gfx/ui::button "run") "Run") button-style) "action/run"))
      (def children0 [])
      (def children1 ((core/vec::push children0) title))
      (def children2 ((core/vec::push children1) button))
      (def root (((core/gfx/ui::container "root") root-style) children2))
      (def draws (((core/gfx/ui/runtime::to-2d-draws root) 800000) 600000))
      (def scene (((core/gfx/ui/runtime::to-2d-scene root) 800000) 600000))
      (def batches (core/gfx/runtime::2d-batches draws))
      (def frame-a (((((core/gfx/ui/runtime::plan-frame-batched root) 800000) 600000) "ui-pass") 777))
      (def frame-b (((((core/gfx/ui/runtime::plan-frame-batched root) 800000) 600000) "ui-pass") 777))
      {
        :draws draws
        :scene scene
        :batches batches
        :frame frame-a
        :hash-a (core/gfx/runtime::hash-frame-graph frame-a)
        :hash-b (core/gfx/runtime::hash-frame-graph frame-b)
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

    assert_eq!(
        m.get(&TermOrdKey(Term::symbol(":hash-a"))),
        m.get(&TermOrdKey(Term::symbol(":hash-b"))),
        "UI frame planning should be deterministic"
    );

    let Term::Map(scene) = m.get(&TermOrdKey(Term::symbol(":scene"))).expect(":scene") else {
        panic!("scene must be map");
    };
    assert_eq!(
        scene.get(&TermOrdKey(Term::symbol(":type"))),
        Some(&Term::symbol(":gfx/2d-scene"))
    );

    let Term::Vector(draws) = m.get(&TermOrdKey(Term::symbol(":draws"))).expect(":draws") else {
        panic!("draws must be vector");
    };
    assert_eq!(
        draws.len(),
        4,
        "root bg + title text + button bg + button text"
    );
    let Term::Map(d0) = &draws[0] else {
        panic!("draw[0] must be map");
    };
    assert_eq!(
        d0.get(&TermOrdKey(Term::symbol(":kind"))),
        Some(&Term::symbol(":rect"))
    );
    let Term::Map(d1) = &draws[1] else {
        panic!("draw[1] must be map");
    };
    assert_eq!(
        d1.get(&TermOrdKey(Term::symbol(":kind"))),
        Some(&Term::symbol(":text"))
    );
    let Term::Map(d2) = &draws[2] else {
        panic!("draw[2] must be map");
    };
    assert_eq!(
        d2.get(&TermOrdKey(Term::symbol(":kind"))),
        Some(&Term::symbol(":rect"))
    );
    let Term::Map(d3) = &draws[3] else {
        panic!("draw[3] must be map");
    };
    assert_eq!(
        d3.get(&TermOrdKey(Term::symbol(":kind"))),
        Some(&Term::symbol(":text"))
    );

    let Term::Vector(batches) = m
        .get(&TermOrdKey(Term::symbol(":batches")))
        .expect(":batches")
    else {
        panic!("batches must be vector");
    };
    assert_eq!(
        batches.len(),
        4,
        "alternating rect/text keys produce 4 runs"
    );

    let Term::Map(frame) = m.get(&TermOrdKey(Term::symbol(":frame"))).expect(":frame") else {
        panic!("frame must be map");
    };
    let Some(Term::Vector(rpasses)) = frame.get(&TermOrdKey(Term::symbol(":render-passes"))) else {
        panic!("render passes missing");
    };
    assert_eq!(rpasses.len(), 1);
    let Term::Map(pass) = &rpasses[0] else {
        panic!("render pass must be map");
    };
    let Some(Term::Vector(commands)) = pass.get(&TermOrdKey(Term::symbol(":commands"))) else {
        panic!("commands missing");
    };
    assert_eq!(commands.len(), 5, "set-pipeline + one draw per batch");
}

#[test]
fn prelude_gfx_runtime_trace_artifact_is_deterministic_and_storeable() {
    let src = r#"
      (def scene (core/gfx/scene::empty "trace-demo"))
      (def ui-style ((((core/gfx/ui::style {:axis "vertical"}) {:w 640000 :h 480000}) {:gap 8000}) {:bg [0 0 0 1000000]}))
      (def title (((core/gfx/ui::text "title") "Trace") ui-style))
      (def children0 [])
      (def children1 ((core/vec::push children0) title))
      (def ui-root (((core/gfx/ui::container "root") ui-style) children1))
      (def plan-a ((((((core/gfx/runtime::plan-frame-2d+ui-trace scene) ui-root) "scene-pass") 17) "ui-pass") 23))
      (def plan-b ((((((core/gfx/runtime::plan-frame-2d+ui-trace scene) ui-root) "scene-pass") 17) "ui-pass") 23))
      (def trace-a ((core/map::get plan-a) (quote :trace)))
      {
        :plan plan-a
        :trace-h-a (core/coreform::hash-term trace-a)
        :trace-h-b (core/coreform::hash-term ((core/map::get plan-b) (quote :trace)))
        :store ((core/gfx/runtime::store-trace-artifact-with-meta trace-a) {:workflow "ai-editor"})
      }
    "#;
    let forms = canonicalize_module(parse_module(src).expect("parse")).expect("canonicalize");
    let mut ctx = EvalCtx::new();
    let prelude = build_prelude(&mut ctx);
    let mut env = prelude.env;
    let v = eval_module(&mut ctx, &mut env, &forms).expect("eval");
    let Value::Map(m) = v else {
        panic!("expected map");
    };

    let plan_v = m
        .get(&TermOrdKey(Term::symbol(":plan")))
        .expect(":plan")
        .clone();
    let Some(Term::Map(plan)) = value_to_data_term(&plan_v) else {
        panic!("plan must be map");
    };
    assert_eq!(
        plan.get(&TermOrdKey(Term::symbol(":kind"))),
        Some(&Term::symbol(":gfx/plan-frame-2d+ui-trace"))
    );
    let Some(Term::Map(trace)) = plan.get(&TermOrdKey(Term::symbol(":trace"))) else {
        panic!("trace must be map");
    };
    assert_eq!(
        trace.get(&TermOrdKey(Term::symbol(":kind"))),
        Some(&Term::symbol(":gfx/frame-trace"))
    );
    assert_eq!(
        trace.get(&TermOrdKey(Term::symbol(":ui-node-count"))),
        Some(&Term::Int(2.into()))
    );

    let Some(Term::Str(trace_h)) = m
        .get(&TermOrdKey(Term::symbol(":trace-h-a")))
        .and_then(value_to_data_term)
    else {
        panic!("trace-h-a must be string");
    };
    let Some(Term::Str(trace_h_b)) = m
        .get(&TermOrdKey(Term::symbol(":trace-h-b")))
        .and_then(value_to_data_term)
    else {
        panic!("trace-h-b must be string");
    };
    assert_eq!(trace_h.len(), 64);
    assert_eq!(trace_h, trace_h_b, "trace hash must be deterministic");

    let store_v = m
        .get(&TermOrdKey(Term::symbol(":store")))
        .expect(":store")
        .clone();
    let req = get_req(store_v);
    assert_eq!(req.op, "core/store::put");
    let Term::Map(payload) = req.payload else {
        panic!("store payload must be map");
    };
    let Some(Term::Map(artifact)) = payload.get(&TermOrdKey(Term::symbol(":artifact"))) else {
        panic!("artifact payload missing");
    };
    assert_eq!(
        artifact.get(&TermOrdKey(Term::symbol(":kind"))),
        Some(&Term::symbol("genesis/gfx-frame-trace-v0.2"))
    );
    assert_eq!(
        artifact.get(&TermOrdKey(Term::symbol(":trace-h"))),
        Some(&Term::Str(trace_h))
    );
    let Some(Term::Map(meta)) = artifact.get(&TermOrdKey(Term::symbol(":meta"))) else {
        panic!("trace artifact meta missing");
    };
    assert_eq!(
        meta.get(&TermOrdKey(Term::symbol(":workflow"))),
        Some(&Term::Str("ai-editor".to_string()))
    );
}
