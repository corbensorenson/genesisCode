use std::collections::BTreeMap;

use gc_coreform::{Term, TermOrdKey};
use gc_kernel::{SealId, Value};
use num_traits::ToPrimitive;

use crate::policy::OpPolicy;
use crate::runner_gpu_backend_policy::{
    GpuBackendFallbackPolicy, GpuBackendKind, gpu_backend_fallback_policy, gpu_backend_kind,
    gpu_backend_kind_label, gpu_op_prefers_device_backend, inject_backend_fallback_metadata,
};
use crate::runner_gpu_device_backend::call_device_backend;
use crate::runner_host_bridge::{BridgeError, call_host_bridge};

const FIRST_PARTY_GPU_BUFFER_MAX_BYTES: usize = 8 * 1024 * 1024;
const FIRST_PARTY_GPU_TEXTURE_MAX_BYTES: usize = 16 * 1024 * 1024;

#[derive(Debug, Clone)]
struct GpuBufferState {
    bytes: Vec<u8>,
}

#[derive(Debug, Clone)]
struct GpuTextureState {
    bytes: Vec<u8>,
}

#[derive(Debug, Clone)]
enum GpuResourceState {
    Buffer(GpuBufferState),
    Texture(GpuTextureState),
    Opaque { kind: String, payload_h: String },
}

#[derive(Debug, Clone, Default)]
pub(crate) struct GpuHostRuntime {
    next_resource: u64,
    resources: BTreeMap<String, GpuResourceState>,
}

pub(crate) fn gpu_host_call(
    runtime: &mut GpuHostRuntime,
    op: &str,
    payload: &Term,
    pol: Option<&OpPolicy>,
    error_tok: SealId,
) -> Option<Value> {
    if !is_gpu_host_op(op) {
        return None;
    }
    if !has_explicit_bridge_profile(pol) {
        let backend_kind = gpu_backend_kind(pol);
        if backend_kind != GpuBackendKind::FirstParty
            && gpu_op_prefers_device_backend(op, backend_kind)
        {
            return Some(match call_device_backend(op, payload) {
                Ok(resp) => Value::Data(resp),
                Err(err) => match gpu_backend_fallback_policy(pol) {
                    GpuBackendFallbackPolicy::RequireDevice => mk_error(
                        error_tok,
                        &BridgeError {
                            code: err.code,
                            message: err.message,
                        },
                        Some(op),
                    ),
                    GpuBackendFallbackPolicy::AllowFallback => {
                        let fallback = first_party_gpu_response(runtime, op, payload);
                        let decorated = inject_backend_fallback_metadata(
                            fallback,
                            gpu_backend_kind_label(backend_kind),
                            &err.message,
                        );
                        Value::Data(decorated)
                    }
                },
            });
        }
        return Some(Value::Data(first_party_gpu_response(runtime, op, payload)));
    }
    Some(match call_host_bridge("gpu", op, payload, pol) {
        Ok(resp) => Value::Data(resp),
        Err(err) => mk_error(error_tok, &err, Some(op)),
    })
}

fn has_explicit_bridge_profile(pol: Option<&OpPolicy>) -> bool {
    let Some(pol) = pol else {
        return false;
    };
    let has_nonempty_str = |key: &str| {
        pol.extra
            .get(key)
            .and_then(|v| v.as_str())
            .is_some_and(|s| !s.trim().is_empty())
    };
    has_nonempty_str("bridge_cmd")
        || has_nonempty_str("wasi_bridge_response")
        || has_nonempty_str("wasi_bridge_response_file")
        || pol
            .extra
            .get("wasi_bridge_profile")
            .and_then(|v| v.as_bool())
            .unwrap_or(false)
}

