use std::io::{Read as _, Write as _};
use std::path::PathBuf;

use anyhow::{Context, Result, bail};

pub(crate) const GPU_COMPUTE_BRIDGE_MODE_ARG: &str = "--gpu-compute-bridge";

#[derive(Debug, Clone)]
pub(crate) struct BridgeCommandSpec {
    pub base_dir: PathBuf,
    pub cmd_name: String,
    pub args: Vec<String>,
}

#[cfg(feature = "device-bridge")]
fn shell_escape_string(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}

fn read_framed_payload() -> Result<String> {
    let mut input = String::new();
    std::io::stdin()
        .read_to_string(&mut input)
        .context("read bridge stdin frame")?;
    let Some((len_s, body)) = input.split_once('\n') else {
        bail!("bridge request must use framed format: <len>\\n<body>");
    };
    let expected = len_s
        .trim()
        .parse::<usize>()
        .context("parse bridge frame length")?;
    if body.len() != expected {
        bail!(
            "bridge frame length mismatch: expected {expected} bytes, got {} bytes",
            body.len()
        );
    }
    Ok(body.to_string())
}

fn write_framed_response(response: &str) -> Result<()> {
    let mut stdout = std::io::stdout().lock();
    write!(stdout, "{}\n{response}", response.len()).context("write bridge response frame")?;
    stdout.flush().context("flush bridge response frame")
}

pub(crate) fn inrepo_device_bridge_spec() -> Result<Option<BridgeCommandSpec>> {
    #[cfg(feature = "device-bridge")]
    {
        let exe = std::env::current_exe().context("resolve runtime bench executable")?;
        let base_dir = exe
            .parent()
            .context("resolve runtime bench executable parent directory")?
            .to_path_buf();
        let cmd_name = exe
            .file_name()
            .and_then(|s| s.to_str())
            .context("resolve runtime bench executable filename")?
            .to_string();
        return Ok(Some(BridgeCommandSpec {
            base_dir,
            cmd_name,
            args: vec![GPU_COMPUTE_BRIDGE_MODE_ARG.to_string()],
        }));
    }
    #[cfg(not(feature = "device-bridge"))]
    {
        Ok(None)
    }
}

pub(crate) fn maybe_run_bridge_mode_from_argv() -> Result<bool> {
    let mut args = std::env::args().skip(1);
    let Some(flag) = args.next() else {
        return Ok(false);
    };
    if flag != GPU_COMPUTE_BRIDGE_MODE_ARG {
        return Ok(false);
    }
    let op = args
        .next()
        .or_else(|| std::env::var("GENESIS_HOST_BRIDGE_OP").ok())
        .unwrap_or_else(|| "gpu/compute::submit".to_string());
    let payload = read_framed_payload()?;
    let response = bridge_response_for_op(&op, &payload)?;
    write_framed_response(&response)?;
    Ok(true)
}

fn bridge_response_for_op(op: &str, payload: &str) -> Result<String> {
    match op {
        "gpu/compute::submit" => submit_response(payload),
        "gpu/compute::limits" => limits_response(),
        "gpu/compute::features" => features_response(),
        other => bail!("unsupported gpu bridge op: {other}"),
    }
}

#[cfg(not(feature = "device-bridge"))]
fn submit_response(_payload: &str) -> Result<String> {
    bail!("device bridge unavailable: compile gc_runtime_bench with --features device-bridge")
}

#[cfg(not(feature = "device-bridge"))]
fn limits_response() -> Result<String> {
    bail!("device bridge unavailable: compile gc_runtime_bench with --features device-bridge")
}

#[cfg(not(feature = "device-bridge"))]
fn features_response() -> Result<String> {
    bail!("device bridge unavailable: compile gc_runtime_bench with --features device-bridge")
}

