use super::*;
#[path = "pkg_workspace_ops_build_artifacts.rs"]
mod build_artifacts;

#[derive(Clone, Copy)]
pub(super) struct BuildTargetProfile {
    runtime: &'static str,
    host_profile: &'static str,
    artifact_format: &'static str,
}

pub(super) fn handle_build(
    pkg: &Path,
    target: &str,
    out_dir: &Path,
    _frontend: gc_obligations::CoreformFrontend,
) -> Result<LocalPkgResult, String> {
    let target_label = normalize_build_target(target)?;
    let target_profile = build_target_profile(target_label)?;
    let artifact_layout = build_artifacts::artifact_layout_for_target(target_label)?;
    let (manifest, _) = PackageManifest::load(pkg).map_err(|e| e.to_string())?;
    let package_src = std::fs::read(pkg).map_err(|e| e.to_string())?;
    let package_h = blake3::hash(&package_src).to_hex().to_string();
    let package_artifact = gc_obligations::package_artifact_hash(pkg).map_err(|e| e.to_string())?;

    let build_manifest = Term::Map(
        [
            (
                TermOrdKey(Term::symbol(":type")),
                Term::symbol(":gcpm/build-manifest"),
            ),
            (TermOrdKey(Term::symbol(":v")), Term::Int(2.into())),
            (
                TermOrdKey(Term::symbol(":pipeline-kind")),
                Term::Str("executable-target-bundle-v2".to_string()),
            ),
            (
                TermOrdKey(Term::symbol(":target")),
                Term::Str(target_label.to_string()),
            ),
            (
                TermOrdKey(Term::symbol(":target-profile")),
                Term::Map(
                    [
                        (
                            TermOrdKey(Term::symbol(":runtime")),
                            Term::Str(target_profile.runtime.to_string()),
                        ),
                        (
                            TermOrdKey(Term::symbol(":host-profile")),
                            Term::Str(target_profile.host_profile.to_string()),
                        ),
                        (
                            TermOrdKey(Term::symbol(":artifact-format")),
                            Term::Str(target_profile.artifact_format.to_string()),
                        ),
                    ]
                    .into_iter()
                    .collect(),
                ),
            ),
            (
                TermOrdKey(Term::symbol(":verification-lanes")),
                proper_list(vec![
                    Term::symbol(":artifact-signature"),
                    Term::symbol(":boot"),
                    Term::symbol(":smoke"),
                ]),
            ),
            (
                TermOrdKey(Term::symbol(":artifact-layout")),
                Term::Map(
                    [
                        (
                            TermOrdKey(Term::symbol(":package")),
                            Term::Str(artifact_layout.package_rel.to_string()),
                        ),
                        (
                            TermOrdKey(Term::symbol(":signature")),
                            Term::Str(artifact_layout.signature_rel.to_string()),
                        ),
                        (
                            TermOrdKey(Term::symbol(":executable")),
                            Term::Str(artifact_layout.executable_rel.to_string()),
                        ),
                    ]
                    .into_iter()
                    .collect(),
                ),
            ),
            (
                TermOrdKey(Term::symbol(":package")),
                Term::Map(
                    [
                        (
                            TermOrdKey(Term::symbol(":name")),
                            Term::Str(manifest.name.clone()),
                        ),
                        (
                            TermOrdKey(Term::symbol(":version")),
                            Term::Str(manifest.version.clone()),
                        ),
                        (
                            TermOrdKey(Term::symbol(":package-h")),
                            Term::Str(package_h.clone()),
                        ),
                        (
                            TermOrdKey(Term::symbol(":package-artifact")),
                            Term::Str(package_artifact.clone()),
                        ),
                    ]
                    .into_iter()
                    .collect(),
                ),
            ),
        ]
        .into_iter()
        .collect(),
    );
    let build_manifest_src = gc_coreform::print_term(&build_manifest) + "\n";
    let bundle_h = blake3::hash(build_manifest_src.as_bytes())
        .to_hex()
        .to_string();
    let bundle_root = out_dir.join(target_label).join(&bundle_h);
    std::fs::create_dir_all(&bundle_root).map_err(|e| e.to_string())?;

    write_if_same_or_new(
        &bundle_root.join("build_manifest.gc"),
        build_manifest_src.as_bytes(),
    )
    .map_err(|e| e.to_string())?;
    write_if_same_or_new(&bundle_root.join("package.toml"), &package_src)
        .map_err(|e| e.to_string())?;
    write_if_same_or_new(
        &bundle_root.join("package_artifact.txt"),
        format!("{package_artifact}\n").as_bytes(),
    )
    .map_err(|e| e.to_string())?;

    let executable_bundle = build_artifacts::write_target_executable_bundle(
        &bundle_root,
        target_label,
        &bundle_h,
        &target_profile,
        &package_h,
        &package_artifact,
        &artifact_layout,
    )?;

    let provenance = Term::Map(
        [
            (
                TermOrdKey(Term::symbol(":type")),
                Term::symbol(":gcpm/build-provenance"),
            ),
            (TermOrdKey(Term::symbol(":v")), Term::Int(2.into())),
            (
                TermOrdKey(Term::symbol(":target")),
                Term::Str(target_label.to_string()),
            ),
            (
                TermOrdKey(Term::symbol(":bundle-h")),
                Term::Str(bundle_h.clone()),
            ),
            (
                TermOrdKey(Term::symbol(":build-manifest-h")),
                Term::Str(bundle_h.clone()),
            ),
            (
                TermOrdKey(Term::symbol(":package-artifact")),
                Term::Str(package_artifact.clone()),
            ),
            (
                TermOrdKey(Term::symbol(":artifact-sha256")),
                Term::Str(executable_bundle.package_sha256.clone()),
            ),
            (
                TermOrdKey(Term::symbol(":artifact-path")),
                Term::Str(executable_bundle.package_path.display().to_string()),
            ),
            (
                TermOrdKey(Term::symbol(":signature-path")),
                Term::Str(executable_bundle.signature_path.display().to_string()),
            ),
            (
                TermOrdKey(Term::symbol(":executable-path")),
                Term::Str(executable_bundle.executable_path.display().to_string()),
            ),
            (
                TermOrdKey(Term::symbol(":generated-by")),
                Term::Str(format!("genesis {}", env!("CARGO_PKG_VERSION"))),
            ),
        ]
        .into_iter()
        .collect(),
    );
    let provenance_src = gc_coreform::print_term(&provenance) + "\n";
    write_if_same_or_new(
        &bundle_root.join("provenance.gc"),
        provenance_src.as_bytes(),
    )
    .map_err(|e| e.to_string())?;

    let value = Term::Map(
        [
            (TermOrdKey(Term::symbol(":ok")), Term::Bool(true)),
            (
                TermOrdKey(Term::symbol(":target")),
                Term::Str(target_label.to_string()),
            ),
            (
                TermOrdKey(Term::symbol(":pkg")),
                Term::Str(pkg.display().to_string()),
            ),
            (
                TermOrdKey(Term::symbol(":pipeline-kind")),
                Term::Str("executable-target-bundle-v2".to_string()),
            ),
            (TermOrdKey(Term::symbol(":bundle-h")), Term::Str(bundle_h)),
            (
                TermOrdKey(Term::symbol(":bundle-root")),
                Term::Str(bundle_root.display().to_string()),
            ),
            (TermOrdKey(Term::symbol(":package-h")), Term::Str(package_h)),
            (
                TermOrdKey(Term::symbol(":package-artifact")),
                Term::Str(package_artifact),
            ),
            (
                TermOrdKey(Term::symbol(":artifact")),
                Term::Map(
                    [
                        (
                            TermOrdKey(Term::symbol(":package")),
                            Term::Str(executable_bundle.package_path.display().to_string()),
                        ),
                        (
                            TermOrdKey(Term::symbol(":signature")),
                            Term::Str(executable_bundle.signature_path.display().to_string()),
                        ),
                        (
                            TermOrdKey(Term::symbol(":executable")),
                            Term::Str(executable_bundle.executable_path.display().to_string()),
                        ),
                        (
                            TermOrdKey(Term::symbol(":sha256")),
                            Term::Str(executable_bundle.package_sha256),
                        ),
                    ]
                    .into_iter()
                    .collect(),
                ),
            ),
        ]
        .into_iter()
        .collect(),
    );
    Ok(LocalPkgResult {
        kind: "genesis/pkg-build-v0.1",
        log_op: "pkg-build",
        program_hash: hash_term(&value),
        value,
    })
}