fn first_party_gpu_response(runtime: &mut GpuHostRuntime, op: &str, payload: &Term) -> Term {
    match op {
        "gpu/compute::create-buffer" | "gfx/gpu::create-buffer" => {
            first_party_create_buffer(runtime, op, payload)
        }
        "gfx/gpu::create-texture" => first_party_create_texture(runtime, op, payload),
        "gfx/gpu::create-sampler" => first_party_create_opaque(runtime, op, payload, "sampler"),
        "gpu/compute::create-shader-module" | "gfx/gpu::create-shader-module" => {
            first_party_create_opaque(runtime, op, payload, "shader-module")
        }
        "gpu/compute::create-bind-group-layout" | "gfx/gpu::create-bind-group-layout" => {
            first_party_create_opaque(runtime, op, payload, "bind-group-layout")
        }
        "gpu/compute::create-bind-group" | "gfx/gpu::create-bind-group" => {
            first_party_create_opaque(runtime, op, payload, "bind-group")
        }
        "gpu/compute::create-pipeline-layout" | "gfx/gpu::create-pipeline-layout" => {
            first_party_create_opaque(runtime, op, payload, "pipeline-layout")
        }
        "gpu/compute::create-compute-pipeline" | "gpu/compute::create-kernel" => {
            first_party_create_opaque(runtime, op, payload, "compute-pipeline")
        }
        "gfx/gpu::create-render-pipeline" => {
            first_party_create_opaque(runtime, op, payload, "render-pipeline")
        }
        "gpu/compute::write-buffer" | "gfx/gpu::write-buffer" => {
            first_party_write_buffer(runtime, op, payload)
        }
        "gpu/compute::read-buffer" | "gfx/gpu::read-buffer" => {
            first_party_read_buffer(runtime, op, payload)
        }
        "gfx/gpu::write-texture" => first_party_write_texture(runtime, op, payload),
        "gfx/gpu::read-texture" => first_party_read_texture(runtime, op, payload),
        "gpu/compute::destroy-resource" | "gfx/gpu::destroy-resource" => {
            first_party_destroy_resource(runtime, op, payload)
        }
        "gpu/compute::limits" => first_party_compute_limits(),
        "gfx/gpu::limits" => first_party_gfx_limits(),
        "gpu/compute::features" => first_party_compute_features(),
        "gfx/gpu::features" => first_party_gfx_features(),
        "gpu/compute::submit" => first_party_submit(payload, "gpu-compute-submit"),
        "gfx/gpu::submit-frame-graph" => first_party_submit(payload, "gfx-frame-submit"),
        _ => first_party_error(op, "unsupported-op"),
    }
}

fn first_party_create_buffer(runtime: &mut GpuHostRuntime, op: &str, payload: &Term) -> Term {
    let requested_size = payload_desc_size(payload).unwrap_or(0);
    if requested_size > FIRST_PARTY_GPU_BUFFER_MAX_BYTES {
        return first_party_error(op, "buffer-size-too-large");
    }
    let id = alloc_resource_id(runtime, "buffer");
    runtime.resources.insert(
        id.clone(),
        GpuResourceState::Buffer(GpuBufferState {
            bytes: vec![0_u8; requested_size],
        }),
    );
    map_term(vec![
        (":ok", Term::Bool(true)),
        (":backend", Term::Str("first-party-runtime".to_string())),
        (":id", Term::Str(id)),
        (":kind", Term::symbol(":buffer")),
        (":size", Term::Int((requested_size as i64).into())),
    ])
}

fn first_party_create_texture(runtime: &mut GpuHostRuntime, op: &str, payload: &Term) -> Term {
    let requested_size = payload_texture_byte_size(payload).unwrap_or(0);
    if requested_size > FIRST_PARTY_GPU_TEXTURE_MAX_BYTES {
        return first_party_error(op, "texture-size-too-large");
    }
    let id = alloc_resource_id(runtime, "texture");
    runtime.resources.insert(
        id.clone(),
        GpuResourceState::Texture(GpuTextureState {
            bytes: vec![0_u8; requested_size],
        }),
    );
    map_term(vec![
        (":ok", Term::Bool(true)),
        (":backend", Term::Str("first-party-runtime".to_string())),
        (":id", Term::Str(id)),
        (":kind", Term::symbol(":texture")),
        (":byte-size", Term::Int((requested_size as i64).into())),
    ])
}

