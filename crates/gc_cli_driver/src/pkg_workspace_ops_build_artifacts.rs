use super::*;
use sha2::{Digest, Sha256};

pub(super) struct TargetArtifactLayout {
    pub(super) package_rel: &'static str,
    pub(super) signature_rel: &'static str,
    pub(super) executable_rel: &'static str,
    pub(super) entrypoint_rel: &'static str,
}

pub(super) struct TargetExecutableBundle {
    pub(super) package_path: PathBuf,
    pub(super) signature_path: PathBuf,
    pub(super) executable_path: PathBuf,
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
            executable_rel: "artifact/launch_web.sh",
            entrypoint_rel: "artifact/entrypoint.gc",
        },
        "desktop" => TargetArtifactLayout {
            package_rel: "artifact/package.desktop.app",
            signature_rel: "artifact/package.desktop.app.sig",
            executable_rel: "artifact/launch_desktop.sh",
            entrypoint_rel: "artifact/entrypoint.gc",
        },
        "service" => TargetArtifactLayout {
            package_rel: "artifact/package.service.bin",
            signature_rel: "artifact/package.service.bin.sig",
            executable_rel: "artifact/launch_service.sh",
            entrypoint_rel: "artifact/entrypoint.gc",
        },
        "ios" => TargetArtifactLayout {
            package_rel: "artifact/package.ipa",
            signature_rel: "artifact/package.ipa.sig",
            executable_rel: "artifact/launch_ios.sh",
            entrypoint_rel: "artifact/entrypoint.gc",
        },
        "android" => TargetArtifactLayout {
            package_rel: "artifact/package.aab",
            signature_rel: "artifact/package.aab.sig",
            executable_rel: "artifact/launch_android.sh",
            entrypoint_rel: "artifact/entrypoint.gc",
        },
        "edge" => TargetArtifactLayout {
            package_rel: "artifact/package.edge.wasm",
            signature_rel: "artifact/package.edge.wasm.sig",
            executable_rel: "artifact/launch_edge.sh",
            entrypoint_rel: "artifact/entrypoint.gc",
        },
        "service-runtime" => TargetArtifactLayout {
            package_rel: "artifact/package.service-runtime.wasm",
            signature_rel: "artifact/package.service-runtime.wasm.sig",
            executable_rel: "artifact/launch_service_runtime.sh",
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
    let entrypoint_path = bundle_root.join(layout.entrypoint_rel);

    write_if_same_or_new(&package_path, package_src.as_bytes()).map_err(|e| e.to_string())?;
    write_if_same_or_new(&signature_path, format!("{package_sha256}\n").as_bytes())
        .map_err(|e| e.to_string())?;
    write_if_same_or_new(&entrypoint_path, entrypoint_src.as_bytes()).map_err(|e| e.to_string())?;

    let launch_script = format!(
        r#"#!/usr/bin/env bash
set -euo pipefail
MODE="${{1:---boot}}"
SCRIPT_DIR="$(cd "$(dirname "${{BASH_SOURCE[0]}}")" && pwd)"
PKG_PATH="$SCRIPT_DIR/{pkg_rel_name}"
SIG_PATH="$SCRIPT_DIR/{sig_rel_name}"
ENTRYPOINT_PATH="$SCRIPT_DIR/{entrypoint_rel_name}"
TARGET="{target}"
BUNDLE_H="{bundle_h}"
GENESIS_BIN="${{GENESIS_BIN:-genesis}}"
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
if ! command -v "$GENESIS_BIN" >/dev/null 2>&1; then
  echo "genesis-bin-not-found:$TARGET:$GENESIS_BIN" >&2
  exit 127
fi
hash_text() {{
  python3 - <<'PY'
import hashlib
import sys
print(hashlib.sha256(sys.stdin.buffer.read()).hexdigest())
PY
}}
run_entrypoint() {{
  "$GENESIS_BIN" eval "$ENTRYPOINT_PATH"
}}
case "$MODE" in
  --boot)
    BOOT_OUT="$(run_entrypoint)"
    BOOT_H="$(printf "%s" "$BOOT_OUT" | hash_text)"
    echo "boot-exec-ok:$TARGET:$BUNDLE_H:$BOOT_H"
    ;;
  --smoke)
    SMOKE_A="$(run_entrypoint)"
    SMOKE_B="$(run_entrypoint)"
    if [[ "$SMOKE_A" != "$SMOKE_B" ]]; then
      echo "smoke-nondeterministic:$TARGET:$BUNDLE_H" >&2
      exit 3
    fi
    SMOKE_H="$(printf "%s" "$SMOKE_A" | hash_text)"
    echo "smoke-exec-ok:$TARGET:$BUNDLE_H:$SMOKE_H"
    ;;
  *)
    echo "usage: launch.sh [--boot|--smoke]" >&2
    exit 64
    ;;
esac
"#,
        pkg_rel_name = relative_name(layout.package_rel),
        sig_rel_name = relative_name(layout.signature_rel),
        entrypoint_rel_name = relative_name(layout.entrypoint_rel),
        target = target_label,
        bundle_h = bundle_h
    );
    write_immutable_executable(&executable_path, launch_script.as_bytes())
        .map_err(|e| e.to_string())?;

    Ok(TargetExecutableBundle {
        package_path,
        signature_path,
        executable_path,
        entrypoint_path,
        package_sha256,
        entrypoint_h,
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
