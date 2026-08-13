use gc_coreform::{Term, TermOrdKey};

use crate::policy::{AuthorizedGpuPolicy, OpPolicy};

pub(crate) use crate::policy::{
    AuthorizedGpuBackend as GpuBackendKind, AuthorizedGpuFallback as GpuBackendFallbackPolicy,
};

pub(crate) const GPU_BACKEND_FIRST_PARTY: &str = "first-party-runtime";
pub(crate) const GPU_BACKEND_DEVICE_RUNTIME: &str = "device-runtime";
pub(crate) const GPU_BACKEND_DEVICE_RUNTIME_FULL: &str = "device-runtime-full";
fn authorized_gpu_policy(pol: Option<&OpPolicy>) -> Result<AuthorizedGpuPolicy, String> {
    let Some(policy) = pol else {
        return Ok(AuthorizedGpuPolicy {
            backend: GpuBackendKind::FirstParty,
            fallback: GpuBackendFallbackPolicy::AllowFallback,
        });
    };
    policy
        .authorized_gpu
        .ok_or_else(|| "missing GenesisCode GPU policy authority".to_string())
}

pub(crate) fn gpu_backend_kind(pol: Option<&OpPolicy>) -> Result<GpuBackendKind, String> {
    authorized_gpu_policy(pol).map(|policy| policy.backend)
}

pub(crate) fn gpu_backend_kind_label(kind: GpuBackendKind) -> &'static str {
    match kind {
        GpuBackendKind::FirstParty => GPU_BACKEND_FIRST_PARTY,
        GpuBackendKind::DeviceRuntimeSubmitIntrospection => GPU_BACKEND_DEVICE_RUNTIME,
        GpuBackendKind::DeviceRuntimeFullLifecycle => GPU_BACKEND_DEVICE_RUNTIME_FULL,
    }
}

pub(crate) fn gpu_backend_fallback_policy(
    pol: Option<&OpPolicy>,
) -> Result<GpuBackendFallbackPolicy, String> {
    authorized_gpu_policy(pol).map(|policy| policy.fallback)
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
        GpuBackendFallbackPolicy, GpuBackendKind, gpu_backend_fallback_policy, gpu_backend_kind,
    };
    use crate::policy::{AuthorizedGpuPolicy, OpPolicy};

    fn op_with_policy(
        entries: &[(&str, &str)],
        authorized_gpu: Option<AuthorizedGpuPolicy>,
    ) -> OpPolicy {
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
            authorized_cap: None,
            authorized_max_bytes: None,
            authorized_process_programs: None,
            authorized_database: None,
            authorized_network: None,
            authorized_crypto: None,
            authorized_gpu,
            authorized_bridge_identity: None,
            authorized_plugin: None,
            authorized_ffi: None,
        }
    }

    #[test]
    fn gpu_backend_selection_consumes_authority_before_raw_policy() {
        let op = op_with_policy(
            &[
                ("gpu_backend", "first-party-runtime"),
                ("gpu_backend_policy", "allow-fallback"),
            ],
            Some(AuthorizedGpuPolicy {
                backend: GpuBackendKind::DeviceRuntimeSubmitIntrospection,
                fallback: GpuBackendFallbackPolicy::RequireDevice,
            }),
        );
        assert_eq!(
            gpu_backend_kind(Some(&op)).unwrap(),
            GpuBackendKind::DeviceRuntimeSubmitIntrospection,
        );
        assert_eq!(
            gpu_backend_fallback_policy(Some(&op)).unwrap(),
            GpuBackendFallbackPolicy::RequireDevice,
        );
    }

    #[test]
    fn absent_policy_uses_local_default_but_present_missing_authority_fails_closed() {
        assert_eq!(gpu_backend_kind(None).unwrap(), GpuBackendKind::FirstParty,);
        assert_eq!(
            gpu_backend_fallback_policy(None).unwrap(),
            GpuBackendFallbackPolicy::AllowFallback,
        );
        let missing = op_with_policy(&[("gpu_backend", "device-runtime")], None);
        assert!(gpu_backend_kind(Some(&missing)).is_err());
        assert!(gpu_backend_fallback_policy(Some(&missing)).is_err());
    }
}
