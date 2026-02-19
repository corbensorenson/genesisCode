use std::collections::BTreeMap;

use gc_coreform::{Term, TermOrdKey, hash_term};
use gc_kernel::{SealId, Value};
use num_bigint::BigInt;
use num_traits::ToPrimitive;

const GPU_RESOURCE_MAX_BYTES: usize = 64 * 1024 * 1024;

#[derive(Debug, Clone, Default)]
pub(crate) struct GpuHostRuntime {
    next_id: u64,
    submit_seq: u64,
    resources: BTreeMap<String, GpuResource>,
}

#[derive(Debug, Clone)]
enum GpuResource {
    Buffer(BufferResource),
    Texture(TextureResource),
    Sampler { desc: Term },
    ShaderModule { desc: Term },
    BindGroupLayout { desc: Term },
    BindGroup { desc: Term },
    PipelineLayout { desc: Term },
    RenderPipeline { desc: Term },
    ComputePipeline { desc: Term },
}

#[derive(Debug, Clone)]
struct BufferResource {
    desc: Term,
    bytes: Vec<u8>,
}

#[derive(Debug, Clone)]
struct TextureResource {
    desc: Term,
    bytes: Vec<u8>,
}

pub(crate) fn gpu_host_call(
    runtime: &mut GpuHostRuntime,
    op: &str,
    payload: &Term,
    error_tok: SealId,
) -> Option<Value> {
    match op {
        "gfx/gpu::create-buffer" => Some(create_buffer(runtime, payload, error_tok, op)),
        "gfx/gpu::create-texture" => Some(create_texture(runtime, payload, error_tok, op)),
        "gfx/gpu::create-sampler" => Some(create_simple_resource(
            runtime,
            payload,
            GpuKind::Sampler,
            "sampler",
        )),
        "gfx/gpu::create-shader-module" => Some(create_simple_resource(
            runtime,
            payload,
            GpuKind::ShaderModule,
            "shader-module",
        )),
        "gfx/gpu::create-bind-group-layout" => Some(create_simple_resource(
            runtime,
            payload,
            GpuKind::BindGroupLayout,
            "bind-group-layout",
        )),
        "gfx/gpu::create-bind-group" => Some(create_simple_resource(
            runtime,
            payload,
            GpuKind::BindGroup,
            "bind-group",
        )),
        "gfx/gpu::create-pipeline-layout" => Some(create_simple_resource(
            runtime,
            payload,
            GpuKind::PipelineLayout,
            "pipeline-layout",
        )),
        "gfx/gpu::create-render-pipeline" => Some(create_simple_resource(
            runtime,
            payload,
            GpuKind::RenderPipeline,
            "render-pipeline",
        )),
        "gfx/gpu::create-compute-pipeline" => Some(create_simple_resource(
            runtime,
            payload,
            GpuKind::ComputePipeline,
            "compute-pipeline",
        )),
        "gfx/gpu::destroy-resource" => Some(destroy_resource(runtime, payload, error_tok, op)),
        "gfx/gpu::write-buffer" => Some(write_buffer(runtime, payload, error_tok, op)),
        "gfx/gpu::write-texture" => Some(write_texture(runtime, payload, error_tok, op)),
        "gfx/gpu::read-buffer" => Some(read_buffer(runtime, payload, error_tok, op)),
        "gfx/gpu::read-texture" => Some(read_texture(runtime, payload, error_tok, op)),
        "gfx/gpu::submit-frame-graph" => {
            Some(submit_graph(runtime, payload, "frame", error_tok, op))
        }
        "gfx/gpu::submit-compute-graph" => {
            Some(submit_graph(runtime, payload, "compute", error_tok, op))
        }
        "gfx/gpu::limits" => Some(gpu_limits()),
        "gfx/gpu::features" => Some(gpu_features()),
        _ => None,
    }
}