fn first_party_create_opaque(
    runtime: &mut GpuHostRuntime,
    op: &str,
    payload: &Term,
    kind: &str,
) -> Term {
    let id = alloc_resource_id(runtime, kind);
    let payload_h = blake3::hash(gc_coreform::print_term(payload).as_bytes())
        .to_hex()
        .to_string();
    runtime.resources.insert(
        id.clone(),
        GpuResourceState::Opaque {
            kind: kind.to_string(),
            payload_h: payload_h.clone(),
        },
    );
    map_term(vec![
        (":ok", Term::Bool(true)),
        (":backend", Term::Str("first-party-runtime".to_string())),
        (":id", Term::Str(id)),
        (":kind", Term::Str(kind.to_string())),
        (":op", Term::symbol(op)),
        (":payload-h", Term::Str(payload_h)),
    ])
}

fn first_party_write_buffer(runtime: &mut GpuHostRuntime, op: &str, payload: &Term) -> Term {
    let Some(map) = payload_map(payload) else {
        return first_party_error(op, "invalid-payload");
    };
    let Some(id) = map_get_string(map, ":id").or_else(|| map_get_string(map, ":resource")) else {
        return first_party_error(op, "missing-resource-id");
    };
    let offset = map_get_nonnegative_usize(map, ":offset").unwrap_or(0);
    let write_bytes = map
        .get(&TermOrdKey(Term::symbol(":data")))
        .and_then(term_to_bytes)
        .unwrap_or_default();
    let Some(resource) = runtime.resources.get_mut(&id) else {
        return first_party_error(op, "resource-not-found");
    };
    let GpuResourceState::Buffer(buffer) = resource else {
        return first_party_error(op, "not-buffer");
    };
    let end = offset.saturating_add(write_bytes.len());
    if offset > buffer.bytes.len() || end > buffer.bytes.len() {
        return first_party_error(op, "write-out-of-bounds");
    }
    buffer.bytes[offset..end].copy_from_slice(&write_bytes);
    map_term(vec![
        (":ok", Term::Bool(true)),
        (":backend", Term::Str("first-party-runtime".to_string())),
        (":id", Term::Str(id)),
        (":offset", Term::Int((offset as i64).into())),
        (":written", Term::Int((write_bytes.len() as i64).into())),
    ])
}

fn first_party_read_buffer(runtime: &mut GpuHostRuntime, op: &str, payload: &Term) -> Term {
    let Some(map) = payload_map(payload) else {
        return first_party_error(op, "invalid-payload");
    };
    let Some(id) = map_get_string(map, ":id").or_else(|| map_get_string(map, ":resource")) else {
        return first_party_error(op, "missing-resource-id");
    };
    let offset = map_get_nonnegative_usize(map, ":offset").unwrap_or(0);
    let Some(resource) = runtime.resources.get(&id) else {
        return first_party_error(op, "resource-not-found");
    };
    let GpuResourceState::Buffer(buffer) = resource else {
        return first_party_error(op, "not-buffer");
    };
    if offset > buffer.bytes.len() {
        return first_party_error(op, "read-out-of-bounds");
    }
    let requested_size =
        map_get_nonnegative_usize(map, ":size").unwrap_or(buffer.bytes.len() - offset);
    let end = offset.saturating_add(requested_size);
    if end > buffer.bytes.len() {
        return first_party_error(op, "read-out-of-bounds");
    }
    map_term(vec![
        (":ok", Term::Bool(true)),
        (":backend", Term::Str("first-party-runtime".to_string())),
        (":id", Term::Str(id)),
        (":offset", Term::Int((offset as i64).into())),
        (":size", Term::Int((requested_size as i64).into())),
        (
            ":data",
            Term::Bytes(buffer.bytes[offset..end].to_vec().into()),
        ),
    ])
}

