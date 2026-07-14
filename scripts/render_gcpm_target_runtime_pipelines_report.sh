#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

if [[ "$#" -ne 2 ]]; then
  echo "usage: $0 <report-output> <artifact-directory-output>" >&2
  exit 2
fi

source "$ROOT_DIR/scripts/lib/cargo_target_dir.sh"
genesis_configure_cargo_target_dir \
  "$ROOT_DIR" \
  "gcpm-target-runtime-pipelines" \
  root-host

GENESIS_BIN="${GENESIS_BIN:-$CARGO_TARGET_DIR/debug/genesis}"
REPORT_PATH="$1"
ARTIFACT_DIR="$2"
REQUIRE_NON_SYNTHETIC="${GENESIS_GCPM_TARGET_RUNTIME_REQUIRE_NON_SYNTHETIC:-}"

if [[ -z "$REQUIRE_NON_SYNTHETIC" ]]; then
  if [[ "${CI:-}" == "true" ]]; then
    REQUIRE_NON_SYNTHETIC="1"
  else
    REQUIRE_NON_SYNTHETIC="0"
  fi
fi

if [[ "$REQUIRE_NON_SYNTHETIC" != "0" && "$REQUIRE_NON_SYNTHETIC" != "1" ]]; then
  echo "gcpm-target-runtime-pipelines: GENESIS_GCPM_TARGET_RUNTIME_REQUIRE_NON_SYNTHETIC must be 0 or 1" >&2
  exit 2
fi

cargo build -p gc_cli --bin genesis >/dev/null

python3 - "$ROOT_DIR" "$GENESIS_BIN" "$REPORT_PATH" "$ARTIFACT_DIR" "$REQUIRE_NON_SYNTHETIC" <<'PY'
import datetime as dt
import hashlib
import json
import os
import pathlib
import re
import shutil
import subprocess
import sys
import tempfile
from typing import Any, Optional

root = pathlib.Path(sys.argv[1]).resolve()
genesis_bin = pathlib.Path(sys.argv[2])
report_path = pathlib.Path(sys.argv[3])
artifact_dir = pathlib.Path(sys.argv[4])
require_non_synthetic = sys.argv[5] == "1"

if not genesis_bin.is_file():
    raise SystemExit(f"gcpm-target-runtime-pipelines: missing genesis binary: {genesis_bin}")

targets = ["ios", "android", "edge", "service-runtime"]
target_specs = {
    "ios": {
        "package_rel": "artifact/package.ipa",
        "sig_rel": "artifact/package.ipa.sig",
        "launch_rel": "artifact/launch_ios.gc",
        "launch_sh_rel": "artifact/launch_ios.sh",
        "runtime_cmd_env": "GENESIS_GCPM_IOS_RUNTIME_CMD",
        "runtime_class_env": "GENESIS_GCPM_IOS_RUNTIME_CLASS",
        "default_runtime_class": "emulator",
    },
    "android": {
        "package_rel": "artifact/package.aab",
        "sig_rel": "artifact/package.aab.sig",
        "launch_rel": "artifact/launch_android.gc",
        "launch_sh_rel": "artifact/launch_android.sh",
        "runtime_cmd_env": "GENESIS_GCPM_ANDROID_RUNTIME_CMD",
        "runtime_class_env": "GENESIS_GCPM_ANDROID_RUNTIME_CLASS",
        "default_runtime_class": "emulator",
    },
    "edge": {
        "package_rel": "artifact/package.edge.wasm",
        "sig_rel": "artifact/package.edge.wasm.sig",
        "launch_rel": "artifact/launch_edge.gc",
        "launch_sh_rel": "artifact/launch_edge.sh",
        "runtime_cmd_env": "GENESIS_GCPM_EDGE_RUNTIME_CMD",
        "runtime_class_env": "GENESIS_GCPM_EDGE_RUNTIME_CLASS",
        "default_runtime_class": "container",
    },
    "service-runtime": {
        "package_rel": "artifact/package.service-runtime.wasm",
        "sig_rel": "artifact/package.service-runtime.wasm.sig",
        "launch_rel": "artifact/launch_service_runtime.gc",
        "launch_sh_rel": "artifact/launch_service_runtime.sh",
        "runtime_cmd_env": "GENESIS_GCPM_SERVICE_RUNTIME_RUNTIME_CMD",
        "runtime_class_env": "GENESIS_GCPM_SERVICE_RUNTIME_RUNTIME_CLASS",
        "default_runtime_class": "container",
    },
}

