use std::path::{Path, PathBuf};

use gc_coreform::{Term, TermOrdKey, hash_term, print_term};
use gc_kernel::{Apply, EvalCtx, MemLimits, Value};
use gc_pkg::{
    PACKAGE_MANIFEST_AUTHORITY_BINDING, PackageManifest, decode_authorized_package_manifest,
    package_manifest_authority_request, read_package_manifest_transport,
};
use gc_prelude::{build_prelude, load_selfhost_coreform_toolchain_v1_with_mode};

use crate::EffectsError;
use crate::policy::SelfhostAuthorityConfig;

const STEP_LIMIT: u64 = 20_000_000;
const ALLOC_LIMIT: u64 = 80_000_000;

pub(crate) struct PkgPackageManifestAuthority {
    authority: Value,
    context: EvalCtx,
}

impl PkgPackageManifestAuthority {
    pub(crate) fn required_for_request(op: &str, payload: &Term) -> bool {
        if matches!(
            op,
            "core/pkg-low::load-package"
                | "core/pkg-low::snapshot"
                | "editor/task::test-pkg"
                | "editor/task::typecheck-pkg"
        ) {
            return true;
        }
        if op != "editor/task::spawn" {
            return false;
        }
        let Term::Map(fields) = payload else {
            return false;
        };
        matches!(
            fields.get(&TermOrdKey(Term::symbol(":task-kind"))),
            Some(Term::Symbol(kind)) | Some(Term::Str(kind))
                if matches!(
                    kind.as_str(),
                    "editor/task::build-pkg"
                        | "editor/task::debug-pkg"
                        | "editor/task::index-workspace"
                        | "editor/task::run-pkg"
                        | "editor/task::test-pkg"
                        | "editor/task::typecheck-pkg"
                )
        )
    }

    pub(crate) fn load(config: &SelfhostAuthorityConfig) -> Result<Self, EffectsError> {
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
        .map_err(|error| authority_error(format!("artifact bootstrap failed: {error:#}")))?;
        let authority = environment
            .get(PACKAGE_MANIFEST_AUTHORITY_BINDING)
            .ok_or_else(|| {
                authority_error(format!(
                    "missing binding {PACKAGE_MANIFEST_AUTHORITY_BINDING}"
                ))
            })?;
        context.reset_counters();
        context.step_limit = Some(STEP_LIMIT);
        Ok(Self { authority, context })
    }

    pub(crate) fn load_manifest(
        &mut self,
        path: &Path,
    ) -> Result<(PackageManifest, PathBuf), EffectsError> {
        let transport = read_package_manifest_transport(path)
            .map_err(|error| authority_error(format!("manifest transport failed: {error}")))?;
        let request =
            package_manifest_authority_request(transport.document, &transport.source_hash);
        let request_hash = blake3::Hash::from_bytes(hash_term(&request))
            .to_hex()
            .to_string();
        self.context.reset_counters();
        self.context.step_limit = Some(STEP_LIMIT);
        let value = self
            .authority
            .clone()
            .apply(&mut self.context, Value::data(request))
            .map_err(|error| authority_error(format!("apply failed: {error}")))?;
        let term = plain_result(value, &self.context)?;
        let manifest =
            decode_authorized_package_manifest(path, term, &request_hash, &transport.source_hash)
                .map_err(|error| authority_error(format!("result rejected: {error}")))?;
        Ok((manifest, transport.package_dir))
    }
}

fn plain_result(value: Value, context: &EvalCtx) -> Result<Term, EffectsError> {
    if let Value::Sealed { token, payload } = &value
        && context
            .protocol
            .is_some_and(|protocol| *token == protocol.error)
    {
        let detail = payload
            .to_plain_term()
            .map(|term| print_term(&term))
            .unwrap_or_else(|| "<opaque-error-payload>".to_string());
        return Err(authority_error(format!("returned sealed ERROR {detail}")));
    }
    value
        .to_plain_term()
        .ok_or_else(|| authority_error(format!("returned opaque value: {value:?}")))
}

fn authority_error(message: impl Into<String>) -> EffectsError {
    EffectsError::Log(format!(
        "selfhost package manifest authority: {}",
        message.into()
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn artifact() -> PathBuf {
        let workspace = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
        std::env::var_os("GENESIS_TEST_SELFHOST_ARTIFACT")
            .or_else(|| std::env::var_os("GENESIS_SELFHOST_TOOLCHAIN_ARTIFACT"))
            .map(PathBuf::from)
            .map(|path| {
                if path.is_absolute() {
                    path
                } else {
                    workspace.join(path)
                }
            })
            .unwrap_or_else(|| workspace.join("selfhost/toolchain.gc"))
    }

    #[test]
    fn request_classifier_is_narrow_and_covers_manifest_consumers() {
        assert!(PkgPackageManifestAuthority::required_for_request(
            "core/pkg-low::load-package",
            &Term::Nil
        ));
        let spawn = Term::Map(
            [(
                TermOrdKey(Term::symbol(":task-kind")),
                Term::symbol("editor/task::index-workspace"),
            )]
            .into_iter()
            .collect(),
        );
        assert!(PkgPackageManifestAuthority::required_for_request(
            "editor/task::spawn",
            &spawn
        ));
        assert!(!PkgPackageManifestAuthority::required_for_request(
            "editor/task::parse-module",
            &Term::Nil
        ));
        assert!(!PkgPackageManifestAuthority::required_for_request(
            "editor/task::spawn",
            &Term::Nil
        ));
    }

    #[test]
    fn artifact_authority_normalizes_manifest_without_native_fallback() {
        let config = SelfhostAuthorityConfig {
            bootstrap_mode: gc_prelude::SelfhostBootstrapMode::ArtifactOnly,
            artifact: Some(artifact()),
        };
        let mut authority = PkgPackageManifestAuthority::load(&config).expect("load authority");
        let directory = tempfile::tempdir().expect("temporary package");
        let manifest_path = directory.path().join("package.toml");
        std::fs::write(
            &manifest_path,
            "name = \"effects-authority\"\nversion = \"0.1.0\"\nobligations = []\n[[modules]]\npath = \"main.gc\"\n",
        )
        .expect("write manifest");
        let (manifest, package_dir) = authority
            .load_manifest(&manifest_path)
            .expect("authorize manifest");
        assert_eq!(manifest.schema, 1);
        assert!(manifest.dependencies.is_empty());
        assert_eq!(manifest.modules[0].path, "main.gc");
        assert_eq!(package_dir, directory.path());
    }
}
