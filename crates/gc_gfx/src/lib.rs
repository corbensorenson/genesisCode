use std::collections::BTreeMap;

use gc_coreform::{Term, TermOrdKey, hash_term};

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct ResourceId(pub u64);

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BufferDesc {
    pub label: String,
    pub size_bytes: u64,
    pub usage_bits: u64,
    pub mapped_at_creation: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TextureDesc {
    pub label: String,
    pub width: u32,
    pub height: u32,
    pub layers: u32,
    pub mip_levels: u32,
    pub format: String,
    pub usage_bits: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SamplerDesc {
    pub label: String,
    pub min_filter: String,
    pub mag_filter: String,
    pub mipmap_filter: String,
    pub address_u: String,
    pub address_v: String,
    pub address_w: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ShaderModuleDesc {
    pub label: String,
    pub source_artifact: String,
    pub stage: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RenderPipelineDesc {
    pub label: String,
    pub vertex_shader: ResourceId,
    pub fragment_shader: ResourceId,
    pub vertex_layout: String,
    pub color_format: String,
    pub depth_format: Option<String>,
    pub cull_mode: String,
    pub front_face: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ComputePipelineDesc {
    pub label: String,
    pub compute_shader: ResourceId,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DrawCommand {
    SetPipeline {
        pipeline: ResourceId,
    },
    SetVertexBuffer {
        slot: u32,
        buffer: ResourceId,
        offset_bytes: u64,
    },
    SetIndexBuffer {
        buffer: ResourceId,
        offset_bytes: u64,
        index_format: String,
    },
    SetBindGroup {
        index: u32,
        bind_group: ResourceId,
    },
    SetPushConstants {
        stage: String,
        offset_bytes: u32,
        data_artifact: String,
    },
    Draw {
        vertex_count: u32,
        instance_count: u32,
        first_vertex: u32,
        first_instance: u32,
    },
    DrawIndexed {
        index_count: u32,
        instance_count: u32,
        first_index: u32,
        base_vertex: i32,
        first_instance: u32,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ComputeCommand {
    SetPipeline {
        pipeline: ResourceId,
    },
    SetBindGroup {
        index: u32,
        bind_group: ResourceId,
    },
    SetPushConstants {
        offset_bytes: u32,
        data_artifact: String,
    },
    Dispatch {
        x: u32,
        y: u32,
        z: u32,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RenderPass {
    pub label: String,
    pub color_attachments: Vec<ResourceId>,
    pub depth_attachment: Option<ResourceId>,
    pub commands: Vec<DrawCommand>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ComputePass {
    pub label: String,
    pub commands: Vec<ComputeCommand>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FrameGraph {
    pub render_passes: Vec<RenderPass>,
    pub compute_passes: Vec<ComputePass>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Vec3i {
    pub x: i64,
    pub y: i64,
    pub z: i64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct QuatI {
    pub x: i64,
    pub y: i64,
    pub z: i64,
    pub w: i64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Transform {
    pub translation: Vec3i,
    pub rotation: QuatI,
    pub scale: Vec3i,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MeshRef {
    pub mesh_artifact: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MaterialPbr {
    pub base_color: [i64; 4],
    pub base_color_texture: Option<String>,
    pub metallic_roughness_texture: Option<String>,
    pub normal_texture: Option<String>,
    pub emissive_texture: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Camera {
    pub kind: String,
    pub fov_milli_deg: i64,
    pub near_mm: i64,
    pub far_mm: i64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Light {
    pub kind: String,
    pub color_rgb: [i64; 3],
    pub intensity_milli: i64,
    pub range_mm: i64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SceneNode {
    pub name: String,
    pub transform: Transform,
    pub mesh: Option<MeshRef>,
    pub material: Option<MaterialPbr>,
    pub camera: Option<Camera>,
    pub light: Option<Light>,
    pub children: Vec<u32>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Scene {
    pub name: String,
    pub root_nodes: Vec<u32>,
    pub nodes: Vec<SceneNode>,
}

pub fn frame_graph_hash(graph: &FrameGraph) -> [u8; 32] {
    let mut pre = b"GCv0.2\0gfx/frame-graph\0".to_vec();
    pre.extend_from_slice(&hash_term(&graph.to_term()));
    *blake3::hash(&pre).as_bytes()
}

pub fn scene_hash(scene: &Scene) -> [u8; 32] {
    let mut pre = b"GCv0.2\0gfx/scene\0".to_vec();
    pre.extend_from_slice(&hash_term(&scene.to_term()));
    *blake3::hash(&pre).as_bytes()
}

pub trait ToTerm {
    fn to_term(&self) -> Term;
}

impl ToTerm for ResourceId {
    fn to_term(&self) -> Term {
        Term::Int((self.0 as i128).into())
    }
}

fn map(fields: Vec<(&str, Term)>) -> Term {
    let mut m = BTreeMap::new();
    for (k, v) in fields {
        m.insert(TermOrdKey(Term::Symbol(k.to_string())), v);
    }
    Term::Map(m)
}

fn term_opt_str(v: &Option<String>) -> Term {
    match v {
        Some(s) => Term::Str(s.clone()),
        None => Term::Nil,
    }
}

impl ToTerm for BufferDesc {
    fn to_term(&self) -> Term {
        map(vec![
            (":type", Term::Symbol(":gfx/buffer-desc".to_string())),
            (":label", Term::Str(self.label.clone())),
            (":size-bytes", Term::Int((self.size_bytes as i128).into())),
            (":usage-bits", Term::Int((self.usage_bits as i128).into())),
            (":mapped-at-creation", Term::Bool(self.mapped_at_creation)),
        ])
    }
}

impl ToTerm for TextureDesc {
    fn to_term(&self) -> Term {
        map(vec![
            (":type", Term::Symbol(":gfx/texture-desc".to_string())),
            (":label", Term::Str(self.label.clone())),
            (":width", Term::Int((self.width as i128).into())),
            (":height", Term::Int((self.height as i128).into())),
            (":layers", Term::Int((self.layers as i128).into())),
            (":mip-levels", Term::Int((self.mip_levels as i128).into())),
            (":format", Term::Str(self.format.clone())),
            (":usage-bits", Term::Int((self.usage_bits as i128).into())),
        ])
    }
}

impl ToTerm for SamplerDesc {
    fn to_term(&self) -> Term {
        map(vec![
            (":type", Term::Symbol(":gfx/sampler-desc".to_string())),
            (":label", Term::Str(self.label.clone())),
            (":min-filter", Term::Str(self.min_filter.clone())),
            (":mag-filter", Term::Str(self.mag_filter.clone())),
            (":mipmap-filter", Term::Str(self.mipmap_filter.clone())),
            (":address-u", Term::Str(self.address_u.clone())),
            (":address-v", Term::Str(self.address_v.clone())),
            (":address-w", Term::Str(self.address_w.clone())),
        ])
    }
}

impl ToTerm for ShaderModuleDesc {
    fn to_term(&self) -> Term {
        map(vec![
            (":type", Term::Symbol(":gfx/shader-module-desc".to_string())),
            (":label", Term::Str(self.label.clone())),
            (":source-artifact", Term::Str(self.source_artifact.clone())),
            (":stage", Term::Str(self.stage.clone())),
        ])
    }
}

impl ToTerm for RenderPipelineDesc {
    fn to_term(&self) -> Term {
        map(vec![
            (
                ":type",
                Term::Symbol(":gfx/render-pipeline-desc".to_string()),
            ),
            (":label", Term::Str(self.label.clone())),
            (":vs", self.vertex_shader.to_term()),
            (":fs", self.fragment_shader.to_term()),
            (":vertex-layout", Term::Str(self.vertex_layout.clone())),
            (":color-format", Term::Str(self.color_format.clone())),
            (
                ":depth-format",
                self.depth_format.clone().map_or(Term::Nil, Term::Str),
            ),
            (":cull-mode", Term::Str(self.cull_mode.clone())),
            (":front-face", Term::Str(self.front_face.clone())),
        ])
    }
}

impl ToTerm for ComputePipelineDesc {
    fn to_term(&self) -> Term {
        map(vec![
            (
                ":type",
                Term::Symbol(":gfx/compute-pipeline-desc".to_string()),
            ),
            (":label", Term::Str(self.label.clone())),
            (":cs", self.compute_shader.to_term()),
        ])
    }
}

impl ToTerm for DrawCommand {
    fn to_term(&self) -> Term {
        match self {
            DrawCommand::SetPipeline { pipeline } => map(vec![
                (":op", Term::Symbol(":set-pipeline".to_string())),
                (":pipeline", pipeline.to_term()),
            ]),
            DrawCommand::SetVertexBuffer {
                slot,
                buffer,
                offset_bytes,
            } => map(vec![
                (":op", Term::Symbol(":set-vertex-buffer".to_string())),
                (":slot", Term::Int((*slot as i128).into())),
                (":buffer", buffer.to_term()),
                (":offset-bytes", Term::Int((*offset_bytes as i128).into())),
            ]),
            DrawCommand::SetIndexBuffer {
                buffer,
                offset_bytes,
                index_format,
            } => map(vec![
                (":op", Term::Symbol(":set-index-buffer".to_string())),
                (":buffer", buffer.to_term()),
                (":offset-bytes", Term::Int((*offset_bytes as i128).into())),
                (":index-format", Term::Str(index_format.clone())),
            ]),
            DrawCommand::SetBindGroup { index, bind_group } => map(vec![
                (":op", Term::Symbol(":set-bind-group".to_string())),
                (":index", Term::Int((*index as i128).into())),
                (":bind-group", bind_group.to_term()),
            ]),
            DrawCommand::SetPushConstants {
                stage,
                offset_bytes,
                data_artifact,
            } => map(vec![
                (":op", Term::Symbol(":set-push-constants".to_string())),
                (":stage", Term::Str(stage.clone())),
                (":offset-bytes", Term::Int((*offset_bytes as i128).into())),
                (":data-artifact", Term::Str(data_artifact.clone())),
            ]),
            DrawCommand::Draw {
                vertex_count,
                instance_count,
                first_vertex,
                first_instance,
            } => map(vec![
                (":op", Term::Symbol(":draw".to_string())),
                (":vertex-count", Term::Int((*vertex_count as i128).into())),
                (
                    ":instance-count",
                    Term::Int((*instance_count as i128).into()),
                ),
                (":first-vertex", Term::Int((*first_vertex as i128).into())),
                (
                    ":first-instance",
                    Term::Int((*first_instance as i128).into()),
                ),
            ]),
            DrawCommand::DrawIndexed {
                index_count,
                instance_count,
                first_index,
                base_vertex,
                first_instance,
            } => map(vec![
                (":op", Term::Symbol(":draw-indexed".to_string())),
                (":index-count", Term::Int((*index_count as i128).into())),
                (
                    ":instance-count",
                    Term::Int((*instance_count as i128).into()),
                ),
                (":first-index", Term::Int((*first_index as i128).into())),
                (":base-vertex", Term::Int((*base_vertex as i128).into())),
                (
                    ":first-instance",
                    Term::Int((*first_instance as i128).into()),
                ),
            ]),
        }
    }
}

impl ToTerm for ComputeCommand {
    fn to_term(&self) -> Term {
        match self {
            ComputeCommand::SetPipeline { pipeline } => map(vec![
                (":op", Term::Symbol(":set-pipeline".to_string())),
                (":pipeline", pipeline.to_term()),
            ]),
            ComputeCommand::SetBindGroup { index, bind_group } => map(vec![
                (":op", Term::Symbol(":set-bind-group".to_string())),
                (":index", Term::Int((*index as i128).into())),
                (":bind-group", bind_group.to_term()),
            ]),
            ComputeCommand::SetPushConstants {
                offset_bytes,
                data_artifact,
            } => map(vec![
                (":op", Term::Symbol(":set-push-constants".to_string())),
                (":offset-bytes", Term::Int((*offset_bytes as i128).into())),
                (":data-artifact", Term::Str(data_artifact.clone())),
            ]),
            ComputeCommand::Dispatch { x, y, z } => map(vec![
                (":op", Term::Symbol(":dispatch".to_string())),
                (":x", Term::Int((*x as i128).into())),
                (":y", Term::Int((*y as i128).into())),
                (":z", Term::Int((*z as i128).into())),
            ]),
        }
    }
}

impl ToTerm for RenderPass {
    fn to_term(&self) -> Term {
        map(vec![
            (":type", Term::Symbol(":gfx/render-pass".to_string())),
            (":label", Term::Str(self.label.clone())),
            (
                ":color-attachments",
                Term::Vector(self.color_attachments.iter().map(ToTerm::to_term).collect()),
            ),
            (
                ":depth-attachment",
                self.depth_attachment
                    .as_ref()
                    .map_or(Term::Nil, ToTerm::to_term),
            ),
            (
                ":commands",
                Term::Vector(self.commands.iter().map(ToTerm::to_term).collect()),
            ),
        ])
    }
}

impl ToTerm for ComputePass {
    fn to_term(&self) -> Term {
        map(vec![
            (":type", Term::Symbol(":gfx/compute-pass".to_string())),
            (":label", Term::Str(self.label.clone())),
            (
                ":commands",
                Term::Vector(self.commands.iter().map(ToTerm::to_term).collect()),
            ),
        ])
    }
}

impl ToTerm for FrameGraph {
    fn to_term(&self) -> Term {
        map(vec![
            (":type", Term::Symbol(":gfx/frame-graph".to_string())),
            (
                ":render-passes",
                Term::Vector(self.render_passes.iter().map(ToTerm::to_term).collect()),
            ),
            (
                ":compute-passes",
                Term::Vector(self.compute_passes.iter().map(ToTerm::to_term).collect()),
            ),
        ])
    }
}

impl ToTerm for Vec3i {
    fn to_term(&self) -> Term {
        map(vec![
            (":x", Term::Int(self.x.into())),
            (":y", Term::Int(self.y.into())),
            (":z", Term::Int(self.z.into())),
        ])
    }
}

impl ToTerm for QuatI {
    fn to_term(&self) -> Term {
        map(vec![
            (":x", Term::Int(self.x.into())),
            (":y", Term::Int(self.y.into())),
            (":z", Term::Int(self.z.into())),
            (":w", Term::Int(self.w.into())),
        ])
    }
}

impl ToTerm for Transform {
    fn to_term(&self) -> Term {
        map(vec![
            (":translation", self.translation.to_term()),
            (":rotation", self.rotation.to_term()),
            (":scale", self.scale.to_term()),
        ])
    }
}

impl ToTerm for MeshRef {
    fn to_term(&self) -> Term {
        map(vec![(
            ":mesh-artifact",
            Term::Str(self.mesh_artifact.clone()),
        )])
    }
}

impl ToTerm for MaterialPbr {
    fn to_term(&self) -> Term {
        map(vec![
            (":type", Term::Symbol(":gfx/material-pbr".to_string())),
            (
                ":base-color",
                Term::Vector(
                    self.base_color
                        .iter()
                        .map(|x| Term::Int((*x).into()))
                        .collect(),
                ),
            ),
            (
                ":base-color-texture",
                term_opt_str(&self.base_color_texture),
            ),
            (
                ":metallic-roughness-texture",
                term_opt_str(&self.metallic_roughness_texture),
            ),
            (":normal-texture", term_opt_str(&self.normal_texture)),
            (":emissive-texture", term_opt_str(&self.emissive_texture)),
        ])
    }
}

impl ToTerm for Camera {
    fn to_term(&self) -> Term {
        map(vec![
            (":kind", Term::Str(self.kind.clone())),
            (":fov-milli-deg", Term::Int(self.fov_milli_deg.into())),
            (":near-mm", Term::Int(self.near_mm.into())),
            (":far-mm", Term::Int(self.far_mm.into())),
        ])
    }
}

impl ToTerm for Light {
    fn to_term(&self) -> Term {
        map(vec![
            (":kind", Term::Str(self.kind.clone())),
            (
                ":color-rgb",
                Term::Vector(
                    self.color_rgb
                        .iter()
                        .map(|x| Term::Int((*x).into()))
                        .collect(),
                ),
            ),
            (":intensity-milli", Term::Int(self.intensity_milli.into())),
            (":range-mm", Term::Int(self.range_mm.into())),
        ])
    }
}

impl ToTerm for SceneNode {
    fn to_term(&self) -> Term {
        map(vec![
            (":name", Term::Str(self.name.clone())),
            (":transform", self.transform.to_term()),
            (
                ":mesh",
                self.mesh.as_ref().map_or(Term::Nil, ToTerm::to_term),
            ),
            (
                ":material",
                self.material.as_ref().map_or(Term::Nil, ToTerm::to_term),
            ),
            (
                ":camera",
                self.camera.as_ref().map_or(Term::Nil, ToTerm::to_term),
            ),
            (
                ":light",
                self.light.as_ref().map_or(Term::Nil, ToTerm::to_term),
            ),
            (
                ":children",
                Term::Vector(
                    self.children
                        .iter()
                        .map(|i| Term::Int((*i as i128).into()))
                        .collect(),
                ),
            ),
        ])
    }
}

impl ToTerm for Scene {
    fn to_term(&self) -> Term {
        map(vec![
            (":type", Term::Symbol(":gfx/scene".to_string())),
            (":name", Term::Str(self.name.clone())),
            (
                ":root-nodes",
                Term::Vector(
                    self.root_nodes
                        .iter()
                        .map(|i| Term::Int((*i as i128).into()))
                        .collect(),
                ),
            ),
            (
                ":nodes",
                Term::Vector(self.nodes.iter().map(ToTerm::to_term).collect()),
            ),
        ])
    }
}

#[cfg(test)]
mod tests {
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
}
