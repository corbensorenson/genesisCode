use super::*;
use sha2::{Digest, Sha256};

pub(super) struct TargetArtifactLayout {
    pub(super) package_rel: &'static str,
    pub(super) signature_rel: &'static str,
    pub(super) executable_rel: &'static str,
    pub(super) launcher_rel: &'static str,
    pub(super) entrypoint_rel: &'static str,
}

pub(super) struct TargetExecutableBundle {
    pub(super) package_path: PathBuf,
    pub(super) signature_path: PathBuf,
    pub(super) executable_path: PathBuf,
    pub(super) launcher_path: PathBuf,
    pub(super) entrypoint_path: PathBuf,
    pub(super) package_sha256: String,
    pub(super) entrypoint_h: String,
}

pub(super) struct TargetExecutableBundleInput<'a> {
    pub(super) bundle_root: &'a Path,
    pub(super) target_label: &'a str,
    pub(super) bundle_h: &'a str,
    pub(super) target_profile: &'a BuildTargetProfile,
    pub(super) package_h: &'a str,
    pub(super) package_artifact: &'a str,
    pub(super) layout: &'a TargetArtifactLayout,
    pub(super) entrypoint_src: &'a str,
}

pub(super) fn artifact_layout_for_target(target: &str) -> Result<TargetArtifactLayout, String> {
    let layout = match target {
        "web" => TargetArtifactLayout {
            package_rel: "artifact/package.webbundle",
            signature_rel: "artifact/package.webbundle.sig",
            executable_rel: "artifact/launch_web.gc",
            launcher_rel: "artifact/launch_web.sh",
            entrypoint_rel: "artifact/entrypoint.gc",
        },
        "desktop" => TargetArtifactLayout {
            package_rel: "artifact/package.desktop.app",
            signature_rel: "artifact/package.desktop.app.sig",
            executable_rel: "artifact/launch_desktop.gc",
            launcher_rel: "artifact/launch_desktop.sh",
            entrypoint_rel: "artifact/entrypoint.gc",
        },
        "service" => TargetArtifactLayout {
            package_rel: "artifact/package.service.bin",
            signature_rel: "artifact/package.service.bin.sig",
            executable_rel: "artifact/launch_service.gc",
            launcher_rel: "artifact/launch_service.sh",
            entrypoint_rel: "artifact/entrypoint.gc",
        },
        "ios" => TargetArtifactLayout {
            package_rel: "artifact/package.ipa",
            signature_rel: "artifact/package.ipa.sig",
            executable_rel: "artifact/launch_ios.gc",
            launcher_rel: "artifact/launch_ios.sh",
            entrypoint_rel: "artifact/entrypoint.gc",
        },
        "android" => TargetArtifactLayout {
            package_rel: "artifact/package.aab",
            signature_rel: "artifact/package.aab.sig",
            executable_rel: "artifact/launch_android.gc",
            launcher_rel: "artifact/launch_android.sh",
            entrypoint_rel: "artifact/entrypoint.gc",
        },
        "edge" => TargetArtifactLayout {
            package_rel: "artifact/package.edge.wasm",
            signature_rel: "artifact/package.edge.wasm.sig",
            executable_rel: "artifact/launch_edge.gc",
            launcher_rel: "artifact/launch_edge.sh",
            entrypoint_rel: "artifact/entrypoint.gc",
        },
        "service-runtime" => TargetArtifactLayout {
            package_rel: "artifact/package.service-runtime.wasm",
            signature_rel: "artifact/package.service-runtime.wasm.sig",
            executable_rel: "artifact/launch_service_runtime.gc",
            launcher_rel: "artifact/launch_service_runtime.sh",
            entrypoint_rel: "artifact/entrypoint.gc",
        },
        other => {
            return Err(format!(
                "unsupported target `{other}` for executable artifact layout"
            ));
        }
    };
    Ok(layout)
}

