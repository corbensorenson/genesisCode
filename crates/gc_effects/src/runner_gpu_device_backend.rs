use gc_coreform::Term;

#[derive(Debug, Clone)]
pub(crate) struct DeviceBackendError {
    pub code: String,
    pub message: String,
}

pub(crate) fn call_device_backend(op: &str, payload: &Term) -> Result<Term, DeviceBackendError> {
    match canonical_device_op(op) {
        Some(DeviceOp::Submit(kind)) => device_submit_response(kind, op, payload),
        Some(DeviceOp::Create(kind)) => device_create_response(kind, op, payload),
        Some(DeviceOp::WriteBuffer) => device_write_buffer_response(op, payload),
        Some(DeviceOp::ReadBuffer) => device_read_buffer_response(op, payload),
        Some(DeviceOp::WriteTexture) => device_write_texture_response(op, payload),
        Some(DeviceOp::ReadTexture) => device_read_texture_response(op, payload),
        Some(DeviceOp::DestroyResource) => device_destroy_resource_response(op, payload),
        Some(DeviceOp::Limits) => device_limits_response(op),
        Some(DeviceOp::Features) => device_features_response(op),
        None => Err(DeviceBackendError {
            code: "gpu/device-backend-unsupported-op".to_string(),
            message: format!("device backend does not support op `{op}`"),
        }),
    }
}

#[derive(Debug, Clone, Copy)]
enum DeviceOp {
    Submit(&'static str),
    Create(DeviceResourceKind),
    WriteBuffer,
    ReadBuffer,
    WriteTexture,
    ReadTexture,
    DestroyResource,
    Limits,
    Features,
}

#[derive(Debug, Clone, Copy)]
enum DeviceResourceKind {
    Buffer,
    Texture,
    Sampler,
    ShaderModule,
    BindGroupLayout,
    BindGroup,
    PipelineLayout,
    ComputePipeline,
    RenderPipeline,
}

#[cfg(all(not(target_os = "wasi"), feature = "gpu-device-backend"))]
impl DeviceResourceKind {
    fn id_prefix(self) -> &'static str {
        match self {
            DeviceResourceKind::Buffer => "buffer",
            DeviceResourceKind::Texture => "texture",
            DeviceResourceKind::Sampler => "sampler",
            DeviceResourceKind::ShaderModule => "shader-module",
            DeviceResourceKind::BindGroupLayout => "bind-group-layout",
            DeviceResourceKind::BindGroup => "bind-group",
            DeviceResourceKind::PipelineLayout => "pipeline-layout",
            DeviceResourceKind::ComputePipeline => "compute-pipeline",
            DeviceResourceKind::RenderPipeline => "render-pipeline",
        }
    }

    fn kind_term(self) -> Term {
        match self {
            DeviceResourceKind::Buffer => Term::symbol(":buffer"),
            DeviceResourceKind::Texture => Term::symbol(":texture"),
            _ => Term::Str(self.id_prefix().to_string()),
        }
    }
}

fn canonical_device_op(op: &str) -> Option<DeviceOp> {
    match op {
        "gpu/compute::submit" => Some(DeviceOp::Submit("gpu-compute-submit")),
        "gfx/gpu::submit-frame-graph" => Some(DeviceOp::Submit("gfx-frame-submit")),
        "gpu/compute::create-buffer" | "gfx/gpu::create-buffer" => {
            Some(DeviceOp::Create(DeviceResourceKind::Buffer))
        }
        "gfx/gpu::create-texture" => Some(DeviceOp::Create(DeviceResourceKind::Texture)),
        "gfx/gpu::create-sampler" => Some(DeviceOp::Create(DeviceResourceKind::Sampler)),
        "gpu/compute::create-shader-module" | "gfx/gpu::create-shader-module" => {
            Some(DeviceOp::Create(DeviceResourceKind::ShaderModule))
        }
        "gpu/compute::create-bind-group-layout" | "gfx/gpu::create-bind-group-layout" => {
            Some(DeviceOp::Create(DeviceResourceKind::BindGroupLayout))
        }
        "gpu/compute::create-bind-group" | "gfx/gpu::create-bind-group" => {
            Some(DeviceOp::Create(DeviceResourceKind::BindGroup))
        }
        "gpu/compute::create-pipeline-layout" | "gfx/gpu::create-pipeline-layout" => {
            Some(DeviceOp::Create(DeviceResourceKind::PipelineLayout))
        }
        "gpu/compute::create-compute-pipeline" | "gpu/compute::create-kernel" => {
            Some(DeviceOp::Create(DeviceResourceKind::ComputePipeline))
        }
        "gfx/gpu::create-render-pipeline" => {
            Some(DeviceOp::Create(DeviceResourceKind::RenderPipeline))
        }
        "gpu/compute::write-buffer" | "gfx/gpu::write-buffer" => Some(DeviceOp::WriteBuffer),
        "gpu/compute::read-buffer" | "gfx/gpu::read-buffer" => Some(DeviceOp::ReadBuffer),
        "gfx/gpu::write-texture" => Some(DeviceOp::WriteTexture),
        "gfx/gpu::read-texture" => Some(DeviceOp::ReadTexture),
        "gpu/compute::destroy-resource" | "gfx/gpu::destroy-resource" => {
            Some(DeviceOp::DestroyResource)
        }
        "gpu/compute::limits" | "gfx/gpu::limits" => Some(DeviceOp::Limits),
        "gpu/compute::features" | "gfx/gpu::features" => Some(DeviceOp::Features),
        _ => None,
    }
}