fn first_party_write_texture(runtime: &mut GpuHostRuntime, op: &str, payload: &Term) -> Term {
    let Some(map) = payload_map(payload) else {
        return first_party_error(op, "invalid-payload");
    };
    let Some(id) = map_get_string(map, ":id").or_else(|| map_get_string(map, ":resource")) else {
        return first_party_error(op, "missing-resource-id");
    };
    let offset = map
        .get(&TermOrdKey(Term::symbol(":region")))
        .and_then(payload_map)
        .and_then(|region| map_get_nonnegative_usize(region, ":offset"))
        .or_else(|| map_get_nonnegative_usize(map, ":offset"))
        .unwrap_or(0);
    let write_bytes = map
        .get(&TermOrdKey(Term::symbol(":data")))
        .and_then(term_to_bytes)
        .unwrap_or_default();
    let Some(resource) = runtime.resources.get_mut(&id) else {
        return first_party_error(op, "resource-not-found");
    };
    let GpuResourceState::Texture(texture) = resource else {
        return first_party_error(op, "not-texture");
    };
    let end = offset.saturating_add(write_bytes.len());
    if offset > texture.bytes.len() || end > texture.bytes.len() {
        return first_party_error(op, "write-out-of-bounds");
    }
    texture.bytes[offset..end].copy_from_slice(&write_bytes);
    map_term(vec![
        (":ok", Term::Bool(true)),
        (":backend", Term::Str("first-party-runtime".to_string())),
        (":id", Term::Str(id)),
        (":offset", Term::Int((offset as i64).into())),
        (":written", Term::Int((write_bytes.len() as i64).into())),
    ])
}

fn first_party_read_texture(runtime: &mut GpuHostRuntime, op: &str, payload: &Term) -> Term {
    let Some(map) = payload_map(payload) else {
        return first_party_error(op, "invalid-payload");
    };
    let Some(id) = map_get_string(map, ":id").or_else(|| map_get_string(map, ":resource")) else {
        return first_party_error(op, "missing-resource-id");
    };
    let offset = map
        .get(&TermOrdKey(Term::symbol(":region")))
        .and_then(payload_map)
        .and_then(|region| map_get_nonnegative_usize(region, ":offset"))
        .or_else(|| map_get_nonnegative_usize(map, ":offset"))
        .unwrap_or(0);
    let Some(resource) = runtime.resources.get(&id) else {
        return first_party_error(op, "resource-not-found");
    };
    let GpuResourceState::Texture(texture) = resource else {
        return first_party_error(op, "not-texture");
    };
    if offset > texture.bytes.len() {
        return first_party_error(op, "read-out-of-bounds");
    }
    let requested_size = map
        .get(&TermOrdKey(Term::symbol(":region")))
        .and_then(payload_map)
        .and_then(|region| map_get_nonnegative_usize(region, ":size"))
        .or_else(|| map_get_nonnegative_usize(map, ":size"))
        .unwrap_or(texture.bytes.len() - offset);
    let end = offset.saturating_add(requested_size);
    if end > texture.bytes.len() {
        return first_party_error(op, "read-out-of-bounds");
    }
    map_term(vec![
        (":ok", Term::Bool(true)),
        (":backend", Term::Str("first-party-runtime".to_string())),
        (":id", Term::Str(id)),
        (":offset", Term::Int((offset as i64).into())),
        (":size", Term::Int((requested_size as i64).into())),
        (
            ":data",
            Term::Bytes(texture.bytes[offset..end].to_vec().into()),
        ),
    ])
}

fn first_party_destroy_resource(runtime: &mut GpuHostRuntime, op: &str, payload: &Term) -> Term {
    let Some(map) = payload_map(payload) else {
        return first_party_error(op, "invalid-payload");
    };
    let Some(id) = map_get_string(map, ":id").or_else(|| map_get_string(map, ":resource")) else {
        return first_party_error(op, "missing-resource-id");
    };
    let Some(resource) = runtime.resources.remove(&id) else {
        return first_party_error(op, "resource-not-found");
    };
    let kind = match resource {
        GpuResourceState::Buffer(_) => Term::symbol(":buffer"),
        GpuResourceState::Texture(_) => Term::symbol(":texture"),
        GpuResourceState::Opaque { kind, payload_h } => {
            return map_term(vec![
                (":ok", Term::Bool(true)),
                (":backend", Term::Str("first-party-runtime".to_string())),
                (":id", Term::Str(id)),
                (":kind", Term::Str(kind)),
                (":payload-h", Term::Str(payload_h)),
                (":destroyed", Term::Bool(true)),
            ]);
        }
    };
    map_term(vec![
        (":ok", Term::Bool(true)),
        (":backend", Term::Str("first-party-runtime".to_string())),
        (":id", Term::Str(id)),
        (":kind", kind),
        (":destroyed", Term::Bool(true)),
    ])
}

