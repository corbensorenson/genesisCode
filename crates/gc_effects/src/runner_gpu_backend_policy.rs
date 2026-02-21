use gc_coreform::{Term, TermOrdKey};

use crate::policy::OpPolicy;

pub(crate) const GPU_BACKEND_FIRST_PARTY: &str = "first-party-runtime";
pub(crate) const GPU_BACKEND_DEVICE_RUNTIME: &str = "device-runtime";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum GpuBackendKind {
    FirstParty,
    DeviceRuntime,
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
        "device-runtime" | "device" | "device-bridge" => GpuBackendKind::DeviceRuntime,
        _ => GpuBackendKind::FirstParty,
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

pub(crate) fn gpu_op_prefers_device_backend(op: &str) -> bool {
    matches!(
        op,
        "gpu/compute::submit"
            | "gfx/gpu::submit-frame-graph"
            | "gfx/gpu::submit-compute-graph"
            | "gpu/compute::limits"
            | "gfx/gpu::limits"
            | "gpu/compute::features"
            | "gfx/gpu::features"
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
