use std::path::{Path, PathBuf};

use gc_coreform::hash_term;
use gc_kernel::{Apply, EvalCtx, MemLimits, Value};
use gc_pkg::{
    PACKAGE_MANIFEST_AUTHORITY_BINDING, PackageManifest, decode_authorized_package_manifest,
    package_manifest_authority_request, read_package_manifest_transport,
};
use gc_prelude::{build_prelude, load_selfhost_coreform_toolchain_v1_with_mode};

use crate::frontend::{enforce_frontend_allowed, rust_frontend_compat_enabled};
use crate::{CoreformFrontend, ObligationError};

const STEP_LIMIT: u64 = 20_000_000;
const ALLOC_LIMIT: u64 = 80_000_000;

pub fn load_package_manifest_with_frontend(
    path: &Path,
    frontend: &CoreformFrontend,
) -> Result<(PackageManifest, PathBuf), ObligationError> {
    enforce_frontend_allowed(frontend, "package-manifest authority")?;
    let CoreformFrontend::Selfhost(config) = frontend else {
        if rust_frontend_compat_enabled() {
            return PackageManifest::load(path)
                .map_err(|error| ObligationError::Manifest(error.to_string()));
        }
        return Err(ObligationError::Manifest(
            "package-manifest authority requires an artifact-loaded selfhost frontend".to_string(),
        ));
    };

    let transport = read_package_manifest_transport(path)
        .map_err(|error| ObligationError::Manifest(error.to_string()))?;
    let request = package_manifest_authority_request(transport.document, &transport.source_hash);
    let request_hash = blake3::Hash::from_bytes(hash_term(&request))
        .to_hex()
        .to_string();
    let mut context = EvalCtx::with_step_limit(None);
    context.set_mem_limits(MemLimits {
        max_alloc_units: Some(ALLOC_LIMIT),
        max_bytes_len: Some(16 * 1024 * 1024),
        max_map_len: Some(65_536),
        max_string_len: Some(16 * 1024 * 1024),
        max_vec_len: Some(65_536),
        ..MemLimits::default()
    });
    let prelude = build_prelude(&mut context);
    let mut environment = prelude.env;
    load_selfhost_coreform_toolchain_v1_with_mode(
        &mut context,
        &mut environment,
        config.bootstrap_mode,
        config.artifact.as_deref(),
    )
    .map_err(|error| {
        ObligationError::Manifest(format!(
            "package-manifest artifact bootstrap failed: {error:#}"
        ))
    })?;
    let authority = environment
        .get(PACKAGE_MANIFEST_AUTHORITY_BINDING)
        .ok_or_else(|| {
            ObligationError::Manifest(format!(
                "missing binding {PACKAGE_MANIFEST_AUTHORITY_BINDING}"
            ))
        })?;
    context.reset_counters();
    context.step_limit = Some(STEP_LIMIT);
    let value = authority
        .apply(&mut context, Value::data(request))
        .map_err(|error| {
            ObligationError::Manifest(format!(
                "{PACKAGE_MANIFEST_AUTHORITY_BINDING} failed: {error}"
            ))
        })?;
    if let Some(error) = crate::extract_protocol_error(&context, &value) {
        return Err(ObligationError::Manifest(format!(
            "{PACKAGE_MANIFEST_AUTHORITY_BINDING} returned sealed error: {error}"
        )));
    }
    let term = value.to_plain_term().ok_or_else(|| {
        ObligationError::Manifest(format!(
            "{PACKAGE_MANIFEST_AUTHORITY_BINDING} returned non-plain result"
        ))
    })?;
    let manifest =
        decode_authorized_package_manifest(path, term, &request_hash, &transport.source_hash)
            .map_err(|error| ObligationError::Manifest(error.to_string()))?;
    Ok((manifest, transport.package_dir))
}