fn first_party_compute_limits() -> Term {
    map_term(vec![
        (":ok", Term::Bool(true)),
        (":backend", Term::Str("first-party-runtime".to_string())),
        (":max-workgroup-size", Term::Int(256.into())),
        (":max-bind-groups", Term::Int(8.into())),
    ])
}

fn first_party_gfx_limits() -> Term {
    map_term(vec![
        (":ok", Term::Bool(true)),
        (":backend", Term::Str("first-party-runtime".to_string())),
        (
            ":max-buffer-bytes",
            Term::Int((FIRST_PARTY_GPU_BUFFER_MAX_BYTES as i64).into()),
        ),
        (
            ":max-texture-bytes",
            Term::Int((FIRST_PARTY_GPU_TEXTURE_MAX_BYTES as i64).into()),
        ),
        (":max-bind-groups", Term::Int(8.into())),
        (":max-render-targets", Term::Int(8.into())),
    ])
}

fn first_party_compute_features() -> Term {
    map_term(vec![
        (":ok", Term::Bool(true)),
        (":backend", Term::Str("first-party-runtime".to_string())),
        (
            ":features",
            Term::Vector(vec![
                Term::symbol(":deterministic-replay"),
                Term::symbol(":canonical-submit"),
            ]),
        ),
    ])
}

fn first_party_gfx_features() -> Term {
    map_term(vec![
        (":ok", Term::Bool(true)),
        (":backend", Term::Str("first-party-runtime".to_string())),
        (
            ":features",
            Term::Vector(vec![
                Term::symbol(":deterministic-replay"),
                Term::symbol(":canonical-submit"),
                Term::symbol(":frame-graph"),
                Term::symbol(":texture-readback"),
                Term::symbol(":compute-interop"),
            ]),
        ),
    ])
}

fn first_party_submit(payload: &Term, kind: &str) -> Term {
    let payload_src = gc_coreform::print_term(payload);
    let payload_h = blake3::hash(payload_src.as_bytes()).to_hex().to_string();
    map_term(vec![
        (":ok", Term::Bool(true)),
        (":kind", Term::Str(kind.to_string())),
        (":backend", Term::Str("first-party-runtime".to_string())),
        (":payload-h", Term::Str(payload_h)),
    ])
}

fn alloc_resource_id(runtime: &mut GpuHostRuntime, kind: &str) -> String {
    runtime.next_resource = runtime.next_resource.saturating_add(1);
    format!("gpu-first-party-{kind}-{}", runtime.next_resource)
}

fn payload_desc_size(payload: &Term) -> Option<usize> {
    let payload = payload_map(payload)?;
    payload
        .get(&TermOrdKey(Term::symbol(":desc")))
        .and_then(payload_map)
        .and_then(|desc| map_get_nonnegative_usize(desc, ":size"))
        .or_else(|| map_get_nonnegative_usize(payload, ":size"))
}

fn payload_texture_byte_size(payload: &Term) -> Option<usize> {
    let payload = payload_map(payload)?;
    payload
        .get(&TermOrdKey(Term::symbol(":desc")))
        .and_then(payload_map)
        .and_then(|desc| map_get_nonnegative_usize(desc, ":byte-size"))
        .or_else(|| {
            payload
                .get(&TermOrdKey(Term::symbol(":desc")))
                .and_then(payload_map)
                .and_then(|desc| map_get_nonnegative_usize(desc, ":size"))
        })
        .or_else(|| map_get_nonnegative_usize(payload, ":byte-size"))
        .or_else(|| map_get_nonnegative_usize(payload, ":size"))
}