fn create_buffer(
    runtime: &mut GpuHostRuntime,
    payload: &Term,
    error_tok: SealId,
    op: &str,
) -> Value {
    let desc = map_field(payload, ":desc").cloned().unwrap_or(Term::Nil);
    let mut size = map_field_usize(&desc, ":size").unwrap_or(0);
    let initial_data = map_field_bytes(payload, ":data")
        .or_else(|| map_field_bytes(&desc, ":data"))
        .unwrap_or_default();
    if initial_data.len() > size {
        size = initial_data.len();
    }
    if size > GPU_RESOURCE_MAX_BYTES {
        return mk_error(
            error_tok,
            "core/caps/resource-limit",
            format!("buffer size exceeds host limit ({size} > {GPU_RESOURCE_MAX_BYTES})"),
            Some(op),
        );
    }
    let mut bytes = vec![0u8; size];
    if !initial_data.is_empty() {
        bytes[..initial_data.len()].copy_from_slice(&initial_data);
    }
    let id = alloc_id(runtime, "buffer");
    runtime.resources.insert(
        id.clone(),
        GpuResource::Buffer(BufferResource { desc, bytes }),
    );
    Value::Data(map_term([
        (":ok", Term::Bool(true)),
        (":id", Term::Str(id)),
        (":kind", Term::Symbol(":buffer".to_string())),
        (":size", Term::Int(BigInt::from(size))),
    ]))
}

fn create_texture(
    runtime: &mut GpuHostRuntime,
    payload: &Term,
    error_tok: SealId,
    op: &str,
) -> Value {
    let desc = map_field(payload, ":desc").cloned().unwrap_or(Term::Nil);
    let mut size = map_field_usize(&desc, ":byte-size")
        .or_else(|| map_field_usize(payload, ":byte-size"))
        .unwrap_or_else(|| infer_texture_bytes(&desc));
    let initial_data = map_field_bytes(payload, ":data")
        .or_else(|| map_field_bytes(&desc, ":data"))
        .unwrap_or_default();
    if initial_data.len() > size {
        size = initial_data.len();
    }
    if size > GPU_RESOURCE_MAX_BYTES {
        return mk_error(
            error_tok,
            "core/caps/resource-limit",
            format!("texture size exceeds host limit ({size} > {GPU_RESOURCE_MAX_BYTES})"),
            Some(op),
        );
    }
    let mut bytes = vec![0u8; size];
    if !initial_data.is_empty() {
        bytes[..initial_data.len()].copy_from_slice(&initial_data);
    }
    let id = alloc_id(runtime, "texture");
    runtime.resources.insert(
        id.clone(),
        GpuResource::Texture(TextureResource { desc, bytes }),
    );
    Value::Data(map_term([
        (":ok", Term::Bool(true)),
        (":id", Term::Str(id)),
        (":kind", Term::Symbol(":texture".to_string())),
        (":byte-size", Term::Int(BigInt::from(size))),
    ]))
}

enum GpuKind {
    Sampler,
    ShaderModule,
    BindGroupLayout,
    BindGroup,
    PipelineLayout,
    RenderPipeline,
    ComputePipeline,
}

fn create_simple_resource(
    runtime: &mut GpuHostRuntime,
    payload: &Term,
    kind: GpuKind,
    id_suffix: &str,
) -> Value {
    let desc = map_field(payload, ":desc").cloned().unwrap_or(Term::Nil);
    let id = alloc_id(runtime, id_suffix);
    let resource = match kind {
        GpuKind::Sampler => GpuResource::Sampler { desc },
        GpuKind::ShaderModule => GpuResource::ShaderModule { desc },
        GpuKind::BindGroupLayout => GpuResource::BindGroupLayout { desc },
        GpuKind::BindGroup => GpuResource::BindGroup { desc },
        GpuKind::PipelineLayout => GpuResource::PipelineLayout { desc },
        GpuKind::RenderPipeline => GpuResource::RenderPipeline { desc },
        GpuKind::ComputePipeline => GpuResource::ComputePipeline { desc },
    };
    runtime.resources.insert(id.clone(), resource);
    Value::Data(map_term([
        (":ok", Term::Bool(true)),
        (":id", Term::Str(id)),
        (":kind", Term::Str(id_suffix.to_string())),
    ]))
}