fn normalize_build_target(target: &str) -> Result<&'static str, String> {
    match target.trim().to_ascii_lowercase().as_str() {
        "web" => Ok("web"),
        "desktop" => Ok("desktop"),
        "service" => Ok("service"),
        "ios" => Ok("ios"),
        "android" => Ok("android"),
        "edge" => Ok("edge"),
        "service-runtime" => Ok("service-runtime"),
        other => Err(format!(
            "invalid build target `{other}`; expected one of web|desktop|service|ios|android|edge|service-runtime"
        )),
    }
}

fn build_target_profile(target: &str) -> Result<BuildTargetProfile, String> {
    match target {
        "web" => Ok(BuildTargetProfile {
            runtime: "wasm32-unknown-unknown",
            host_profile: "browser",
            artifact_format: "wasm-bundle-v1",
        }),
        "desktop" => Ok(BuildTargetProfile {
            runtime: "native",
            host_profile: "desktop",
            artifact_format: "native-bundle-v1",
        }),
        "service" => Ok(BuildTargetProfile {
            runtime: "native",
            host_profile: "headless",
            artifact_format: "service-bundle-v1",
        }),
        "ios" => Ok(BuildTargetProfile {
            runtime: "native",
            host_profile: "mobile-ios",
            artifact_format: "ios-app-bundle-v1",
        }),
        "android" => Ok(BuildTargetProfile {
            runtime: "native",
            host_profile: "mobile-android",
            artifact_format: "android-app-bundle-v1",
        }),
        "edge" => Ok(BuildTargetProfile {
            runtime: "wasm32-wasi-preview2",
            host_profile: "edge-runtime",
            artifact_format: "edge-wasi-bundle-v1",
        }),
        "service-runtime" => Ok(BuildTargetProfile {
            runtime: "wasm32-wasi-preview2",
            host_profile: "service-runtime",
            artifact_format: "service-runtime-bundle-v1",
        }),
        other => Err(format!(
            "invalid build target `{other}`; expected one of web|desktop|service|ios|android|edge|service-runtime"
        )),
    }
}

fn write_immutable_executable(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    write_if_same_or_new(path, bytes)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let metadata = std::fs::metadata(path)?;
        let mut perms = metadata.permissions();
        let mode = perms.mode();
        if mode & 0o111 != 0o111 {
            perms.set_mode(mode | 0o755);
            std::fs::set_permissions(path, perms)?;
        }
    }
    Ok(())
}

fn proper_list(items: Vec<Term>) -> Term {
    let mut acc = Term::Nil;
    for item in items.into_iter().rev() {
        acc = Term::Pair(Box::new(item), Box::new(acc));
    }
    acc
}
