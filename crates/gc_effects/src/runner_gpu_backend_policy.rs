use gc_coreform::{Term, TermOrdKey};

use crate::policy::OpPolicy;

pub(crate) const GPU_BACKEND_FIRST_PARTY: &str = "first-party-runtime";
pub(crate) const GPU_BACKEND_DEVICE_RUNTIME: &str = "device-runtime";
pub(crate) const GPU_BACKEND_DEVICE_RUNTIME_FULL: &str = "device-runtime-full";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum GpuBackendKind {
    FirstParty,
    DeviceRuntimeSubmitIntrospection,
    DeviceRuntimeFullLifecycle,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum GpuBackendFallbackPolicy {
    AllowFallback,
    RequireDevice,
}

pub(crate) fn gpu_backend_kind(pol: Option<&OpPolicy>) -> GpuBackendKind {
    let raw = pol
        .and_then(|p| {
            p.extra
                .get("gpu_backend")
                .or_else(|| p.extra.get("backend"))
                .and_then(|v| v.as_str())
        })
        .unwrap_or(GPU_BACKEND_FIRST_PARTY)
        .trim()
        .to_ascii_lowercase();
    match raw.as_str() {
        "device-runtime-full" | "device-runtime-lifecycle" | "device-full" | "full-device" => {
            GpuBackendKind::DeviceRuntimeFullLifecycle
        }
        "device-runtime" | "device-runtime-submit" | "device" | "device-bridge" => {
            GpuBackendKind::DeviceRuntimeSubmitIntrospection
        }
        _ => GpuBackendKind::FirstParty,
    }
}

pub(crate) fn gpu_backend_kind_label(kind: GpuBackendKind) -> &'static str {
    match kind {
        GpuBackendKind::FirstParty => GPU_BACKEND_FIRST_PARTY,
        GpuBackendKind::DeviceRuntimeSubmitIntrospection => GPU_BACKEND_DEVICE_RUNTIME,
        GpuBackendKind::DeviceRuntimeFullLifecycle => GPU_BACKEND_DEVICE_RUNTIME_FULL,
    }
}

pub(crate) fn gpu_backend_fallback_policy(pol: Option<&OpPolicy>) -> GpuBackendFallbackPolicy {
    let raw = pol
        .and_then(|p| {
            p.extra
                .get("gpu_backend_policy")
                .or_else(|| p.extra.get("backend_policy"))
                .and_then(|v| v.as_str())
        })
        .unwrap_or("allow-fallback")
        .trim()
        .to_ascii_lowercase();
    match raw.as_str() {
        "require-device" => GpuBackendFallbackPolicy::RequireDevice,
        _ => GpuBackendFallbackPolicy::AllowFallback,
    }
}

pub(crate) fn gpu_op_prefers_device_backend(op: &str, backend_kind: GpuBackendKind) -> bool {
    match backend_kind {
        GpuBackendKind::FirstParty => false,
        GpuBackendKind::DeviceRuntimeSubmitIntrospection => gpu_op_submit_or_introspection(op),
        GpuBackendKind::DeviceRuntimeFullLifecycle => gpu_op_canonical_lifecycle(op),
    }
}

fn gpu_op_submit_or_introspection(op: &str) -> bool {
    matches!(
        op,
        "gpu/compute::submit"
            | "gfx/gpu::submit-frame-graph"
            | "gpu/compute::limits"
            | "gfx/gpu::limits"
            | "gpu/compute::features"
            | "gfx/gpu::features"
    )
}

fn gpu_op_canonical_lifecycle(op: &str) -> bool {
    gpu_op_submit_or_introspection(op)
        || matches!(
            op,
            "gpu/compute::create-buffer"
                | "gfx/gpu::create-buffer"
                | "gfx/gpu::create-texture"
                | "gfx/gpu::create-sampler"
                | "gpu/compute::create-shader-module"
                | "gfx/gpu::create-shader-module"
                | "gpu/compute::create-bind-group-layout"
                | "gfx/gpu::create-bind-group-layout"
                | "gpu/compute::create-bind-group"
                | "gfx/gpu::create-bind-group"
                | "gpu/compute::create-pipeline-layout"
                | "gfx/gpu::create-pipeline-layout"
                | "gpu/compute::create-compute-pipeline"
                | "gpu/compute::create-kernel"
                | "gfx/gpu::create-render-pipeline"
                | "gpu/compute::write-buffer"
                | "gfx/gpu::write-buffer"
                | "gpu/compute::read-buffer"
                | "gfx/gpu::read-buffer"
                | "gfx/gpu::write-texture"
                | "gfx/gpu::read-texture"
                | "gpu/compute::destroy-resource"
                | "gfx/gpu::destroy-resource"
        )
}

pub(crate) fn inject_backend_fallback_metadata(
    term: Term,
    requested_backend: &str,
    reason: &str,
) -> Term {
    let Term::Map(mut map) = term else {
        return term;
    };
    map.insert(
        TermOrdKey(Term::symbol(":backend-fallback-from")),
        Term::Str(requested_backend.to_string()),
    );
    map.insert(
        TermOrdKey(Term::symbol(":backend-fallback-reason")),
        Term::Str(reason.to_string()),
    );
    Term::Map(map)
}
