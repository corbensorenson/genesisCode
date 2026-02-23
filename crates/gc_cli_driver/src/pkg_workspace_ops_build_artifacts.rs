use super::*;
use sha2::{Digest, Sha256};

pub(super) struct TargetArtifactLayout {
    pub(super) package_rel: &'static str,
    pub(super) signature_rel: &'static str,
    pub(super) executable_rel: &'static str,
}

pub(super) struct TargetExecutableBundle {
    pub(super) package_path: PathBuf,
    pub(super) signature_path: PathBuf,
    pub(super) executable_path: PathBuf,
    pub(super) package_sha256: String,
}

pub(super) fn artifact_layout_for_target(target: &str) -> Result<TargetArtifactLayout, String> {
    let layout = match target {
        "web" => TargetArtifactLayout {
            package_rel: "artifact/package.webbundle",
            signature_rel: "artifact/package.webbundle.sig",
            executable_rel: "artifact/launch_web.sh",
        },
        "desktop" => TargetArtifactLayout {
            package_rel: "artifact/package.desktop.app",
            signature_rel: "artifact/package.desktop.app.sig",
            executable_rel: "artifact/launch_desktop.sh",
        },
        "service" => TargetArtifactLayout {
            package_rel: "artifact/package.service.bin",
            signature_rel: "artifact/package.service.bin.sig",
            executable_rel: "artifact/launch_service.sh",
        },
        "ios" => TargetArtifactLayout {
            package_rel: "artifact/package.ipa",
            signature_rel: "artifact/package.ipa.sig",
            executable_rel: "artifact/launch_ios.sh",
        },
        "android" => TargetArtifactLayout {
            package_rel: "artifact/package.aab",
            signature_rel: "artifact/package.aab.sig",
            executable_rel: "artifact/launch_android.sh",
        },
        "edge" => TargetArtifactLayout {
            package_rel: "artifact/package.edge.wasm",
            signature_rel: "artifact/package.edge.wasm.sig",
            executable_rel: "artifact/launch_edge.sh",
        },
        "service-runtime" => TargetArtifactLayout {
            package_rel: "artifact/package.service-runtime.wasm",
            signature_rel: "artifact/package.service-runtime.wasm.sig",
            executable_rel: "artifact/launch_service_runtime.sh",
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
    bundle_root: &Path,
    target_label: &str,
    bundle_h: &str,
    target_profile: &BuildTargetProfile,
    package_h: &str,
    package_artifact: &str,
    layout: &TargetArtifactLayout,
) -> Result<TargetExecutableBundle, String> {
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
        ]
        .into_iter()
        .collect(),
    );
    let package_src = gc_coreform::print_term(&package_term) + "\n";
    let package_sha256 = sha256_hex(package_src.as_bytes());

    let package_path = bundle_root.join(layout.package_rel);
    let signature_path = bundle_root.join(layout.signature_rel);
    let executable_path = bundle_root.join(layout.executable_rel);

    write_if_same_or_new(&package_path, package_src.as_bytes()).map_err(|e| e.to_string())?;
    write_if_same_or_new(&signature_path, format!("{package_sha256}\n").as_bytes())
        .map_err(|e| e.to_string())?;

    let launch_script = format!(
        r#"#!/usr/bin/env bash
set -euo pipefail
MODE="${{1:---boot}}"
SCRIPT_DIR="$(cd "$(dirname "${{BASH_SOURCE[0]}}")" && pwd)"
PKG_PATH="$SCRIPT_DIR/{pkg_rel_name}"
SIG_PATH="$SCRIPT_DIR/{sig_rel_name}"
TARGET="{target}"
BUNDLE_H="{bundle_h}"
EXPECTED_SIG="$(tr -d '\r\n' < "$SIG_PATH")"
ACTUAL_SIG="$(python3 - "$PKG_PATH" <<'PY'
import hashlib
import pathlib
import sys
path = pathlib.Path(sys.argv[1])
print(hashlib.sha256(path.read_bytes()).hexdigest())
PY
)"
if [[ "$ACTUAL_SIG" != "$EXPECTED_SIG" ]]; then
  echo "signature-mismatch:$TARGET:$ACTUAL_SIG:$EXPECTED_SIG" >&2
  exit 2
fi
case "$MODE" in
  --boot)
    echo "boot-ok:$TARGET:$BUNDLE_H"
    ;;
  --smoke)
    echo "smoke-ok:$TARGET:$BUNDLE_H"
    ;;
  *)
    echo "usage: launch.sh [--boot|--smoke]" >&2
    exit 64
    ;;
esac
"#,
        pkg_rel_name = relative_name(layout.package_rel),
        sig_rel_name = relative_name(layout.signature_rel),
        target = target_label,
        bundle_h = bundle_h
    );
    write_immutable_executable(&executable_path, launch_script.as_bytes())
        .map_err(|e| e.to_string())?;

    Ok(TargetExecutableBundle {
        package_path,
        signature_path,
        executable_path,
        package_sha256,
    })
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
