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


mod headless;
mod term_codec;

pub use headless::{HeadlessRenderOutput, render_frame_graph_headless};
pub use term_codec::ToTerm;

#[cfg(test)]
mod tests;
