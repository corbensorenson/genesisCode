use gc_coreform::Term;

#[derive(Debug, Clone)]
pub(crate) struct DeviceBackendError {
    pub code: String,
    pub message: String,
}

pub(crate) fn call_device_backend(op: &str, payload: &Term) -> Result<Term, DeviceBackendError> {
    match canonical_device_op(op) {
        Some(DeviceOp::Submit(kind)) => device_submit_response(kind, op, payload),
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
    Limits,
    Features,
}

fn canonical_device_op(op: &str) -> Option<DeviceOp> {
    match op {
        "gpu/compute::submit" => Some(DeviceOp::Submit("gpu-compute-submit")),
        "gfx/gpu::submit-frame-graph" => Some(DeviceOp::Submit("gfx-frame-submit")),
        "gpu/compute::limits" | "gfx/gpu::limits" => Some(DeviceOp::Limits),
        "gpu/compute::features" | "gfx/gpu::features" => Some(DeviceOp::Features),
        _ => None,
    }
}

#[cfg(all(not(target_os = "wasi"), feature = "gpu-device-backend"))]
mod imp {
    use std::sync::{OnceLock, mpsc};

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

    pub(super) fn device_submit_response(
        kind: &str,
        op: &str,
        payload: &Term,
    ) -> Result<Term, DeviceBackendError> {
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
use imp::{device_features_response, device_limits_response, device_submit_response};

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