#[cfg(all(not(target_os = "wasi"), feature = "gpu-device-backend"))]
mod imp {
    use std::collections::BTreeMap;
    use std::sync::{Mutex, OnceLock, mpsc};

    use bytemuck::cast_slice;
    use gc_coreform::{TermOrdKey, print_term};
    use num_traits::ToPrimitive;
    use wgpu::{Buffer, BufferUsages};

    use super::*;
    use crate::runner_gpu_backend_policy::GPU_BACKEND_DEVICE_RUNTIME;

    const SHADER_SRC: &str = r#"
@group(0) @binding(0) var<storage, read> inbuf: array<u32>;
@group(0) @binding(1) var<storage, read_write> outbuf: array<u32>;

@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
  let i = gid.x;
  if (i < arrayLength(&outbuf)) {
    outbuf[i] = inbuf[i] * 2u + 1u;
  }
}
"#;

    static DEVICE_CONTEXT: OnceLock<Result<DeviceContext, String>> = OnceLock::new();
    static DEVICE_RESOURCES: OnceLock<Mutex<DeviceResourceRuntime>> = OnceLock::new();

    const DEVICE_BUFFER_MAX_BYTES: usize = 8 * 1024 * 1024;
    const DEVICE_TEXTURE_MAX_BYTES: usize = 16 * 1024 * 1024;

    #[derive(Debug, Clone)]
    enum DeviceResourceState {
        Buffer(Vec<u8>),
        Texture(Vec<u8>),
        Opaque {
            kind: DeviceResourceKind,
            descriptor: Term,
        },
    }

    #[derive(Debug, Default)]
    struct DeviceResourceRuntime {
        next_id: u64,
        resources: BTreeMap<String, DeviceResourceState>,
    }

    pub(super) fn device_create_response(
        kind: DeviceResourceKind,
        op: &str,
        payload: &Term,
    ) -> Result<Term, DeviceBackendError> {
        let mut rt = device_resource_runtime()?;
        let id = alloc_resource_id(&mut rt, kind.id_prefix());
        match kind {
            DeviceResourceKind::Buffer => {
                let size = payload_desc_size(payload).unwrap_or(0);
                if size > DEVICE_BUFFER_MAX_BYTES {
                    return Ok(device_error(op, "buffer-size-too-large"));
                }
                rt.resources
                    .insert(id.clone(), DeviceResourceState::Buffer(vec![0_u8; size]));
                Ok(map_term(vec![
                    (":ok", Term::Bool(true)),
                    (
                        ":backend",
                        Term::Str(
                            crate::runner_gpu_backend_policy::GPU_BACKEND_DEVICE_RUNTIME_FULL
                                .to_string(),
                        ),
                    ),
                    (":id", Term::Str(id)),
                    (":kind", kind.kind_term()),
                    (":size", Term::Int((size as i64).into())),
                ]))
            }
            DeviceResourceKind::Texture => {
                let size = payload_texture_byte_size(payload).unwrap_or(0);
                if size > DEVICE_TEXTURE_MAX_BYTES {
                    return Ok(device_error(op, "texture-size-too-large"));
                }
                rt.resources
                    .insert(id.clone(), DeviceResourceState::Texture(vec![0_u8; size]));
                Ok(map_term(vec![
                    (":ok", Term::Bool(true)),
                    (
                        ":backend",
                        Term::Str(
                            crate::runner_gpu_backend_policy::GPU_BACKEND_DEVICE_RUNTIME_FULL
                                .to_string(),
                        ),
                    ),
                    (":id", Term::Str(id)),
                    (":kind", kind.kind_term()),
                    (":byte-size", Term::Int((size as i64).into())),
                ]))
            }
            _ => {
                let descriptor = payload_descriptor(payload);
                rt.resources.insert(
                    id.clone(),
                    DeviceResourceState::Opaque {
                        kind,
                        descriptor: descriptor.clone(),
                    },
                );
                Ok(map_term(vec![
                    (":ok", Term::Bool(true)),
                    (
                        ":backend",
                        Term::Str(
                            crate::runner_gpu_backend_policy::GPU_BACKEND_DEVICE_RUNTIME_FULL
                                .to_string(),
                        ),
                    ),
                    (":id", Term::Str(id)),
                    (":kind", kind.kind_term()),
                    (":descriptor", descriptor),
                ]))
            }
        }
    }

    pub(super) fn device_write_buffer_response(
        op: &str,
        payload: &Term,
    ) -> Result<Term, DeviceBackendError> {
        let Some(payload) = payload_map(payload) else {
            return Ok(device_error(op, "invalid-payload"));
        };
        let Some(id) =
            map_get_string(payload, ":id").or_else(|| map_get_string(payload, ":resource"))
        else {
            return Ok(device_error(op, "missing-resource-id"));
        };
        let offset = map_get_nonnegative_usize(payload, ":offset").unwrap_or(0);
        let write_bytes = payload
            .get(&TermOrdKey(Term::symbol(":data")))
            .and_then(term_to_bytes)
            .unwrap_or_default();

        let mut rt = device_resource_runtime()?;
        let Some(resource) = rt.resources.get_mut(&id) else {
            return Ok(device_error(op, "resource-not-found"));
        };
        let DeviceResourceState::Buffer(buffer) = resource else {
            return Ok(device_error(op, "not-buffer"));
        };
        let end = offset.saturating_add(write_bytes.len());
        if offset > buffer.len() || end > buffer.len() {
            return Ok(device_error(op, "write-out-of-bounds"));
        }
        buffer[offset..end].copy_from_slice(&write_bytes);
        Ok(map_term(vec![
            (":ok", Term::Bool(true)),
            (
                ":backend",
                Term::Str(
                    crate::runner_gpu_backend_policy::GPU_BACKEND_DEVICE_RUNTIME_FULL.to_string(),
                ),
            ),
            (":id", Term::Str(id)),
            (":offset", Term::Int((offset as i64).into())),
            (":written", Term::Int((write_bytes.len() as i64).into())),
        ]))
    }

    pub(super) fn device_read_buffer_response(
        op: &str,
        payload: &Term,
    ) -> Result<Term, DeviceBackendError> {
        let Some(payload) = payload_map(payload) else {
            return Ok(device_error(op, "invalid-payload"));
        };
        let Some(id) =
            map_get_string(payload, ":id").or_else(|| map_get_string(payload, ":resource"))
        else {
            return Ok(device_error(op, "missing-resource-id"));
        };
        let offset = map_get_nonnegative_usize(payload, ":offset").unwrap_or(0);

        let rt = device_resource_runtime()?;
        let Some(resource) = rt.resources.get(&id) else {
            return Ok(device_error(op, "resource-not-found"));
        };
        let DeviceResourceState::Buffer(buffer) = resource else {
            return Ok(device_error(op, "not-buffer"));
        };
        if offset > buffer.len() {
            return Ok(device_error(op, "read-out-of-bounds"));
        }
        let requested_size =
            map_get_nonnegative_usize(payload, ":size").unwrap_or(buffer.len() - offset);
        let end = offset.saturating_add(requested_size);
        if end > buffer.len() {
            return Ok(device_error(op, "read-out-of-bounds"));
        }
        Ok(map_term(vec![
            (":ok", Term::Bool(true)),
            (
                ":backend",
                Term::Str(
                    crate::runner_gpu_backend_policy::GPU_BACKEND_DEVICE_RUNTIME_FULL.to_string(),
                ),
            ),
            (":id", Term::Str(id)),
            (":offset", Term::Int((offset as i64).into())),
            (":size", Term::Int((requested_size as i64).into())),
            (":data", Term::Bytes(buffer[offset..end].to_vec().into())),
        ]))
    }

    pub(super) fn device_write_texture_response(
        op: &str,
        payload: &Term,
    ) -> Result<Term, DeviceBackendError> {
        let Some(payload) = payload_map(payload) else {
            return Ok(device_error(op, "invalid-payload"));
        };
        let Some(id) =
            map_get_string(payload, ":id").or_else(|| map_get_string(payload, ":resource"))
        else {
            return Ok(device_error(op, "missing-resource-id"));
        };
        let offset = payload
            .get(&TermOrdKey(Term::symbol(":region")))
            .and_then(payload_map)
            .and_then(|region| map_get_nonnegative_usize(region, ":offset"))
            .or_else(|| map_get_nonnegative_usize(payload, ":offset"))
            .unwrap_or(0);
        let write_bytes = payload
            .get(&TermOrdKey(Term::symbol(":data")))
            .and_then(term_to_bytes)
            .unwrap_or_default();

        let mut rt = device_resource_runtime()?;
        let Some(resource) = rt.resources.get_mut(&id) else {
            return Ok(device_error(op, "resource-not-found"));
        };
        let DeviceResourceState::Texture(texture) = resource else {
            return Ok(device_error(op, "not-texture"));
        };
        let end = offset.saturating_add(write_bytes.len());
        if offset > texture.len() || end > texture.len() {
            return Ok(device_error(op, "write-out-of-bounds"));
        }
        texture[offset..end].copy_from_slice(&write_bytes);
        Ok(map_term(vec![
            (":ok", Term::Bool(true)),
            (
                ":backend",
                Term::Str(
                    crate::runner_gpu_backend_policy::GPU_BACKEND_DEVICE_RUNTIME_FULL.to_string(),
                ),
            ),
            (":id", Term::Str(id)),
            (":offset", Term::Int((offset as i64).into())),
            (":written", Term::Int((write_bytes.len() as i64).into())),
        ]))
    }

    pub(super) fn device_read_texture_response(
        op: &str,
        payload: &Term,
    ) -> Result<Term, DeviceBackendError> {
        let Some(payload) = payload_map(payload) else {
            return Ok(device_error(op, "invalid-payload"));
        };
        let Some(id) =
            map_get_string(payload, ":id").or_else(|| map_get_string(payload, ":resource"))
        else {
            return Ok(device_error(op, "missing-resource-id"));
        };
        let offset = payload
            .get(&TermOrdKey(Term::symbol(":region")))
            .and_then(payload_map)
            .and_then(|region| map_get_nonnegative_usize(region, ":offset"))
            .or_else(|| map_get_nonnegative_usize(payload, ":offset"))
            .unwrap_or(0);

        let rt = device_resource_runtime()?;
        let Some(resource) = rt.resources.get(&id) else {
            return Ok(device_error(op, "resource-not-found"));
        };
        let DeviceResourceState::Texture(texture) = resource else {
            return Ok(device_error(op, "not-texture"));
        };
        if offset > texture.len() {
            return Ok(device_error(op, "read-out-of-bounds"));
        }
        let requested_size = payload
            .get(&TermOrdKey(Term::symbol(":region")))
            .and_then(payload_map)
            .and_then(|region| map_get_nonnegative_usize(region, ":size"))
            .or_else(|| map_get_nonnegative_usize(payload, ":size"))
            .unwrap_or(texture.len() - offset);
        let end = offset.saturating_add(requested_size);
        if end > texture.len() {
            return Ok(device_error(op, "read-out-of-bounds"));
        }
        Ok(map_term(vec![
            (":ok", Term::Bool(true)),
            (
                ":backend",
                Term::Str(
                    crate::runner_gpu_backend_policy::GPU_BACKEND_DEVICE_RUNTIME_FULL.to_string(),
                ),
            ),
            (":id", Term::Str(id)),
            (":offset", Term::Int((offset as i64).into())),
            (":size", Term::Int((requested_size as i64).into())),
            (":data", Term::Bytes(texture[offset..end].to_vec().into())),
        ]))
    }

    pub(super) fn device_destroy_resource_response(
        op: &str,
        payload: &Term,
    ) -> Result<Term, DeviceBackendError> {
        let Some(payload) = payload_map(payload) else {
            return Ok(device_error(op, "invalid-payload"));
        };
        let Some(id) =
            map_get_string(payload, ":id").or_else(|| map_get_string(payload, ":resource"))
        else {
            return Ok(device_error(op, "missing-resource-id"));
        };
        let mut rt = device_resource_runtime()?;
        let Some(resource) = rt.resources.remove(&id) else {
            return Ok(device_error(op, "resource-not-found"));
        };
        match resource {
            DeviceResourceState::Buffer(_) => Ok(map_term(vec![
                (":ok", Term::Bool(true)),
                (
                    ":backend",
                    Term::Str(
                        crate::runner_gpu_backend_policy::GPU_BACKEND_DEVICE_RUNTIME_FULL
                            .to_string(),
                    ),
                ),
                (":id", Term::Str(id)),
                (":kind", Term::symbol(":buffer")),
                (":destroyed", Term::Bool(true)),
            ])),
            DeviceResourceState::Texture(_) => Ok(map_term(vec![
                (":ok", Term::Bool(true)),
                (
                    ":backend",
                    Term::Str(
                        crate::runner_gpu_backend_policy::GPU_BACKEND_DEVICE_RUNTIME_FULL
                            .to_string(),
                    ),
                ),
                (":id", Term::Str(id)),
                (":kind", Term::symbol(":texture")),
                (":destroyed", Term::Bool(true)),
            ])),
            DeviceResourceState::Opaque { kind, descriptor } => Ok(map_term(vec![
                (":ok", Term::Bool(true)),
                (
                    ":backend",
                    Term::Str(
                        crate::runner_gpu_backend_policy::GPU_BACKEND_DEVICE_RUNTIME_FULL
                            .to_string(),
                    ),
                ),
                (":id", Term::Str(id)),
                (":kind", kind.kind_term()),
                (":descriptor", descriptor),
                (":destroyed", Term::Bool(true)),
            ])),
        }
    }

    pub(super) fn device_submit_response(
        kind: &str,
        op: &str,
        payload: &Term,
    ) -> Result<Term, DeviceBackendError> {
        if let Some(interop) = device_submit_via_resource_interop(kind, op, payload)? {
            return Ok(interop);
        }
        let ctx = device_context()?;
        let lanes = payload_u32_lanes(payload);
        let lane_count = lanes.len().max(1);
        let input = if lanes.is_empty() { vec![0_u32] } else { lanes };
        let in_bytes: &[u8] = cast_slice(&input);
        let byte_len = in_bytes.len() as u64;

        let inbuf = create_buffer(
            &ctx.device,
            "gc-effects-device-in",
            byte_len,
            BufferUsages::STORAGE | BufferUsages::COPY_DST,
        );
        ctx.queue.write_buffer(&inbuf, 0, in_bytes);
        let outbuf = create_buffer(
            &ctx.device,
            "gc-effects-device-out",
            byte_len,
            BufferUsages::STORAGE | BufferUsages::COPY_SRC | BufferUsages::COPY_DST,
        );
        let staging = create_buffer(
            &ctx.device,
            "gc-effects-device-staging",
            byte_len,
            BufferUsages::MAP_READ | BufferUsages::COPY_DST,
        );

        let shader = ctx
            .device
            .create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("gc-effects-device-shader"),
                source: wgpu::ShaderSource::Wgsl(SHADER_SRC.into()),
            });
        let bgl = ctx
            .device
            .create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("gc-effects-device-bgl"),
                entries: &[
                    wgpu::BindGroupLayoutEntry {
                        binding: 0,
                        visibility: wgpu::ShaderStages::COMPUTE,
                        ty: wgpu::BindingType::Buffer {
                            ty: wgpu::BufferBindingType::Storage { read_only: true },
                            has_dynamic_offset: false,
                            min_binding_size: None,
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 1,
                        visibility: wgpu::ShaderStages::COMPUTE,
                        ty: wgpu::BindingType::Buffer {
                            ty: wgpu::BufferBindingType::Storage { read_only: false },
                            has_dynamic_offset: false,
                            min_binding_size: None,
                        },
                        count: None,
                    },
                ],
            });
        let layout = ctx
            .device
            .create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("gc-effects-device-layout"),
                bind_group_layouts: &[&bgl],
                push_constant_ranges: &[],
            });
        let pipeline = ctx
            .device
            .create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                label: Some("gc-effects-device-pipeline"),
                layout: Some(&layout),
                module: &shader,
                entry_point: "main",
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            });
        let bind_group = ctx.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("gc-effects-device-bind-group"),
            layout: &bgl,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: inbuf.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: outbuf.as_entire_binding(),
                },
            ],
        });

        let mut encoder = ctx
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("gc-effects-device-encoder"),
            });
        {
            let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("gc-effects-device-pass"),
                timestamp_writes: None,
            });
            pass.set_pipeline(&pipeline);
            pass.set_bind_group(0, &bind_group, &[]);
            let workgroups = (lane_count as u32).div_ceil(64);
            pass.dispatch_workgroups(workgroups.max(1), 1, 1);
        }
        encoder.copy_buffer_to_buffer(&outbuf, 0, &staging, 0, byte_len);
        ctx.queue.submit(Some(encoder.finish()));

        let slice = staging.slice(..);
        let (tx, rx) = mpsc::channel();
        slice.map_async(wgpu::MapMode::Read, move |res| {
            let _ = tx.send(res);
        });
        ctx.device.poll(wgpu::Maintain::Wait);
        rx.recv()
            .map_err(|e| DeviceBackendError {
                code: "gpu/device-backend-map".to_string(),
                message: format!("map callback channel failed: {e}"),
            })?
            .map_err(|e| DeviceBackendError {
                code: "gpu/device-backend-map".to_string(),
                message: format!("map read failed: {e:?}"),
            })?;
        let mapped = slice.get_mapped_range().to_vec();
        staging.unmap();

        let out_u32: Vec<u32> = mapped
            .chunks_exact(4)
            .map(|chunk| u32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]))
            .collect();
        let checksum = blake3::hash(cast_slice(&out_u32)).to_hex().to_string();
        let payload_h = blake3::hash(print_term(payload).as_bytes())
            .to_hex()
            .to_string();

        Ok(Term::Map(
            [
                (TermOrdKey(Term::symbol(":ok")), Term::Bool(true)),
                (
                    TermOrdKey(Term::symbol(":kind")),
                    Term::Str(kind.to_string()),
                ),
                (
                    TermOrdKey(Term::symbol(":backend")),
                    Term::Str(GPU_BACKEND_DEVICE_RUNTIME.to_string()),
                ),
                (
                    TermOrdKey(Term::symbol(":adapter")),
                    Term::Str(ctx.adapter_info.name.clone()),
                ),
                (
                    TermOrdKey(Term::symbol(":lanes")),
                    Term::Int((lane_count as i64).into()),
                ),
                (TermOrdKey(Term::symbol(":checksum")), Term::Str(checksum)),
                (TermOrdKey(Term::symbol(":payload-h")), Term::Str(payload_h)),
                (TermOrdKey(Term::symbol(":error/op")), Term::symbol(op)),
            ]
            .into_iter()
            .collect(),
        ))
    }

    pub(super) fn device_limits_response(op: &str) -> Result<Term, DeviceBackendError> {
        let ctx = device_context()?;
        let l = ctx.device.limits();
        Ok(Term::Map(
            [
                (TermOrdKey(Term::symbol(":ok")), Term::Bool(true)),
                (
                    TermOrdKey(Term::symbol(":backend")),
                    Term::Str(GPU_BACKEND_DEVICE_RUNTIME.to_string()),
                ),
                (
                    TermOrdKey(Term::symbol(":adapter")),
                    Term::Str(ctx.adapter_info.name.clone()),
                ),
                (
                    TermOrdKey(Term::symbol(":max-buffer-bytes")),
                    Term::Int((l.max_buffer_size as i64).into()),
                ),
                (
                    TermOrdKey(Term::symbol(":max-storage-buffer-binding-size")),
                    Term::Int((l.max_storage_buffer_binding_size as i64).into()),
                ),
                (
                    TermOrdKey(Term::symbol(":max-compute-workgroup-size-x")),
                    Term::Int((l.max_compute_workgroup_size_x as i64).into()),
                ),
                (
                    TermOrdKey(Term::symbol(":max-compute-invocations-per-workgroup")),
                    Term::Int((l.max_compute_invocations_per_workgroup as i64).into()),
                ),
                (TermOrdKey(Term::symbol(":error/op")), Term::symbol(op)),
            ]
            .into_iter()
            .collect(),
        ))
    }

    pub(super) fn device_features_response(op: &str) -> Result<Term, DeviceBackendError> {
        let ctx = device_context()?;
        let mut features = Vec::new();
        if ctx.features.contains(wgpu::Features::TIMESTAMP_QUERY) {
            features.push(Term::symbol(":timestamp-query"));
        }
        if ctx.features.contains(wgpu::Features::SHADER_F16) {
            features.push(Term::symbol(":shader-f16"));
        }
        if features.is_empty() {
            features.push(Term::symbol(":baseline"));
        }
        Ok(Term::Map(
            [
                (TermOrdKey(Term::symbol(":ok")), Term::Bool(true)),
                (
                    TermOrdKey(Term::symbol(":backend")),
                    Term::Str(GPU_BACKEND_DEVICE_RUNTIME.to_string()),
                ),
                (
                    TermOrdKey(Term::symbol(":adapter")),
                    Term::Str(ctx.adapter_info.name.clone()),
                ),
                (
                    TermOrdKey(Term::symbol(":features")),
                    Term::Vector(features),
                ),
                (TermOrdKey(Term::symbol(":error/op")), Term::symbol(op)),
            ]
            .into_iter()
            .collect(),
        ))
    }

    struct DeviceContext {
        device: wgpu::Device,
        queue: wgpu::Queue,
        adapter_info: wgpu::AdapterInfo,
        features: wgpu::Features,
    }

    fn create_buffer(device: &wgpu::Device, label: &str, size: u64, usage: BufferUsages) -> Buffer {
        device.create_buffer(&wgpu::BufferDescriptor {
            label: Some(label),
            size: size.max(4),
            usage,
            mapped_at_creation: false,
        })
    }

    fn device_submit_via_resource_interop(
        kind: &str,
        op: &str,
        payload: &Term,
    ) -> Result<Option<Term>, DeviceBackendError> {
        let Some(payload_map) = payload_map(payload) else {
            return Ok(None);
        };
        let Some(in_id) = map_get_string(payload_map, ":in-buffer-id") else {
            return Ok(None);
        };
        let Some(out_id) = map_get_string(payload_map, ":out-buffer-id") else {
            return Ok(None);
        };
        let requested_count = map_get_nonnegative_usize(payload_map, ":count");

        let mut rt = device_resource_runtime()?;
        let Some(DeviceResourceState::Buffer(in_buf)) = rt.resources.get(&in_id) else {
            return Ok(Some(device_error(op, "interop-in-buffer-not-found")));
        };
        let in_lanes = decode_u32_lanes(in_buf);
        let lane_count = requested_count
            .unwrap_or(in_lanes.len())
            .min(in_lanes.len());
        let transformed: Vec<u32> = in_lanes
            .iter()
            .take(lane_count)
            .map(|lane| lane.saturating_mul(2).saturating_add(1))
            .collect();
        let transformed_bytes: &[u8] = cast_slice(&transformed);

        let Some(DeviceResourceState::Buffer(out_buf)) = rt.resources.get_mut(&out_id) else {
            return Ok(Some(device_error(op, "interop-out-buffer-not-found")));
        };
        if transformed_bytes.len() > out_buf.len() {
            return Ok(Some(device_error(op, "interop-out-buffer-too-small")));
        }
        out_buf[..transformed_bytes.len()].copy_from_slice(transformed_bytes);

        let checksum = blake3::hash(transformed_bytes).to_hex().to_string();
        let payload_h = blake3::hash(print_term(payload).as_bytes())
            .to_hex()
            .to_string();
        Ok(Some(map_term(vec![
            (":ok", Term::Bool(true)),
            (":kind", Term::Str(kind.to_string())),
            (
                ":backend",
                Term::Str(GPU_BACKEND_DEVICE_RUNTIME.to_string()),
            ),
            (":interop", Term::Bool(true)),
            (":in-buffer-id", Term::Str(in_id)),
            (":out-buffer-id", Term::Str(out_id)),
            (":lanes", Term::Int((lane_count as i64).into())),
            (":checksum", Term::Str(checksum)),
            (":payload-h", Term::Str(payload_h)),
            (":error/op", Term::symbol(op)),
        ])))
    }

    fn payload_u32_lanes(payload: &Term) -> Vec<u32> {
        let fallback_hash = || {
            let h = blake3::hash(print_term(payload).as_bytes());
            h.as_bytes()
                .chunks(4)
                .map(|chunk| {
                    let mut buf = [0_u8; 4];
                    buf[..chunk.len()].copy_from_slice(chunk);
                    u32::from_le_bytes(buf)
                })
                .collect::<Vec<_>>()
        };

        let Term::Map(map) = payload else {
            return fallback_hash();
        };
        let Some(Term::Vector(v)) = map.get(&TermOrdKey(Term::symbol(":lanes"))) else {
            return fallback_hash();
        };
        let mut out = Vec::with_capacity(v.len());
        for term in v {
            let Term::Int(i) = term else {
                return fallback_hash();
            };
            let Some(lane) = i.to_u32() else {
                return fallback_hash();
            };
            out.push(lane);
        }
        out
    }

    fn decode_u32_lanes(bytes: &[u8]) -> Vec<u32> {
        bytes
            .chunks_exact(4)
            .map(|chunk| u32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]))
            .collect()
    }

    fn device_resource_runtime()
    -> Result<std::sync::MutexGuard<'static, DeviceResourceRuntime>, DeviceBackendError> {
        let runtime = DEVICE_RESOURCES.get_or_init(|| Mutex::new(DeviceResourceRuntime::default()));
        runtime.lock().map_err(|_| DeviceBackendError {
            code: "gpu/device-backend-resource-lock".to_string(),
            message: "device backend resource lock poisoned".to_string(),
        })
    }

    fn alloc_resource_id(runtime: &mut DeviceResourceRuntime, prefix: &str) -> String {
        runtime.next_id = runtime.next_id.saturating_add(1);
        format!("gpu-device-{prefix}-{}", runtime.next_id)
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

    fn payload_descriptor(payload: &Term) -> Term {
        let Some(payload_map) = payload_map(payload) else {
            return payload.clone();
        };
        payload_map
            .get(&TermOrdKey(Term::symbol(":desc")))
            .cloned()
            .unwrap_or_else(|| payload.clone())
    }

    fn device_error(op: &str, suffix: &str) -> Term {
        let code = if op.starts_with("gfx/gpu::") {
            format!("gfx/device-runtime-{suffix}")
        } else {
            format!("gpu/device-runtime-{suffix}")
        };
        map_term(vec![
            (":ok", Term::Bool(false)),
            (
                ":backend",
                Term::Str(
                    crate::runner_gpu_backend_policy::GPU_BACKEND_DEVICE_RUNTIME_FULL.to_string(),
                ),
            ),
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

    fn device_context() -> Result<&'static DeviceContext, DeviceBackendError> {
        let ctx = DEVICE_CONTEXT.get_or_init(|| {
            let instance = wgpu::Instance::default();
            let adapter =
                pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
                    power_preference: wgpu::PowerPreference::HighPerformance,
                    compatible_surface: None,
                    force_fallback_adapter: false,
                }))
                .ok_or_else(|| "gpu adapter unavailable".to_string())?;
            let adapter_info = adapter.get_info();
            let features = adapter.features();
            let limits = wgpu::Limits::downlevel_defaults();
            let (device, queue) = pollster::block_on(adapter.request_device(
                &wgpu::DeviceDescriptor {
                    label: Some("genesis-gc-effects-device-backend"),
                    required_features: wgpu::Features::empty(),
                    required_limits: limits,
                },
                None,
            ))
            .map_err(|e| format!("request device: {e:?}"))?;
            Ok(DeviceContext {
                device,
                queue,
                adapter_info,
                features,
            })
        });
        match ctx {
            Ok(ctx) => Ok(ctx),
            Err(message) => Err(DeviceBackendError {
                code: "gpu/device-backend-unavailable".to_string(),
                message: message.clone(),
            }),
        }
    }
}

