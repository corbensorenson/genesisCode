use assert_cmd::cargo::cargo_bin_cmd;
use predicates::prelude::*;
use serde_json::Value as JsonValue;
use tempfile::tempdir;

fn build_selfhost_artifact(dir: &std::path::Path) -> std::path::PathBuf {
    let artifact = dir.join("selfhost_toolchain.gc");
    cargo_bin_cmd!("genesis")
        .args(["selfhost-artifact", "--out"])
        .arg(&artifact)
        .assert()
        .success();
    artifact
}

#[test]
fn selfhost_only_rejects_rust_engine_for_fmt() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("m.gc");
    std::fs::write(&file, "(def x 1)\n").unwrap();

    cargo_bin_cmd!("genesis")
        .args([
            "--selfhost-only",
            "fmt",
            file.to_str().unwrap(),
            "--engine",
            "rust",
        ])
        .assert()
        .failure()
        .code(50)
        .stderr(predicate::str::contains(
            "selfhost-only mode requires --engine selfhost",
        ));
}

#[test]
fn selfhost_only_rejects_non_artifact_bootstrap_mode() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("m.gc");
    std::fs::write(&file, "(def x 1)\n").unwrap();

    cargo_bin_cmd!("genesis")
        .args([
            "--selfhost-only",
            "--selfhost-bootstrap",
            "embedded",
            "fmt",
            file.to_str().unwrap(),
            "--engine",
            "selfhost",
        ])
        .assert()
        .failure()
        .code(50)
        .stderr(predicate::str::contains(
            "selfhost-only mode requires --selfhost-bootstrap artifact-only",
        ));
}

#[test]
fn selfhost_only_accepts_fmt_selfhost_with_artifact() {
    let dir = tempdir().unwrap();
    let artifact = build_selfhost_artifact(dir.path());
    let file = dir.path().join("m.gc");
    std::fs::write(&file, "(def  x 1)\n x\n").unwrap();

    cargo_bin_cmd!("genesis")
        .args([
            "--selfhost-only",
            "--no-step-limit",
            "--selfhost-artifact",
            artifact.to_str().unwrap(),
            "fmt",
            file.to_str().unwrap(),
            "--engine",
            "selfhost",
        ])
        .assert()
        .success();
}

#[test]
fn selfhost_only_rejects_non_routed_commands() {
    let dir = tempdir().unwrap();
    let out = dir.path().join("k.toml");

    cargo_bin_cmd!("genesis")
        .args([
            "--selfhost-only",
            "keygen",
            "--out",
            out.to_str().unwrap(),
        ])
        .assert()
        .failure()
        .code(50)
        .stderr(predicate::str::contains(
            "selfhost-only mode currently supports only `fmt`, `eval`, `explain`, `run`, `replay`, `optimize`, `typecheck`, `test`, `apply-patch`, `pack`, `selfhost-dashboard`, and `vcs hash`",
        ));
}

#[test]
fn selfhost_only_rejects_rust_engine_for_optimize() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("m.gc");
    std::fs::write(&file, "(def x 1)\nx\n").unwrap();

    cargo_bin_cmd!("genesis")
        .args([
            "--selfhost-only",
            "optimize",
            file.to_str().unwrap(),
            "--engine",
            "rust",
        ])
        .assert()
        .failure()
        .code(50)
        .stderr(predicate::str::contains(
            "selfhost-only mode requires --engine selfhost",
        ));
}

#[test]
fn selfhost_only_accepts_typecheck_with_selfhost_artifact() {
    let dir = tempdir().unwrap();
    let artifact = build_selfhost_artifact(dir.path());
    let pkg = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../tests/spec/pkg_basic/package.toml"
    );

    cargo_bin_cmd!("genesis")
        .args([
            "--selfhost-only",
            "--no-step-limit",
            "--selfhost-artifact",
            artifact.to_str().unwrap(),
            "typecheck",
            "--pkg",
            pkg,
        ])
        .assert()
        .success();
}

#[test]
fn selfhost_only_accepts_test_with_selfhost_artifact() {
    let dir = tempdir().unwrap();
    let artifact = build_selfhost_artifact(dir.path());
    let pkg = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../tests/spec/pkg_basic/package.toml"
    );

    cargo_bin_cmd!("genesis")
        .args([
            "--selfhost-only",
            "--selfhost-artifact",
            artifact.to_str().unwrap(),
            "test",
            "--pkg",
            pkg,
        ])
        .assert()
        .success();
}

#[test]
fn selfhost_only_accepts_apply_patch_with_selfhost_artifact() {
    let dir = tempdir().unwrap();
    let artifact = build_selfhost_artifact(dir.path());
    let fixture = std::path::Path::new(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../tests/spec/pkg_basic"
    ));
    for name in ["basic.gc", "caps.toml", "package.toml", "pure.gcpatch"] {
        std::fs::copy(fixture.join(name), dir.path().join(name)).unwrap();
    }
    let patch = dir.path().join("pure.gcpatch");
    let pkg = dir.path().join("package.toml");

    cargo_bin_cmd!("genesis")
        .args([
            "--selfhost-only",
            "--selfhost-artifact",
            artifact.to_str().unwrap(),
            "apply-patch",
            patch.to_str().unwrap(),
            "--pkg",
            pkg.to_str().unwrap(),
        ])
        .assert()
        .success();
}