allowed_runtime_classes = {
    "emulator",
    "device",
    "container",
    "host-runtime",
    "synthetic-adapter",
}
runtime_class_fingerprints = {
    "emulator": ["simctl", "emulator", "adb"],
    "device": ["adb", "idevice", "xcodebuild", "ios-deploy"],
    "container": ["docker", "podman", "nerdctl", "kubectl"],
    "host-runtime": ["wasmtime", "node", "bun", "deno"],
}

boot_re = re.compile(r"^boot-exec-ok:([a-z-]+):([0-9a-f]{64}):([0-9a-f]{64})$")
smoke_re = re.compile(r"^smoke-exec-ok:([a-z-]+):([0-9a-f]{64}):([0-9a-f]{64})$")


def tail_text(raw: str, max_chars: int = 800) -> str:
    text = (raw or "").strip()
    if len(text) <= max_chars:
        return text
    return text[-max_chars:]


def run(cmd: list[str], *, cwd: pathlib.Path, env: Optional[dict[str, str]] = None) -> subprocess.CompletedProcess[str]:
    return subprocess.run(
        cmd,
        cwd=cwd,
        env=env,
        capture_output=True,
        text=True,
    )


def sha256_hex_bytes(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def sha256_hex_file(path: pathlib.Path) -> str:
    return sha256_hex_bytes(path.read_bytes())


def copy_required(src: pathlib.Path, dst: pathlib.Path) -> None:
    dst.parent.mkdir(parents=True, exist_ok=True)
    shutil.copy2(src, dst)


errors: list[str] = []
target_reports: list[dict[str, Any]] = []

artifact_dir.mkdir(parents=True, exist_ok=True)

with tempfile.TemporaryDirectory() as tmp:
    tmp_dir = pathlib.Path(tmp)
    workspace = tmp_dir / "runtime-pipeline"
    workspace.mkdir(parents=True, exist_ok=True)
    command_env = os.environ.copy()
    toolchain = tmp_dir / "selfhost_toolchain.gc"
    repo_toolchain = root / "selfhost" / "toolchain.gc"
    if repo_toolchain.is_file():
        shutil.copyfile(repo_toolchain, toolchain)
    else:
        artifact_proc = run(
            [str(genesis_bin), "selfhost-artifact", "--out", str(toolchain)],
            cwd=workspace,
            env=command_env,
        )
        if artifact_proc.returncode != 0:
            raise SystemExit(
                "gcpm-target-runtime-pipelines: selfhost artifact failed "
                f"(exit={artifact_proc.returncode} stderr={tail_text(artifact_proc.stderr)!r})"
            )
    command_env["GENESIS_SELFHOST_TOOLCHAIN_ARTIFACT"] = str(toolchain)

    (workspace / "caps.toml").write_text("allow = []\n", encoding="utf-8")
    (workspace / "lib.gc").write_text("(def mini::x 1)\nmini::x\n", encoding="utf-8")
    (workspace / "package.toml").write_text(
        "\n".join(
            [
                'name = "mini"',
                'version = "0.1.0"',
                "obligations = []",
                "dependencies = []",
                "",
                "[[modules]]",
                'path = "lib.gc"',
                "",
            ]
        ),
        encoding="utf-8",
    )

    pack_proc = run(
        [str(genesis_bin), "pack", "--pkg", str(workspace / "package.toml")],
        cwd=workspace,
        env=command_env,
    )
    if pack_proc.returncode != 0:
        report = {
            "kind": "genesis/gcpm-target-runtime-evidence-v0.1",
            "generated_at_utc": dt.datetime.now(dt.timezone.utc).isoformat(),
            "ok": False,
            "require_non_synthetic": require_non_synthetic,
            "errors": [
                "gcpm-target-runtime-pipelines:pack-failed",
                f"gcpm-target-runtime-pipelines:pack-exit={pack_proc.returncode}",
            ],
            "targets": [],
            "pack_stdout_tail": tail_text(pack_proc.stdout),
            "pack_stderr_tail": tail_text(pack_proc.stderr),
        }
        report_path.parent.mkdir(parents=True, exist_ok=True)
        report_path.write_text(json.dumps(report, indent=2, sort_keys=True) + "\n", encoding="utf-8")
        raise SystemExit(
            "gcpm-target-runtime-pipelines: pack failed "
            f"(exit={pack_proc.returncode})"
        )

    for target in targets:
        spec = target_specs[target]
        target_report: dict[str, Any] = {
            "target": target,
            "ok": True,
            "checks": {},
            "errors": [],
        }
        target_artifact_dir = artifact_dir / target
        target_artifact_dir.mkdir(parents=True, exist_ok=True)

        build_cmd = [
            str(genesis_bin),
            "gcpm",
            "--caps",
            str(workspace / "caps.toml"),
            "build",
            "--pkg",
            str(workspace / "package.toml"),
            "--target",
            target,
            "--out-dir",
            str(workspace / ".genesis/build-targets"),
        ]
        build_a = run(build_cmd, cwd=workspace, env=command_env)
        build_b = run(build_cmd, cwd=workspace, env=command_env)
        hash_a = build_a.stdout.strip()
        hash_b = build_b.stdout.strip()
        target_report["checks"]["build_reproducible"] = {
            "ok": build_a.returncode == 0 and build_b.returncode == 0 and hash_a == hash_b,
            "exit_code_a": build_a.returncode,
            "exit_code_b": build_b.returncode,
            "hash_a": hash_a,
            "hash_b": hash_b,
            "stderr_tail_a": tail_text(build_a.stderr),
            "stderr_tail_b": tail_text(build_b.stderr),
        }
        if build_a.returncode != 0 or build_b.returncode != 0 or hash_a != hash_b:
            err = (
                f"gcpm-target-runtime-pipelines:{target}:build-reproducibility-failed "
                f"(exit_a={build_a.returncode} exit_b={build_b.returncode} hash_a={hash_a!r} hash_b={hash_b!r})"
            )
            errors.append(err)
            target_report["errors"].append(err)
            target_report["ok"] = False
            target_reports.append(target_report)
            continue

        target_report["bundle_hash"] = hash_a
        bundle_root = workspace / ".genesis/build-targets" / target / hash_a
        target_report["bundle_root"] = target

        required = [
            bundle_root / "build_manifest.gc",
            bundle_root / "provenance.gc",
            bundle_root / "package.toml",
            bundle_root / "package_artifact.txt",
            bundle_root / spec["package_rel"],
            bundle_root / spec["sig_rel"],
            bundle_root / "artifact/entrypoint.gc",
            bundle_root / spec["launch_rel"],
            bundle_root / spec["launch_sh_rel"],
        ]
        missing = [p.relative_to(bundle_root).as_posix() for p in required if not p.is_file()]
        target_report["checks"]["required_artifacts"] = {
            "ok": not missing,
            "missing": missing,
        }
        if missing:
            err = f"gcpm-target-runtime-pipelines:{target}:missing-artifacts"
            errors.append(err)
            target_report["errors"].append(err)
            target_report["ok"] = False
            target_reports.append(target_report)
            continue

        build_manifest = bundle_root / "build_manifest.gc"
        provenance = bundle_root / "provenance.gc"
        package_artifact = bundle_root / spec["package_rel"]
        sig_artifact = bundle_root / spec["sig_rel"]
        launch_gc = bundle_root / spec["launch_rel"]
        launch_sh = bundle_root / spec["launch_sh_rel"]

        # Persist replayable artifacts for readiness and incident analysis.
        for src in [
            build_manifest,
            provenance,
            bundle_root / "package.toml",
            bundle_root / "package_artifact.txt",
            package_artifact,
            sig_artifact,
            bundle_root / "artifact/entrypoint.gc",
            launch_gc,
            launch_sh,
        ]:
            copy_required(src, target_artifact_dir / src.relative_to(bundle_root))

        manifest_text = build_manifest.read_text(encoding="utf-8")
        manifest_ok = '"executable-target-bundle-v2"' in manifest_text
        target_report["checks"]["manifest_pipeline_kind"] = {"ok": manifest_ok}
        if not manifest_ok:
            err = f"gcpm-target-runtime-pipelines:{target}:manifest-pipeline-kind-missing"
            errors.append(err)
            target_report["errors"].append(err)
            target_report["ok"] = False

        provenance_text = provenance.read_text(encoding="utf-8")
        expected_provenance_paths = {
            ":artifact-path": spec["package_rel"],
            ":signature-path": spec["sig_rel"],
            ":executable-path": spec["launch_rel"],
            ":entrypoint-path": "artifact/entrypoint.gc",
            ":launcher-path": spec["launch_sh_rel"],
        }
        provenance_path_checks = {
            key: bool(re.search(rf"{re.escape(key)}\s+{re.escape(json.dumps(value))}", provenance_text))
            for key, value in expected_provenance_paths.items()
        }
        copied_bundle_files = [
            path for path in target_artifact_dir.rglob("*") if path.is_file()
        ]
        forbidden_roots = (str(tmp_dir).encode("utf-8"), str(root).encode("utf-8"))
        leaked_bundle_paths = [
            path.relative_to(target_artifact_dir).as_posix()
            for path in copied_bundle_files
            if any(token in path.read_bytes() for token in forbidden_roots)
        ]
        provenance_paths_ok = all(provenance_path_checks.values()) and not leaked_bundle_paths
        target_report["checks"]["portable_provenance_paths"] = {
            "ok": provenance_paths_ok,
            "expected": expected_provenance_paths,
            "field_checks": provenance_path_checks,
            "leaked_bundle_paths": leaked_bundle_paths,
        }
        if not provenance_paths_ok:
            err = f"gcpm-target-runtime-pipelines:{target}:non-portable-provenance"
            errors.append(err)
            target_report["errors"].append(err)
            target_report["ok"] = False

        expected_sig = sig_artifact.read_text(encoding="utf-8").strip()
        actual_sig = sha256_hex_file(package_artifact)
        sig_ok = expected_sig == actual_sig
        target_report["checks"]["artifact_signature"] = {
            "ok": sig_ok,
            "expected": expected_sig,
            "actual": actual_sig,
        }
        if not sig_ok:
            err = f"gcpm-target-runtime-pipelines:{target}:artifact-signature-mismatch"
            errors.append(err)
            target_report["errors"].append(err)
            target_report["ok"] = False

        adapter_src = launch_gc.read_text(encoding="utf-8")
        adapter_ok = (
            ":gcpm/target-exec-adapter" in adapter_src
            and package_artifact.name in adapter_src
            and sig_artifact.name in adapter_src
            and "entrypoint.gc" in adapter_src
        )
        target_report["checks"]["launch_adapter_contract"] = {"ok": adapter_ok}
        if not adapter_ok:
            err = f"gcpm-target-runtime-pipelines:{target}:launch-adapter-contract-missing"
            errors.append(err)
            target_report["errors"].append(err)
            target_report["ok"] = False

        boot_proc = run(["bash", str(launch_sh), "--boot"], cwd=bundle_root)
        boot_out = boot_proc.stdout.strip()
        boot_match = boot_re.fullmatch(boot_out)
        boot_ok = (
            boot_proc.returncode == 0
            and boot_match is not None
            and boot_match.group(1) == target
            and boot_match.group(2) == hash_a
        )
        target_report["checks"]["boot_lane"] = {
            "ok": boot_ok,
            "exit_code": boot_proc.returncode,
            "stdout": boot_out,
            "stderr_tail": tail_text(boot_proc.stderr),
        }
        if not boot_ok:
            err = f"gcpm-target-runtime-pipelines:{target}:boot-lane-mismatch"
            errors.append(err)
            target_report["errors"].append(err)
            target_report["ok"] = False

        smoke_a = run(["bash", str(launch_sh), "--smoke"], cwd=bundle_root)
        smoke_b = run(["bash", str(launch_sh), "--smoke"], cwd=bundle_root)
        smoke_out_a = smoke_a.stdout.strip()
        smoke_out_b = smoke_b.stdout.strip()
        smoke_match_a = smoke_re.fullmatch(smoke_out_a)
        smoke_match_b = smoke_re.fullmatch(smoke_out_b)
        smoke_ok = (
            smoke_a.returncode == 0
            and smoke_b.returncode == 0
            and smoke_out_a == smoke_out_b
            and smoke_match_a is not None
            and smoke_match_b is not None
            and smoke_match_a.group(1) == target
            and smoke_match_a.group(2) == hash_a
        )
        target_report["checks"]["smoke_lane"] = {
            "ok": smoke_ok,
            "exit_code_a": smoke_a.returncode,
            "exit_code_b": smoke_b.returncode,
            "stdout_a": smoke_out_a,
            "stdout_b": smoke_out_b,
            "stderr_tail_a": tail_text(smoke_a.stderr),
            "stderr_tail_b": tail_text(smoke_b.stderr),
        }
        if not smoke_ok:
            err = f"gcpm-target-runtime-pipelines:{target}:smoke-lane-mismatch"
            errors.append(err)
            target_report["errors"].append(err)
            target_report["ok"] = False

        runtime_cmd_env = spec["runtime_cmd_env"]
        runtime_class_env = spec["runtime_class_env"]
        runtime_cmd = os.environ.get(runtime_cmd_env, "").strip()
        runtime_class = os.environ.get(runtime_class_env, "").strip()
        if runtime_cmd and not runtime_class:
            runtime_class = spec["default_runtime_class"]
        if not runtime_cmd:
            runtime_class = "synthetic-adapter"

        if runtime_class not in allowed_runtime_classes:
            err = f"gcpm-target-runtime-pipelines:{target}:invalid-runtime-class:{runtime_class}"
            errors.append(err)
            target_report["errors"].append(err)
            target_report["ok"] = False
            runtime_class = "synthetic-adapter"

        if runtime_cmd and runtime_class in runtime_class_fingerprints:
            lower_cmd = runtime_cmd.lower()
            fingerprints = runtime_class_fingerprints[runtime_class]
            if not any(token in lower_cmd for token in fingerprints):
                err = (
                    f"gcpm-target-runtime-pipelines:{target}:runtime-command-class-fingerprint-missing:"
                    f"class={runtime_class}"
                )
                errors.append(err)
                target_report["errors"].append(err)
                target_report["ok"] = False

        runtime_env = command_env.copy()
        runtime_env.update(
            {
                "GENESIS_TARGET": target,
                "GENESIS_TARGET_BUNDLE_ROOT": str(bundle_root),
                "GENESIS_TARGET_PACKAGE": str(package_artifact),
                "GENESIS_TARGET_SIGNATURE": str(sig_artifact),
                "GENESIS_TARGET_LAUNCH_SCRIPT": str(launch_sh),
                "GENESIS_TARGET_HASH": hash_a,
                "GENESIS_TARGET_ARTIFACT_DIR": str(target_artifact_dir),
            }
        )

        runtime_mode = "synthetic-adapter"
        runtime_proc = None
        if runtime_cmd:
            runtime_mode = "non-synthetic"
            runtime_proc = run(["bash", "-lc", runtime_cmd], cwd=bundle_root, env=runtime_env)
            (target_artifact_dir / "runtime_command.txt").write_text(
                runtime_cmd + "\n",
                encoding="utf-8",
            )
            (target_artifact_dir / "runtime_stdout.log").write_text(
                runtime_proc.stdout,
                encoding="utf-8",
            )
            (target_artifact_dir / "runtime_stderr.log").write_text(
                runtime_proc.stderr,
                encoding="utf-8",
            )
        else:
            synthetic_log = "\n".join([boot_out, smoke_out_a]) + "\n"
            (target_artifact_dir / "runtime_command.txt").write_text(
                f"synthetic-adapter:{spec['launch_sh_rel']} --boot/--smoke\n",
                encoding="utf-8",
            )
            (target_artifact_dir / "runtime_stdout.log").write_text(
                synthetic_log,
                encoding="utf-8",
            )
            (target_artifact_dir / "runtime_stderr.log").write_text("", encoding="utf-8")

        runtime_stdout = (
            runtime_proc.stdout
            if runtime_proc is not None
            else (target_artifact_dir / "runtime_stdout.log").read_text(encoding="utf-8")
        )
        runtime_stderr = (
            runtime_proc.stderr
            if runtime_proc is not None
            else ""
        )
        runtime_exit = runtime_proc.returncode if runtime_proc is not None else 0
        runtime_ok = runtime_exit == 0
        if not runtime_ok:
            err = f"gcpm-target-runtime-pipelines:{target}:runtime-command-failed:exit={runtime_exit}"
            errors.append(err)
            target_report["errors"].append(err)
            target_report["ok"] = False

        if require_non_synthetic and runtime_mode != "non-synthetic":
            err = f"gcpm-target-runtime-pipelines:{target}:non-synthetic-runtime-evidence-required"
            errors.append(err)
            target_report["errors"].append(err)
            target_report["ok"] = False

        runtime_evidence = {
            "mode": runtime_mode,
            "class": runtime_class,
            "command_env": runtime_cmd_env,
            "class_env": runtime_class_env,
            "command": runtime_cmd if runtime_cmd else f"{spec['launch_sh_rel']} --boot/--smoke",
            "exit_code": runtime_exit,
            "ok": runtime_ok,
            "stdout_tail": tail_text(runtime_stdout),
            "stderr_tail": tail_text(runtime_stderr),
            "stdout_sha256": sha256_hex_bytes(runtime_stdout.encode("utf-8")),
            "stderr_sha256": sha256_hex_bytes(runtime_stderr.encode("utf-8")),
            "replay_artifact_dir": target,
        }
        target_report["runtime_evidence"] = runtime_evidence

        replay_files = []
        for path in sorted(target_artifact_dir.rglob("*")):
            if not path.is_file() or path.name == "runtime_evidence.json":
                continue
            replay_files.append(
                {
                    "path": path.relative_to(target_artifact_dir).as_posix(),
                    "sha256": sha256_hex_file(path),
                    "size_bytes": path.stat().st_size,
                }
            )
        replay_manifest_bytes = json.dumps(
            replay_files,
            sort_keys=True,
            separators=(",", ":"),
        ).encode("utf-8")
        target_report["replay_artifacts"] = {
            "root": target,
            "file_count": len(replay_files),
            "files": replay_files,
            "tree_sha256": sha256_hex_bytes(replay_manifest_bytes),
        }

        (target_artifact_dir / "runtime_evidence.json").write_text(
            json.dumps(
                {
                    "target": target,
                    "bundle_hash": hash_a,
                    "runtime_evidence": runtime_evidence,
                    "replay_artifacts": target_report["replay_artifacts"],
                    "checks": target_report["checks"],
                },
                indent=2,
                sort_keys=True,
            )
            + "\n",
            encoding="utf-8",
        )

        target_reports.append(target_report)

report = {
    "kind": "genesis/gcpm-target-runtime-evidence-v0.1",
    "generated_at_utc": dt.datetime.now(dt.timezone.utc).isoformat(),
    "ok": not errors,
    "require_non_synthetic": require_non_synthetic,
    "ci_context": os.environ.get("CI", "") == "true",
    "targets": target_reports,
    "errors": errors,
}
report_path.parent.mkdir(parents=True, exist_ok=True)
report_path.write_text(json.dumps(report, indent=2, sort_keys=True) + "\n", encoding="utf-8")

if errors:
    raise SystemExit(
        "gcpm-target-runtime-pipelines: failed "
        f"(targets={len(target_reports)} errors={len(errors)})"
    )

print(
    "gcpm-target-runtime-pipelines: ok "
    f"targets={' '.join(targets)} "
    f"require_non_synthetic={str(require_non_synthetic).lower()} "
    f"report={report_path.as_posix()}"
)
PY