fn destroy_resource(
    runtime: &mut GpuHostRuntime,
    payload: &Term,
    error_tok: SealId,
    op: &str,
) -> Value {
    let Some(id) = map_field_str_or_symbol(payload, ":id") else {
        return mk_error(
            error_tok,
            "gfx/gpu/bad-payload",
            "gfx/gpu::destroy-resource payload must include :id".to_string(),
            Some(op),
        );
    };
    if runtime.resources.remove(&id).is_none() {
        return mk_error(
            error_tok,
            "gfx/gpu/not-found",
            format!("unknown gpu resource id: {id}"),
            Some(op),
        );
    }
    Value::Data(map_term([
        (":ok", Term::Bool(true)),
        (":id", Term::Str(id)),
    ]))
}

fn write_buffer(
    runtime: &mut GpuHostRuntime,
    payload: &Term,
    error_tok: SealId,
    op: &str,
) -> Value {
    let Some(id) = map_field_str_or_symbol(payload, ":id") else {
        return mk_error(
            error_tok,
            "gfx/gpu/bad-payload",
            "gfx/gpu::write-buffer payload must include :id".to_string(),
            Some(op),
        );
    };
    let offset = map_field_usize(payload, ":offset").unwrap_or(0);
    let Some(data) = map_field_bytes(payload, ":data") else {
        return mk_error(
            error_tok,
            "gfx/gpu/bad-payload",
            "gfx/gpu::write-buffer payload must include :data bytes".to_string(),
            Some(op),
        );
    };
    let Some(GpuResource::Buffer(buf)) = runtime.resources.get_mut(&id) else {
        return mk_error(
            error_tok,
            "gfx/gpu/type-error",
            format!("resource is not a buffer: {id}"),
            Some(op),
        );
    };
    let end = offset.saturating_add(data.len());
    if end > buf.bytes.len() {
        return mk_error(
            error_tok,
            "gfx/gpu/out-of-bounds",
            format!(
                "write range exceeds buffer size (end={end}, len={})",
                buf.bytes.len()
            ),
            Some(op),
        );
    }
    buf.bytes[offset..end].copy_from_slice(&data);
    Value::Data(map_term([
        (":ok", Term::Bool(true)),
        (":id", Term::Str(id)),
        (":offset", Term::Int(BigInt::from(offset))),
        (":written", Term::Int(BigInt::from(data.len()))),
    ]))
}

fn read_buffer(runtime: &mut GpuHostRuntime, payload: &Term, error_tok: SealId, op: &str) -> Value {
    let Some(id) = map_field_str_or_symbol(payload, ":id") else {
        return mk_error(
            error_tok,
            "gfx/gpu/bad-payload",
            "gfx/gpu::read-buffer payload must include :id".to_string(),
            Some(op),
        );
    };
    let offset = map_field_usize(payload, ":offset").unwrap_or(0);
    let size = map_field_usize(payload, ":size").unwrap_or(0);
    let Some(GpuResource::Buffer(buf)) = runtime.resources.get(&id) else {
        return mk_error(
            error_tok,
            "gfx/gpu/type-error",
            format!("resource is not a buffer: {id}"),
            Some(op),
        );
    };
    let end = offset.saturating_add(size);
    if end > buf.bytes.len() {
        return mk_error(
            error_tok,
            "gfx/gpu/out-of-bounds",
            format!(
                "read range exceeds buffer size (end={end}, len={})",
                buf.bytes.len()
            ),
            Some(op),
        );
    }
    let out = buf.bytes[offset..end].to_vec();
    Value::Data(map_term([
        (":ok", Term::Bool(true)),
        (":id", Term::Str(id)),
        (":data", Term::Bytes(out.into())),
    ]))
}