fn payload_map(payload: &Term) -> Option<&BTreeMap<TermOrdKey, Term>> {
    match payload {
        Term::Map(map) => Some(map),
        _ => None,
    }
}

fn map_get_string(map: &BTreeMap<TermOrdKey, Term>, key: &str) -> Option<String> {
    map.get(&TermOrdKey(Term::symbol(key)))
        .and_then(|value| match value {
            Term::Str(text) => Some(text.clone()),
            Term::Symbol(sym) => Some(sym.clone()),
            _ => None,
        })
}

fn map_get_nonnegative_usize(map: &BTreeMap<TermOrdKey, Term>, key: &str) -> Option<usize> {
    map.get(&TermOrdKey(Term::symbol(key)))
        .and_then(|value| match value {
            Term::Int(v) => v.to_usize(),
            _ => None,
        })
}

fn term_to_bytes(term: &Term) -> Option<Vec<u8>> {
    match term {
        Term::Bytes(bytes) => Some(bytes.to_vec()),
        Term::Str(text) => Some(text.as_bytes().to_vec()),
        Term::Vector(values) => {
            let mut out = Vec::with_capacity(values.len());
            for value in values {
                let Term::Int(n) = value else {
                    return None;
                };
                let byte = n.to_u8()?;
                out.push(byte);
            }
            Some(out)
        }
        _ => None,
    }
}

fn first_party_error(op: &str, suffix: &str) -> Term {
    let code = if op.starts_with("gfx/gpu::") {
        format!("gfx/first-party-{suffix}")
    } else {
        format!("gpu/first-party-{suffix}")
    };
    map_term(vec![
        (":ok", Term::Bool(false)),
        (":backend", Term::Str("first-party-runtime".to_string())),
        (":error/code", Term::Str(code)),
        (":error/op", Term::symbol(op)),
    ])
}

fn map_term(entries: Vec<(&str, Term)>) -> Term {
    Term::Map(
        entries
            .into_iter()
            .map(|(key, value)| (TermOrdKey(Term::symbol(key)), value))
            .collect(),
    )
}

fn is_gpu_host_op(op: &str) -> bool {
    matches!(
        op,
        "gfx/gpu::create-buffer"
            | "gpu/compute::create-buffer"
            | "gfx/gpu::create-texture"
            | "gfx/gpu::create-sampler"
            | "gfx/gpu::create-shader-module"
            | "gpu/compute::create-shader-module"
            | "gfx/gpu::create-bind-group-layout"
            | "gpu/compute::create-bind-group-layout"
            | "gfx/gpu::create-bind-group"
            | "gpu/compute::create-bind-group"
            | "gfx/gpu::create-pipeline-layout"
            | "gpu/compute::create-pipeline-layout"
            | "gfx/gpu::create-render-pipeline"
            | "gpu/compute::create-compute-pipeline"
            | "gpu/compute::create-kernel"
            | "gfx/gpu::destroy-resource"
            | "gpu/compute::destroy-resource"
            | "gfx/gpu::write-buffer"
            | "gpu/compute::write-buffer"
            | "gfx/gpu::write-texture"
            | "gfx/gpu::read-buffer"
            | "gpu/compute::read-buffer"
            | "gfx/gpu::read-texture"
            | "gfx/gpu::submit-frame-graph"
            | "gpu/compute::submit"
            | "gfx/gpu::limits"
            | "gpu/compute::limits"
            | "gfx/gpu::features"
            | "gpu/compute::features"
    )
}

fn mk_error(error_tok: SealId, err: &BridgeError, op: Option<&str>) -> Value {
    let mut mm = BTreeMap::new();
    mm.insert(
        TermOrdKey(Term::symbol(":error/code")),
        Term::Str(err.code.clone()),
    );
    mm.insert(
        TermOrdKey(Term::symbol(":error/message")),
        Term::Str(err.message.clone()),
    );
    mm.insert(
        TermOrdKey(Term::symbol(":error/op")),
        op.map(Term::symbol).unwrap_or(Term::Nil),
    );
    Value::Sealed {
        token: error_tok,
        payload: Box::new(Value::Data(Term::Map(mm))),
    }
}
