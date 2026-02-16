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

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HeadlessRenderOutput {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
    pub png: Vec<u8>,
    pub pixel_hash: [u8; 32],
    pub png_hash: [u8; 32],
}

pub fn render_frame_graph_headless(
    frame_graph: &Term,
    width: u32,
    height: u32,
) -> Result<HeadlessRenderOutput, String> {
    if width == 0 || height == 0 {
        return Err("headless render size must be non-zero".to_string());
    }
    if !matches!(
        map_get(frame_graph, ":type"),
        Some(Term::Symbol(s)) if s == ":gfx/frame-graph"
    ) {
        return Err("expected :gfx/frame-graph term".to_string());
    }

    let render_passes = map_get(frame_graph, ":render-passes")
        .and_then(as_vec)
        .ok_or_else(|| "frame graph missing :render-passes vector".to_string())?;

    let frame_h = hash_term(frame_graph);
    let px_len = (width as usize)
        .saturating_mul(height as usize)
        .saturating_mul(4);
    let mut rgba = vec![0u8; px_len];

    // Deterministic background gradient keyed by frame hash.
    for y in 0..height {
        for x in 0..width {
            let i = ((y as usize) * (width as usize) + (x as usize)) * 4;
            rgba[i] = frame_h[0].wrapping_add((x % 251) as u8);
            rgba[i + 1] = frame_h[1].wrapping_add((y % 241) as u8);
            rgba[i + 2] = frame_h[2].wrapping_add(((x ^ y) % 239) as u8);
            rgba[i + 3] = 255;
        }
    }

    for (pi, pass) in render_passes.iter().enumerate() {
        let Some(commands) = map_get(pass, ":commands").and_then(as_vec) else {
            continue;
        };
        let pass_label = map_get(pass, ":label").and_then(as_str).unwrap_or_default();
        for (ci, cmd) in commands.iter().enumerate() {
            let cmd_hash = hash_term(cmd);
            let mut hasher = blake3::Hasher::new();
            hasher.update(b"GCv0.2\0gfx/headless-raster\0");
            hasher.update(&frame_h);
            hasher.update(&(pi as u64).to_le_bytes());
            hasher.update(&(ci as u64).to_le_bytes());
            hasher.update(pass_label.as_bytes());
            hasher.update(&cmd_hash);
            let seed = hasher.finalize();
            let b = seed.as_bytes();

            let x0 = read_u32(b, 0) % width;
            let y0 = read_u32(b, 4) % height;
            let w_max = (width / 2).max(1);
            let h_max = (height / 2).max(1);
            let rw = 1 + (read_u32(b, 8) % w_max);
            let rh = 1 + (read_u32(b, 12) % h_max);
            let x1 = x0.saturating_add(rw).min(width);
            let y1 = y0.saturating_add(rh).min(height);

            let op = map_get(cmd, ":op").and_then(as_sym).unwrap_or_default();
            let op_bias = match op {
                ":draw" => 17u8,
                ":draw-indexed" => 43u8,
                ":dispatch" => 71u8,
                ":set-pipeline" => 97u8,
                _ => 131u8,
            };
            let color = [
                b[16].wrapping_add(op_bias),
                b[17].wrapping_add(op_bias / 2),
                b[18].wrapping_add(op_bias / 3),
                96u8.wrapping_add(b[19] % 160),
            ];
            blend_rect(&mut rgba, width, x0, y0, x1, y1, color);
        }
    }

    let pixel_hash = *blake3::hash(&rgba).as_bytes();
    let png = encode_png_rgba(width, height, &rgba)?;
    let png_hash = *blake3::hash(&png).as_bytes();
    Ok(HeadlessRenderOutput {
        width,
        height,
        rgba,
        png,
        pixel_hash,
        png_hash,
    })
}

fn encode_png_rgba(width: u32, height: u32, rgba: &[u8]) -> Result<Vec<u8>, String> {
    let mut out = Vec::new();
    let mut enc = png::Encoder::new(&mut out, width, height);
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    let mut writer = enc
        .write_header()
        .map_err(|e| format!("png header write failed: {e}"))?;
    writer
        .write_image_data(rgba)
        .map_err(|e| format!("png data write failed: {e}"))?;
    drop(writer);
    Ok(out)
}

fn read_u32(bytes: &[u8], off: usize) -> u32 {
    let mut b = [0u8; 4];
    b.copy_from_slice(&bytes[off..off + 4]);
    u32::from_le_bytes(b)
}

fn blend_rect(rgba: &mut [u8], width: u32, x0: u32, y0: u32, x1: u32, y1: u32, color: [u8; 4]) {
    let a = color[3] as u16;
    let inv = 255u16.saturating_sub(a);
    for y in y0..y1 {
        for x in x0..x1 {
            let i = ((y as usize) * (width as usize) + (x as usize)) * 4;
            let dr = rgba[i] as u16;
            let dg = rgba[i + 1] as u16;
            let db = rgba[i + 2] as u16;
            rgba[i] = ((dr * inv + (color[0] as u16) * a) / 255) as u8;
            rgba[i + 1] = ((dg * inv + (color[1] as u16) * a) / 255) as u8;
            rgba[i + 2] = ((db * inv + (color[2] as u16) * a) / 255) as u8;
            rgba[i + 3] = 255;
        }
    }
}

fn map_get<'a>(t: &'a Term, key: &str) -> Option<&'a Term> {
    let Term::Map(m) = t else { return None };
    m.get(&TermOrdKey(Term::Symbol(key.to_string())))
}

fn as_vec(t: &Term) -> Option<&Vec<Term>> {
    let Term::Vector(v) = t else { return None };
    Some(v)
}

fn as_str(t: &Term) -> Option<&str> {
    let Term::Str(s) = t else { return None };
    Some(s.as_str())
}

fn as_sym(t: &Term) -> Option<&str> {
    let Term::Symbol(s) = t else { return None };
    Some(s.as_str())
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
}
