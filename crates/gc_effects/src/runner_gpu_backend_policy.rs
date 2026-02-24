use gc_coreform::{Term, TermOrdKey};

use crate::policy::OpPolicy;

pub(crate) const GPU_BACKEND_FIRST_PARTY: &str = "first-party-runtime";
pub(crate) const GPU_BACKEND_DEVICE_RUNTIME: &str = "device-runtime";
pub(crate) const GPU_BACKEND_DEVICE_RUNTIME_FULL: &str = "device-runtime-full";
const GPU_BACKEND_POLICY_ALLOW_FALLBACK: &str = "allow-fallback";
const GPU_BACKEND_POLICY_DEV_ALLOW_FALLBACK: &str = "dev-allow-fallback";
const GPU_BACKEND_POLICY_REQUIRE_DEVICE: &str = "require-device";

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
        .and_then(|p| p.extra.get("gpu_backend").and_then(|v| v.as_str()))
        .unwrap_or(GPU_BACKEND_FIRST_PARTY)
        .trim()
        .to_ascii_lowercase();
    match raw.as_str() {
        "device-runtime-full" => GpuBackendKind::DeviceRuntimeFullLifecycle,
        "device-runtime" => GpuBackendKind::DeviceRuntimeSubmitIntrospection,
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
    if let Some(raw) = pol.and_then(|p| p.extra.get("gpu_backend_policy").and_then(|v| v.as_str()))
    {
        return parse_gpu_backend_fallback_policy(raw);
    }
    default_gpu_backend_fallback_policy()
}

fn parse_gpu_backend_fallback_policy(raw: &str) -> GpuBackendFallbackPolicy {
    let normalized = raw.trim().to_ascii_lowercase();
    match normalized.as_str() {
        GPU_BACKEND_POLICY_REQUIRE_DEVICE => GpuBackendFallbackPolicy::RequireDevice,
        GPU_BACKEND_POLICY_DEV_ALLOW_FALLBACK | GPU_BACKEND_POLICY_ALLOW_FALLBACK => {
            GpuBackendFallbackPolicy::AllowFallback
        }
        _ => GpuBackendFallbackPolicy::AllowFallback,
    }
}

fn default_gpu_backend_fallback_policy() -> GpuBackendFallbackPolicy {
    default_gpu_backend_fallback_policy_from_env(
        std::env::var("GENESIS_GPU_BACKEND_POLICY_DEFAULT")
            .ok()
            .as_deref(),
    )
}

fn default_gpu_backend_fallback_policy_from_env(raw: Option<&str>) -> GpuBackendFallbackPolicy {
    match raw {
        Some(value) => parse_gpu_backend_fallback_policy(value),
        None => GpuBackendFallbackPolicy::AllowFallback,
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

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use toml::Value as TomlValue;

    use super::{
        GpuBackendFallbackPolicy, GpuBackendKind, default_gpu_backend_fallback_policy_from_env,
        gpu_backend_kind, parse_gpu_backend_fallback_policy,
    };
    use crate::policy::OpPolicy;

    fn op_with_extra(entries: &[(&str, &str)]) -> OpPolicy {
        let mut extra = BTreeMap::new();
        for (k, v) in entries {
            extra.insert((*k).to_string(), TomlValue::String((*v).to_string()));
        }
        OpPolicy {
            base_dir: None,
            create_dirs: false,
            timeout_ms: None,
            log_inline_max_bytes: None,
            extra,
        }
    }

    #[test]
    fn gpu_backend_kind_accepts_only_canonical_backend_values() {
        let op = op_with_extra(&[("gpu_backend", "device-runtime")]);
        assert_eq!(
            gpu_backend_kind(Some(&op)),
            GpuBackendKind::DeviceRuntimeSubmitIntrospection
        );

        let op = op_with_extra(&[("gpu_backend", "device-runtime-full")]);
        assert_eq!(
            gpu_backend_kind(Some(&op)),
            GpuBackendKind::DeviceRuntimeFullLifecycle
        );

        let legacy = [
            "device-bridge",
            "device-runtime-submit",
            "device-runtime-lifecycle",
            "device",
            "device-full",
            "full-device",
        ];
        for value in legacy {
            let op = op_with_extra(&[("gpu_backend", value)]);
            assert_eq!(
                gpu_backend_kind(Some(&op)),
                GpuBackendKind::FirstParty,
                "legacy gpu_backend value `{value}` must not be accepted"
            );
        }
    }

    #[test]
    fn gpu_backend_kind_ignores_legacy_backend_key_alias() {
        let op = op_with_extra(&[("backend", "device-runtime")]);
        assert_eq!(
            gpu_backend_kind(Some(&op)),
            GpuBackendKind::FirstParty,
            "legacy `backend` key alias must not be accepted"
        );
    }

    #[test]
    fn parse_gpu_backend_fallback_policy_accepts_release_and_dev_variants() {
        assert_eq!(
            parse_gpu_backend_fallback_policy("require-device"),
            GpuBackendFallbackPolicy::RequireDevice
        );
        assert_eq!(
            parse_gpu_backend_fallback_policy("allow-fallback"),
            GpuBackendFallbackPolicy::AllowFallback
        );
        assert_eq!(
            parse_gpu_backend_fallback_policy("dev-allow-fallback"),
            GpuBackendFallbackPolicy::AllowFallback
        );
    }

    #[test]
    fn default_gpu_backend_policy_is_fail_open_without_override() {
        assert_eq!(
            default_gpu_backend_fallback_policy_from_env(None),
            GpuBackendFallbackPolicy::AllowFallback
        );
    }

    #[test]
    fn default_gpu_backend_policy_can_fail_closed_via_override() {
        assert_eq!(
            default_gpu_backend_fallback_policy_from_env(Some("require-device")),
            GpuBackendFallbackPolicy::RequireDevice
        );
    }

    #[test]
    fn gpu_backend_policy_ignores_legacy_backend_policy_key_alias() {
        let op = op_with_extra(&[("backend_policy", "require-device")]);
        assert_eq!(
            super::gpu_backend_fallback_policy(Some(&op)),
            GpuBackendFallbackPolicy::AllowFallback,
            "legacy `backend_policy` key alias must not be accepted"
        );
    }
}