fn write_texture(
    runtime: &mut GpuHostRuntime,
    payload: &Term,
    error_tok: SealId,
    op: &str,
) -> Value {
    let Some(id) = map_field_str_or_symbol(payload, ":id") else {
        return mk_error(
            error_tok,
            "gfx/gpu/bad-payload",
            "gfx/gpu::write-texture payload must include :id".to_string(),
            Some(op),
        );
    };
    let Some(data) = map_field_bytes(payload, ":data") else {
        return mk_error(
            error_tok,
            "gfx/gpu/bad-payload",
            "gfx/gpu::write-texture payload must include :data bytes".to_string(),
            Some(op),
        );
    };
    let Some(GpuResource::Texture(tex)) = runtime.resources.get_mut(&id) else {
        return mk_error(
            error_tok,
            "gfx/gpu/type-error",
            format!("resource is not a texture: {id}"),
            Some(op),
        );
    };
    if data.len() > GPU_RESOURCE_MAX_BYTES {
        return mk_error(
            error_tok,
            "core/caps/resource-limit",
            format!(
                "texture write exceeds host limit ({} > {GPU_RESOURCE_MAX_BYTES})",
                data.len()
            ),
            Some(op),
        );
    }
    tex.bytes = data;
    Value::Data(map_term([
        (":ok", Term::Bool(true)),
        (":id", Term::Str(id)),
        (":written", Term::Int(BigInt::from(tex.bytes.len()))),
    ]))
}

fn read_texture(
    runtime: &mut GpuHostRuntime,
    payload: &Term,
    error_tok: SealId,
    op: &str,
) -> Value {
    let Some(id) = map_field_str_or_symbol(payload, ":id") else {
        return mk_error(
            error_tok,
            "gfx/gpu/bad-payload",
            "gfx/gpu::read-texture payload must include :id".to_string(),
            Some(op),
        );
    };
    let Some(GpuResource::Texture(tex)) = runtime.resources.get(&id) else {
        return mk_error(
            error_tok,
            "gfx/gpu/type-error",
            format!("resource is not a texture: {id}"),
            Some(op),
        );
    };
    let region = map_field(payload, ":region");
    let offset = region
        .and_then(|r| map_field_usize(r, ":offset"))
        .unwrap_or(0);
    let size = region
        .and_then(|r| map_field_usize(r, ":size"))
        .unwrap_or(tex.bytes.len().saturating_sub(offset));
    let end = offset.saturating_add(size);
    if end > tex.bytes.len() {
        return mk_error(
            error_tok,
            "gfx/gpu/out-of-bounds",
            format!(
                "read region exceeds texture size (end={end}, len={})",
                tex.bytes.len()
            ),
            Some(op),
        );
    }
    let out = tex.bytes[offset..end].to_vec();
    Value::Data(map_term([
        (":ok", Term::Bool(true)),
        (":id", Term::Str(id)),
        (":data", Term::Bytes(out.into())),
    ]))
}

fn submit_graph(
    runtime: &mut GpuHostRuntime,
    payload: &Term,
    kind: &str,
    error_tok: SealId,
    op: &str,
) -> Value {
    let Some(graph) = map_field(payload, ":graph") else {
        return mk_error(
            error_tok,
            "gfx/gpu/bad-payload",
            format!("{op} payload must include :graph"),
            Some(op),
        );
    };
    runtime.submit_seq = runtime.submit_seq.saturating_add(1);
    let graph_h = hash_term(graph);
    let submission_id = format!("gpu-submit-{:016x}", runtime.submit_seq);
    let (resource_count, inventory_h) = resource_inventory(runtime);
    Value::Data(map_term([
        (":ok", Term::Bool(true)),
        (":submission-id", Term::Str(submission_id)),
        (":kind", Term::Symbol(format!(":{kind}"))),
        (":graph-h", Term::Str(hex_lower(&graph_h))),
        (":resource-count", Term::Int(BigInt::from(resource_count))),
        (":inventory-h", Term::Str(inventory_h)),
    ]))
}

fn gpu_limits() -> Value {
    Value::Data(map_term([
        (
            ":max-buffer-bytes",
            Term::Int(BigInt::from(GPU_RESOURCE_MAX_BYTES)),
        ),
        (
            ":max-texture-bytes",
            Term::Int(BigInt::from(GPU_RESOURCE_MAX_BYTES)),
        ),
        (":max-bind-groups", Term::Int(BigInt::from(8))),
        (":max-color-attachments", Term::Int(BigInt::from(8))),
    ]))
}