#[cfg(feature = "device-bridge")]
fn submit_response(payload: &str) -> Result<String> {
    const LANES: usize = 1024;
    const WORKGROUP_SIZE: u32 = 64;
    const DISPATCH_X: u32 = (LANES as u32).div_ceil(WORKGROUP_SIZE);
    const SHADER_SRC: &str = r#"
@group(0) @binding(0)
var<storage, read> inbuf: array<u32, 1024>;

@group(0) @binding(1)
var<storage, read_write> outbuf: array<u32, 1024>;

@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
  let i = gid.x;
  if (i < 1024u) {
    outbuf[i] = inbuf[i] * 1664525u + i + 1013904223u;
  }
}
"#;

    let ctx = DeviceContext::new().context("initialize gpu device context")?;
    let seed = blake3::hash(payload.as_bytes());
    let mut input: Vec<u32> = Vec::with_capacity(LANES);
    let seed_bytes = seed.as_bytes();
    for i in 0..LANES {
        let b = i % (seed_bytes.len() - 3);
        let lane_seed = u32::from_le_bytes([
            seed_bytes[b],
            seed_bytes[b + 1],
            seed_bytes[b + 2],
            seed_bytes[b + 3],
        ]);
        input.push(lane_seed ^ (i as u32));
    }

    let byte_len = (LANES * std::mem::size_of::<u32>()) as u64;
    let inbuf = ctx.device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("genesis-device-bridge-inbuf"),
        size: byte_len,
        usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let outbuf = ctx.device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("genesis-device-bridge-outbuf"),
        size: byte_len,
        usage: wgpu::BufferUsages::STORAGE
            | wgpu::BufferUsages::COPY_SRC
            | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let staging = ctx.device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("genesis-device-bridge-staging"),
        size: byte_len,
        usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    ctx.queue
        .write_buffer(&inbuf, 0, bytemuck::cast_slice(&input[..]));

    let shader = ctx
        .device
        .create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("genesis-device-bridge-shader"),
            source: wgpu::ShaderSource::Wgsl(SHADER_SRC.into()),
        });
    let bgl = ctx
        .device
        .create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("genesis-device-bridge-bgl"),
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
    let pipeline_layout = ctx
        .device
        .create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("genesis-device-bridge-pipeline-layout"),
            bind_group_layouts: &[&bgl],
            push_constant_ranges: &[],
        });
    let pipeline = ctx
        .device
        .create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("genesis-device-bridge-pipeline"),
            layout: Some(&pipeline_layout),
            module: &shader,
            entry_point: "main",
            compilation_options: wgpu::PipelineCompilationOptions::default(),
        });
    let bind_group = ctx.device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("genesis-device-bridge-bg"),
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
            label: Some("genesis-device-bridge-encoder"),
        });
    {
        let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
            label: Some("genesis-device-bridge-pass"),
            timestamp_writes: None,
        });
        pass.set_pipeline(&pipeline);
        pass.set_bind_group(0, &bind_group, &[]);
        pass.dispatch_workgroups(DISPATCH_X, 1, 1);
    }
    encoder.copy_buffer_to_buffer(&outbuf, 0, &staging, 0, byte_len);
    ctx.queue.submit(Some(encoder.finish()));

    let slice = staging.slice(..);
    let (tx, rx) = std::sync::mpsc::channel();
    slice.map_async(wgpu::MapMode::Read, move |res| {
        let _ = tx.send(res);
    });
    ctx.device.poll(wgpu::Maintain::Wait);
    match rx
        .recv_timeout(std::time::Duration::from_secs(5))
        .context("wait for gpu map callback")?
    {
        Ok(()) => {}
        Err(e) => bail!("map_async failed: {e:?}"),
    }

    let data = slice.get_mapped_range();
    let words: &[u32] = bytemuck::cast_slice(&data);
    let checksum = words
        .iter()
        .fold(0u64, |acc, w| acc.wrapping_add((*w) as u64));
    drop(data);
    staging.unmap();

    Ok(format!(
        "{{:ok true :kind \"gpu-compute-submit\" :backend \"device-bridge\" :adapter \"{}\" :lanes {} :checksum \"{}\"}}",
        shell_escape_string(&ctx.adapter_info.name),
        LANES,
        checksum
    ))
}

#[cfg(feature = "device-bridge")]
fn limits_response() -> Result<String> {
    let ctx = DeviceContext::new().context("initialize gpu device context")?;
    let l = ctx.device.limits();
    Ok(format!(
        "{{:ok true :backend \"device-bridge\" :max-buffer-bytes {} :max-storage-buffer-binding-size {} :max-compute-workgroup-size-x {} :max-compute-invocations-per-workgroup {}}}",
        l.max_buffer_size,
        l.max_storage_buffer_binding_size,
        l.max_compute_workgroup_size_x,
        l.max_compute_invocations_per_workgroup,
    ))
}

#[cfg(feature = "device-bridge")]
fn features_response() -> Result<String> {
    let ctx = DeviceContext::new().context("initialize gpu device context")?;
    let mut names: Vec<String> = Vec::new();
    if ctx.features.contains(wgpu::Features::TIMESTAMP_QUERY) {
        names.push("timestamp-query".to_string());
    }
    if ctx.features.contains(wgpu::Features::SHADER_F16) {
        names.push("shader-float16".to_string());
    }
    let feature_list = if names.is_empty() {
        "[]".to_string()
    } else {
        format!(
            "[{}]",
            names
                .iter()
                .map(|s| format!("\"{}\"", shell_escape_string(s)))
                .collect::<Vec<_>>()
                .join(" ")
        )
    };
    Ok(format!(
        "{{:ok true :backend \"device-bridge\" :adapter \"{}\" :features {}}}",
        shell_escape_string(&ctx.adapter_info.name),
        feature_list
    ))
}

#[cfg(feature = "device-bridge")]
struct DeviceContext {
    device: wgpu::Device,
    queue: wgpu::Queue,
    adapter_info: wgpu::AdapterInfo,
    features: wgpu::Features,
}

#[cfg(feature = "device-bridge")]
impl DeviceContext {
    fn new() -> Result<Self> {
        let instance = wgpu::Instance::default();
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            compatible_surface: None,
            force_fallback_adapter: false,
        }))
        .context("request gpu adapter")?;
        let adapter_info = adapter.get_info();
        let features = adapter.features();
        let limits = wgpu::Limits::downlevel_defaults();
        let (device, queue) = pollster::block_on(adapter.request_device(
            &wgpu::DeviceDescriptor {
                label: Some("genesis-device-bridge"),
                required_features: wgpu::Features::empty(),
                required_limits: limits,
            },
            None,
        ))
        .context("request gpu device")?;
        Ok(Self {
            device,
            queue,
            adapter_info,
            features,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::inrepo_device_bridge_spec;

    #[cfg(not(feature = "device-bridge"))]
    #[test]
    fn inrepo_bridge_spec_is_absent_without_feature() {
        assert!(inrepo_device_bridge_spec().expect("spec").is_none());
    }

    #[cfg(feature = "device-bridge")]
    #[test]
    fn inrepo_bridge_spec_points_to_runtime_bench_executable() {
        use super::GPU_COMPUTE_BRIDGE_MODE_ARG;

        let spec = inrepo_device_bridge_spec()
            .expect("spec")
            .expect("bridge spec must exist with device-bridge feature");
        assert!(spec.base_dir.is_dir());
        assert!(!spec.cmd_name.trim().is_empty());
        assert_eq!(spec.args, vec![GPU_COMPUTE_BRIDGE_MODE_ARG.to_string()]);
    }
}
