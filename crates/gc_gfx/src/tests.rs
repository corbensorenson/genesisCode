use super::*;

#[test]
fn frame_graph_hash_is_stable() {
    let g = FrameGraph {
        render_passes: vec![RenderPass {
            label: "main".to_string(),
            color_attachments: vec![ResourceId(1)],
            depth_attachment: Some(ResourceId(2)),
            commands: vec![
                DrawCommand::SetPipeline {
                    pipeline: ResourceId(100),
                },
                DrawCommand::Draw {
                    vertex_count: 3,
                    instance_count: 1,
                    first_vertex: 0,
                    first_instance: 0,
                },
            ],
        }],
        compute_passes: vec![ComputePass {
            label: "cull".to_string(),
            commands: vec![ComputeCommand::Dispatch { x: 16, y: 16, z: 1 }],
        }],
    };

    let h1 = frame_graph_hash(&g);
    let h2 = frame_graph_hash(&g);
    assert_eq!(h1, h2);
}

#[test]
fn scene_term_contains_expected_symbols() {
    let s = Scene {
        name: "demo".to_string(),
        root_nodes: vec![0],
        nodes: vec![SceneNode {
            name: "root".to_string(),
            transform: Transform {
                translation: Vec3i { x: 0, y: 0, z: 0 },
                rotation: QuatI {
                    x: 0,
                    y: 0,
                    z: 0,
                    w: 1_000_000,
                },
                scale: Vec3i {
                    x: 1_000_000,
                    y: 1_000_000,
                    z: 1_000_000,
                },
            },
            mesh: None,
            material: None,
            camera: Some(Camera {
                kind: "perspective".to_string(),
                fov_milli_deg: 60_000,
                near_mm: 100,
                far_mm: 1_000_000,
            }),
            light: None,
            children: vec![],
        }],
    };

    let t = s.to_term();
    let Term::Map(m) = t else {
        panic!("expected map");
    };
    assert!(matches!(
        m.get(&TermOrdKey(Term::Symbol(":type".to_string()))),
        Some(Term::Symbol(s)) if s == ":gfx/scene"
    ));
    assert!(matches!(
        m.get(&TermOrdKey(Term::Symbol(":nodes".to_string()))),
        Some(Term::Vector(_))
    ));
}

#[test]
fn headless_render_is_deterministic() {
    let graph = FrameGraph {
        render_passes: vec![RenderPass {
            label: "main".to_string(),
            color_attachments: vec![],
            depth_attachment: None,
            commands: vec![
                DrawCommand::SetPipeline {
                    pipeline: ResourceId(1),
                },
                DrawCommand::Draw {
                    vertex_count: 3,
                    instance_count: 1,
                    first_vertex: 0,
                    first_instance: 0,
                },
            ],
        }],
        compute_passes: vec![],
    }
    .to_term();
    let a = render_frame_graph_headless(&graph, 128, 96).expect("render a");
    let b = render_frame_graph_headless(&graph, 128, 96).expect("render b");
    assert_eq!(a.png_hash, b.png_hash);
    assert_eq!(a.pixel_hash, b.pixel_hash);
    assert!(!a.png.is_empty());
}

#[test]
fn headless_render_changes_when_commands_change() {
    let g1 = FrameGraph {
        render_passes: vec![RenderPass {
            label: "main".to_string(),
            color_attachments: vec![],
            depth_attachment: None,
            commands: vec![DrawCommand::Draw {
                vertex_count: 3,
                instance_count: 1,
                first_vertex: 0,
                first_instance: 0,
            }],
        }],
        compute_passes: vec![],
    }
    .to_term();
    let g2 = FrameGraph {
        render_passes: vec![RenderPass {
            label: "main".to_string(),
            color_attachments: vec![],
            depth_attachment: None,
            commands: vec![
                DrawCommand::Draw {
                    vertex_count: 3,
                    instance_count: 1,
                    first_vertex: 0,
                    first_instance: 0,
                },
                DrawCommand::Draw {
                    vertex_count: 6,
                    instance_count: 1,
                    first_vertex: 0,
                    first_instance: 0,
                },
            ],
        }],
        compute_passes: vec![],
    }
    .to_term();
    let a = render_frame_graph_headless(&g1, 96, 96).expect("render g1");
    let b = render_frame_graph_headless(&g2, 96, 96).expect("render g2");
    assert_ne!(a.png_hash, b.png_hash);
}