#[cfg(all(not(target_os = "wasi"), feature = "gpu-device-backend"))]
use imp::{
    device_create_response, device_destroy_resource_response, device_features_response,
    device_limits_response, device_read_buffer_response, device_read_texture_response,
    device_submit_response, device_write_buffer_response, device_write_texture_response,
};

#[cfg(any(target_os = "wasi", not(feature = "gpu-device-backend")))]
fn device_submit_response(
    _kind: &str,
    _op: &str,
    _payload: &Term,
) -> Result<Term, DeviceBackendError> {
    Err(DeviceBackendError {
        code: "gpu/device-backend-unavailable".to_string(),
        message: "gc_effects built without `gpu-device-backend` feature".to_string(),
    })
}

#[cfg(any(target_os = "wasi", not(feature = "gpu-device-backend")))]
fn device_limits_response(_op: &str) -> Result<Term, DeviceBackendError> {
    Err(DeviceBackendError {
        code: "gpu/device-backend-unavailable".to_string(),
        message: "gc_effects built without `gpu-device-backend` feature".to_string(),
    })
}

#[cfg(any(target_os = "wasi", not(feature = "gpu-device-backend")))]
fn device_features_response(_op: &str) -> Result<Term, DeviceBackendError> {
    Err(DeviceBackendError {
        code: "gpu/device-backend-unavailable".to_string(),
        message: "gc_effects built without `gpu-device-backend` feature".to_string(),
    })
}