fn gpu_features() -> Value {
    Value::Data(map_term([(
        ":features",
        Term::Vector(vec![
            Term::symbol(":compute"),
            Term::symbol(":render"),
            Term::symbol(":storage-buffer"),
            Term::symbol(":texture-readback"),
        ]),
    )]))
}

fn alloc_id(runtime: &mut GpuHostRuntime, suffix: &str) -> String {
    let id = format!("gpu-{suffix}-{:016x}", runtime.next_id);
    runtime.next_id = runtime.next_id.saturating_add(1);
    id
}

fn map_term<const N: usize>(pairs: [(&str, Term); N]) -> Term {
    Term::Map(
        pairs
            .into_iter()
            .map(|(k, v)| (TermOrdKey(Term::symbol(k)), v))
            .collect(),
    )
}

fn map_field<'a>(t: &'a Term, key: &str) -> Option<&'a Term> {
    let Term::Map(m) = t else {
        return None;
    };
    m.get(&TermOrdKey(Term::symbol(key)))
}

fn map_field_str_or_symbol(t: &Term, key: &str) -> Option<String> {
    match map_field(t, key) {
        Some(Term::Str(s)) => Some(s.clone()),
        Some(Term::Symbol(s)) => Some(s.clone()),
        _ => None,
    }
}

fn map_field_usize(t: &Term, key: &str) -> Option<usize> {
    let Some(Term::Int(i)) = map_field(t, key) else {
        return None;
    };
    if i.sign() == num_bigint::Sign::Minus {
        return None;
    }
    i.to_usize()
}

fn map_field_bytes(t: &Term, key: &str) -> Option<Vec<u8>> {
    match map_field(t, key) {
        Some(Term::Bytes(b)) => Some(b.as_ref().to_vec()),
        _ => None,
    }
}

fn infer_texture_bytes(desc: &Term) -> usize {
    let Some(extent) = map_field(desc, ":extent") else {
        return 0;
    };
    let width = map_field_usize(extent, ":width").unwrap_or(0);
    let height = map_field_usize(extent, ":height").unwrap_or(0);
    let depth = map_field_usize(extent, ":depth").unwrap_or(1);
    width
        .saturating_mul(height)
        .saturating_mul(depth)
        .saturating_mul(4)
}

fn mk_error(error_tok: SealId, code: &str, msg: String, op: Option<&str>) -> Value {
    let mut mm = BTreeMap::new();
    mm.insert(
        TermOrdKey(Term::symbol(":error/code")),
        Term::Str(code.to_string()),
    );
    mm.insert(TermOrdKey(Term::symbol(":error/message")), Term::Str(msg));
    mm.insert(
        TermOrdKey(Term::symbol(":error/op")),
        op.map(Term::symbol).unwrap_or(Term::Nil),
    );
    Value::Sealed {
        token: error_tok,
        payload: Box::new(Value::Data(Term::Map(mm))),
    }
}

fn resource_inventory(runtime: &GpuHostRuntime) -> (usize, String) {
    let mut acc = [0u8; 32];
    for resource in runtime.resources.values() {
        let h = match resource {
            GpuResource::Buffer(BufferResource { desc, .. })
            | GpuResource::Texture(TextureResource { desc, .. })
            | GpuResource::Sampler { desc }
            | GpuResource::ShaderModule { desc }
            | GpuResource::BindGroupLayout { desc }
            | GpuResource::BindGroup { desc }
            | GpuResource::PipelineLayout { desc }
            | GpuResource::RenderPipeline { desc }
            | GpuResource::ComputePipeline { desc } => hash_term(desc),
        };
        for (i, b) in h.iter().enumerate() {
            acc[i] ^= *b;
        }
    }
    (runtime.resources.len(), hex_lower(&acc))
}

fn hex_lower(bytes: &[u8; 32]) -> String {
    let mut out = String::with_capacity(64);
    for b in bytes {
        use std::fmt::Write as _;
        let _ = write!(&mut out, "{b:02x}");
    }
    out
}