#[test]
fn selfhost_only_accepts_pack_with_selfhost_artifact() {
    let dir = tempdir().unwrap();
    let artifact = build_selfhost_artifact(dir.path());
    let fixture = std::path::Path::new(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../tests/spec/pkg_basic"
    ));
    for name in ["basic.gc", "caps.toml", "package.toml"] {
        std::fs::copy(fixture.join(name), dir.path().join(name)).unwrap();
    }
    let pkg = dir.path().join("package.toml");

    cargo_bin_cmd!("genesis")
        .args([
            "--selfhost-only",
            "--selfhost-artifact",
            artifact.to_str().unwrap(),
            "pack",
            "--pkg",
            pkg.to_str().unwrap(),
        ])
        .assert()
        .success();
}

#[test]
fn selfhost_only_accepts_vcs_hash_with_selfhost_artifact() {
    let dir = tempdir().unwrap();
    let artifact = build_selfhost_artifact(dir.path());
    let file = dir.path().join("m.gc");
    std::fs::write(&file, "(def x 1)\nx\n").unwrap();

    let out = cargo_bin_cmd!("genesis")
        .args([
            "--json",
            "--selfhost-only",
            "--selfhost-artifact",
            artifact.to_str().unwrap(),
            "vcs",
            "hash",
            "--in",
            file.to_str().unwrap(),
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let v: JsonValue = serde_json::from_slice(&out).unwrap();
    let kind = v
        .get("data")
        .and_then(|d| d.get("hash_kind"))
        .and_then(JsonValue::as_str)
        .unwrap();
    assert_eq!(kind, "module");
}

#[test]
fn selfhost_only_accepts_explain_with_selfhost_artifact() {
    let dir = tempdir().unwrap();
    let artifact = build_selfhost_artifact(dir.path());
    let file = dir.path().join("m.gc");
    std::fs::write(
        &file,
        r#"
          (def c (core/contract::make (fn (msg) nil) nil {}))
          c
        "#,
    )
    .unwrap();

    cargo_bin_cmd!("genesis")
        .args([
            "--selfhost-only",
            "--selfhost-artifact",
            artifact.to_str().unwrap(),
            "--no-step-limit",
            "explain",
            file.to_str().unwrap(),
            "--engine",
            "selfhost",
            "--contract",
            "c",
            "--msg",
            "(msg foo nil)",
        ])
        .assert()
        .success();
}

#[test]
fn selfhost_only_accepts_run_and_replay_with_selfhost_artifact() {
    let dir = tempdir().unwrap();
    let artifact = build_selfhost_artifact(dir.path());
    let file = dir.path().join("prog.gc");
    std::fs::write(
        &file,
        r#"
          (def prog (core/effect::pure 42))
          prog
        "#,
    )
    .unwrap();
    let caps = dir.path().join("caps.toml");
    std::fs::write(&caps, "allow = []\n").unwrap();
    let log = dir.path().join("out.gclog");

    cargo_bin_cmd!("genesis")
        .args([
            "--selfhost-only",
            "--selfhost-artifact",
            artifact.to_str().unwrap(),
            "--no-step-limit",
            "run",
            file.to_str().unwrap(),
            "--engine",
            "selfhost",
            "--caps",
            caps.to_str().unwrap(),
            "--log",
            log.to_str().unwrap(),
        ])
        .assert()
        .success();

    cargo_bin_cmd!("genesis")
        .args([
            "--selfhost-only",
            "--selfhost-artifact",
            artifact.to_str().unwrap(),
            "--no-step-limit",
            "replay",
            file.to_str().unwrap(),
            "--engine",
            "selfhost",
            "--log",
            log.to_str().unwrap(),
        ])
        .assert()
        .success();
}

#[test]
fn pack_prefers_selfhost_when_artifact_flag_is_set_even_without_selfhost_only() {
    let dir = tempdir().unwrap();
    let fixture = std::path::Path::new(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../tests/spec/pkg_basic"
    ));
    for name in ["basic.gc", "caps.toml", "package.toml"] {
        std::fs::copy(fixture.join(name), dir.path().join(name)).unwrap();
    }
    let pkg = dir.path().join("package.toml");
    let bad_artifact = dir.path().join("bad_toolchain.gc");
    std::fs::write(&bad_artifact, "{ :kind \"bad\" }\n").unwrap();

    cargo_bin_cmd!("genesis")
        .args([
            "--selfhost-artifact",
            bad_artifact.to_str().unwrap(),
            "pack",
            "--pkg",
            pkg.to_str().unwrap(),
        ])
        .assert()
        .failure()
        .code(10)
        .stderr(predicate::str::contains(
            "selfhost artifact bootstrap required",
        ));
}

#[test]
fn pack_prefers_selfhost_when_default_artifact_path_exists() {
    let dir = tempdir().unwrap();
    let fixture = std::path::Path::new(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../tests/spec/pkg_basic"
    ));
    for name in ["basic.gc", "caps.toml", "package.toml"] {
        std::fs::copy(fixture.join(name), dir.path().join(name)).unwrap();
    }
    let pkg = dir.path().join("package.toml");
    let toolchain_dir = dir.path().join(".genesis").join("selfhost");
    std::fs::create_dir_all(&toolchain_dir).unwrap();
    std::fs::write(toolchain_dir.join("toolchain.gc"), "{ :kind \"bad\" }\n").unwrap();

    cargo_bin_cmd!("genesis")
        .args(["pack", "--pkg", pkg.to_str().unwrap()])
        .current_dir(dir.path())
        .assert()
        .failure()
        .code(10)
        .stderr(predicate::str::contains(
            "selfhost artifact bootstrap required",
        ));
}