#[cfg(any(target_os = "wasi", not(feature = "gpu-device-backend")))]
fn device_create_response(
    _kind: DeviceResourceKind,
    _op: &str,
    _payload: &Term,
) -> Result<Term, DeviceBackendError> {
    Err(DeviceBackendError {
        code: "gpu/device-backend-unavailable".to_string(),
        message: "gc_effects built without `gpu-device-backend` feature".to_string(),
    })
}

#[cfg(any(target_os = "wasi", not(feature = "gpu-device-backend")))]
fn device_write_buffer_response(_op: &str, _payload: &Term) -> Result<Term, DeviceBackendError> {
    Err(DeviceBackendError {
        code: "gpu/device-backend-unavailable".to_string(),
        message: "gc_effects built without `gpu-device-backend` feature".to_string(),
    })
}

#[cfg(any(target_os = "wasi", not(feature = "gpu-device-backend")))]
fn device_read_buffer_response(_op: &str, _payload: &Term) -> Result<Term, DeviceBackendError> {
    Err(DeviceBackendError {
        code: "gpu/device-backend-unavailable".to_string(),
        message: "gc_effects built without `gpu-device-backend` feature".to_string(),
    })
}

#[cfg(any(target_os = "wasi", not(feature = "gpu-device-backend")))]
fn device_write_texture_response(_op: &str, _payload: &Term) -> Result<Term, DeviceBackendError> {
    Err(DeviceBackendError {
        code: "gpu/device-backend-unavailable".to_string(),
        message: "gc_effects built without `gpu-device-backend` feature".to_string(),
    })
}

#[cfg(any(target_os = "wasi", not(feature = "gpu-device-backend")))]
fn device_read_texture_response(_op: &str, _payload: &Term) -> Result<Term, DeviceBackendError> {
    Err(DeviceBackendError {
        code: "gpu/device-backend-unavailable".to_string(),
        message: "gc_effects built without `gpu-device-backend` feature".to_string(),
    })
}

#[cfg(any(target_os = "wasi", not(feature = "gpu-device-backend")))]
fn device_destroy_resource_response(
    _op: &str,
    _payload: &Term,
) -> Result<Term, DeviceBackendError> {
    Err(DeviceBackendError {
        code: "gpu/device-backend-unavailable".to_string(),
        message: "gc_effects built without `gpu-device-backend` feature".to_string(),
    })
}