pub(super) fn write_target_executable_bundle(
    input: TargetExecutableBundleInput<'_>,
) -> Result<TargetExecutableBundle, String> {
    let TargetExecutableBundleInput {
        bundle_root,
        target_label,
        bundle_h,
        target_profile,
        package_h,
        package_artifact,
        layout,
        entrypoint_src,
    } = input;
    let entrypoint_h = blake3::hash(entrypoint_src.as_bytes()).to_hex().to_string();
    let package_term = Term::Map(
        [
            (
                TermOrdKey(Term::symbol(":type")),
                Term::symbol(":gcpm/target-package"),
            ),
            (TermOrdKey(Term::symbol(":v")), Term::Int(1.into())),
            (
                TermOrdKey(Term::symbol(":target")),
                Term::Str(target_label.to_string()),
            ),
            (
                TermOrdKey(Term::symbol(":artifact-format")),
                Term::Str(target_profile.artifact_format.to_string()),
            ),
            (
                TermOrdKey(Term::symbol(":runtime")),
                Term::Str(target_profile.runtime.to_string()),
            ),
            (
                TermOrdKey(Term::symbol(":host-profile")),
                Term::Str(target_profile.host_profile.to_string()),
            ),
            (
                TermOrdKey(Term::symbol(":bundle-h")),
                Term::Str(bundle_h.to_string()),
            ),
            (
                TermOrdKey(Term::symbol(":package-h")),
                Term::Str(package_h.to_string()),
            ),
            (
                TermOrdKey(Term::symbol(":package-artifact")),
                Term::Str(package_artifact.to_string()),
            ),
            (
                TermOrdKey(Term::symbol(":execution-lanes")),
                proper_list(vec![Term::symbol(":boot"), Term::symbol(":smoke")]),
            ),
            (
                TermOrdKey(Term::symbol(":entrypoint")),
                Term::Map(
                    [
                        (
                            TermOrdKey(Term::symbol(":path")),
                            Term::Str(relative_name(layout.entrypoint_rel)),
                        ),
                        (
                            TermOrdKey(Term::symbol(":hash")),
                            Term::Str(entrypoint_h.clone()),
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
    let package_src = gc_coreform::print_term(&package_term) + "\n";
    let package_sha256 = sha256_hex(package_src.as_bytes());

    let package_path = bundle_root.join(layout.package_rel);
    let signature_path = bundle_root.join(layout.signature_rel);
    let executable_path = bundle_root.join(layout.executable_rel);
    let launcher_path = bundle_root.join(layout.launcher_rel);
    let entrypoint_path = bundle_root.join(layout.entrypoint_rel);

    write_if_same_or_new(&package_path, package_src.as_bytes()).map_err(|e| e.to_string())?;
    write_if_same_or_new(&signature_path, format!("{package_sha256}\n").as_bytes())
        .map_err(|e| e.to_string())?;
    write_if_same_or_new(&entrypoint_path, entrypoint_src.as_bytes()).map_err(|e| e.to_string())?;

    let launch_adapter = Term::Map(
        [
            (
                TermOrdKey(Term::symbol(":type")),
                Term::symbol(":gcpm/target-exec-adapter"),
            ),
            (TermOrdKey(Term::symbol(":v")), Term::Int(1.into())),
            (
                TermOrdKey(Term::symbol(":target")),
                Term::Str(target_label.to_string()),
            ),
            (
                TermOrdKey(Term::symbol(":bundle-h")),
                Term::Str(bundle_h.to_string()),
            ),
            (
                TermOrdKey(Term::symbol(":lanes")),
                proper_list(vec![Term::symbol(":boot"), Term::symbol(":smoke")]),
            ),
            (
                TermOrdKey(Term::symbol(":verify")),
                Term::Map(
                    [(
                        TermOrdKey(Term::symbol(":sha256")),
                        Term::Map(
                            [
                                (
                                    TermOrdKey(Term::symbol(":package")),
                                    Term::Str(relative_name(layout.package_rel)),
                                ),
                                (
                                    TermOrdKey(Term::symbol(":signature")),
                                    Term::Str(relative_name(layout.signature_rel)),
                                ),
                            ]
                            .into_iter()
                            .collect(),
                        ),
                    )]
                    .into_iter()
                    .collect(),
                ),
            ),
            (
                TermOrdKey(Term::symbol(":entrypoint")),
                Term::Map(
                    [
                        (
                            TermOrdKey(Term::symbol(":path")),
                            Term::Str(relative_name(layout.entrypoint_rel)),
                        ),
                        (
                            TermOrdKey(Term::symbol(":hash")),
                            Term::Str(entrypoint_h.clone()),
                        ),
                    ]
                    .into_iter()
                    .collect(),
                ),
            ),
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
    );
    let launch_adapter_src = gc_coreform::print_term(&launch_adapter) + "\n";
    write_if_same_or_new(&executable_path, launch_adapter_src.as_bytes())
        .map_err(|e| e.to_string())?;
    let launch_script_src = render_launch_script(
        target_label,
        bundle_h,
        &relative_name(layout.package_rel),
        &relative_name(layout.signature_rel),
        &relative_name(layout.entrypoint_rel),
    );
    write_if_same_or_new(&launcher_path, launch_script_src.as_bytes())
        .map_err(|e| e.to_string())?;

    #[cfg(unix)]
    {
        let metadata = std::fs::metadata(&launcher_path).map_err(|e| e.to_string())?;
        let mut perms = metadata.permissions();
        std::os::unix::fs::PermissionsExt::set_mode(&mut perms, 0o755);
        std::fs::set_permissions(&launcher_path, perms).map_err(|e| e.to_string())?;
    }

    Ok(TargetExecutableBundle {
        package_path,
        signature_path,
        executable_path,
        launcher_path,
        entrypoint_path,
        package_sha256,
        entrypoint_h,
    })
}

fn render_launch_script(
    target_label: &str,
    bundle_h: &str,
    package_name: &str,
    signature_name: &str,
    entrypoint_name: &str,
) -> String {
    format!(
        r#"#!/usr/bin/env bash
set -euo pipefail

script_dir="$(cd "$(dirname "${{BASH_SOURCE[0]}}")" && pwd)"
target="{target_label}"
bundle_h="{bundle_h}"
package_path="${{script_dir}}/{package_name}"
signature_path="${{script_dir}}/{signature_name}"
entrypoint_path="${{script_dir}}/{entrypoint_name}"
lane_flag="${{1:-}}"

if [[ ! -f "$package_path" || ! -f "$signature_path" || ! -f "$entrypoint_path" ]]; then
  echo "target-launcher: missing required artifact(s)" >&2
  exit 1
fi

if [[ "$lane_flag" != "--boot" && "$lane_flag" != "--smoke" ]]; then
  echo "usage: $(basename "$0") --boot|--smoke" >&2
  exit 64
fi

expected_sig="$(tr -d '\r\n' < "$signature_path")"
actual_sig="$(python3 - "$package_path" <<'PY'
import hashlib
import pathlib
import sys
print(hashlib.sha256(pathlib.Path(sys.argv[1]).read_bytes()).hexdigest())
PY
)"
if [[ "$actual_sig" != "$expected_sig" ]]; then
  echo "target-launcher: signature mismatch" >&2
  exit 1
fi

lane="boot"
if [[ "$lane_flag" == "--smoke" ]]; then
  lane="smoke"
fi

entrypoint_payload="$(tr -d '\r\n' < "$entrypoint_path")"
lane_hash="$(python3 - "$lane" "$target" "$bundle_h" "$entrypoint_payload" "$actual_sig" <<'PY'
import hashlib
import sys
lane, target, bundle_h, entrypoint_payload, artifact_sha = sys.argv[1:6]
payload = "|".join([lane, target, bundle_h, entrypoint_payload, artifact_sha]).encode("utf-8")
print(hashlib.sha256(payload).hexdigest())
PY
)"

echo "${{lane}}-exec-ok:${{target}}:${{bundle_h}}:${{lane_hash}}"
"#
    )
}

fn relative_name(path: &str) -> String {
    Path::new(path)
        .file_name()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| path.to_string())
}

fn sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    format!("{:x}", hasher.finalize())
}