#[test]
fn fmt_prefers_selfhost_when_artifact_flag_is_set_without_engine() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("m.gc");
    let bad_artifact = dir.path().join("bad_toolchain.gc");
    std::fs::write(&file, "(def x 1)\n").unwrap();
    std::fs::write(&bad_artifact, "{ :kind \"bad\" }\n").unwrap();

    cargo_bin_cmd!("genesis")
        .args([
            "--selfhost-artifact",
            bad_artifact.to_str().unwrap(),
            "fmt",
            file.to_str().unwrap(),
        ])
        .assert()
        .failure()
        .code(1)
        .stderr(predicate::str::contains(
            "selfhost artifact bootstrap required",
        ));
}

#[test]
fn eval_prefers_selfhost_when_artifact_flag_is_set_without_engine() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("m.gc");
    let bad_artifact = dir.path().join("bad_toolchain.gc");
    std::fs::write(&file, "1\n").unwrap();
    std::fs::write(&bad_artifact, "{ :kind \"bad\" }\n").unwrap();

    cargo_bin_cmd!("genesis")
        .args([
            "--selfhost-artifact",
            bad_artifact.to_str().unwrap(),
            "eval",
            file.to_str().unwrap(),
        ])
        .assert()
        .failure()
        .code(1)
        .stderr(predicate::str::contains(
            "selfhost artifact bootstrap required",
        ));
}

#[test]
fn optimize_prefers_selfhost_when_artifact_flag_is_set_without_engine() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("m.gc");
    let bad_artifact = dir.path().join("bad_toolchain.gc");
    std::fs::write(&file, "(def x 1)\nx\n").unwrap();
    std::fs::write(&bad_artifact, "{ :kind \"bad\" }\n").unwrap();

    cargo_bin_cmd!("genesis")
        .args([
            "--selfhost-artifact",
            bad_artifact.to_str().unwrap(),
            "optimize",
            file.to_str().unwrap(),
        ])
        .assert()
        .failure()
        .code(1)
        .stderr(predicate::str::contains(
            "selfhost artifact bootstrap required",
        ));
}

#[test]
fn rust_engine_requires_compat_flag_and_can_override_when_enabled() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("m.gc");
    let bad_artifact = dir.path().join("bad_toolchain.gc");
    std::fs::write(&file, "(def x 1)\n").unwrap();
    std::fs::write(&bad_artifact, "{ :kind \"bad\" }\n").unwrap();

    cargo_bin_cmd!("genesis")
        .args([
            "--selfhost-artifact",
            bad_artifact.to_str().unwrap(),
            "fmt",
            file.to_str().unwrap(),
            "--engine",
            "rust",
        ])
        .assert()
        .failure()
        .code(50)
        .stderr(predicate::str::contains(
            "--engine rust` is disabled in the default selfhost profile",
        ));

    cargo_bin_cmd!("genesis")
        .env("GENESIS_ALLOW_RUST_ENGINE", "1")
        .args([
            "--selfhost-artifact",
            bad_artifact.to_str().unwrap(),
            "fmt",
            file.to_str().unwrap(),
            "--engine",
            "rust",
        ])
        .assert()
        .success();
}

#[test]
fn fmt_defaults_to_selfhost_via_workspace_artifact_fallback() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("m.gc");
    std::fs::write(&file, "(def x 1)\n").unwrap();

    let out = cargo_bin_cmd!("genesis")
        .args(["--json", "fmt", file.to_str().unwrap()])
        .current_dir(dir.path())
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let v: JsonValue = serde_json::from_slice(&out).unwrap();
    let engine = v
        .get("data")
        .and_then(|d| d.get("engine"))
        .and_then(JsonValue::as_str)
        .unwrap();
    assert_eq!(engine, "selfhost");
}
