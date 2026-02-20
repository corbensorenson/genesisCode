use assert_cmd::cargo::cargo_bin_cmd;
use predicates::prelude::*;
use tempfile::tempdir;

mod support;

fn build_selfhost_artifact(dir: &std::path::Path) -> std::path::PathBuf {
    support::copy_repo_toolchain_artifact(dir)
}

#[test]
fn selfhost_only_rejects_legacy_pkg_semantic_fallback_in_run_logs() {
    let dir = tempdir().unwrap();
    let artifact = build_selfhost_artifact(dir.path());
    let file = dir.path().join("legacy.gc");
    std::fs::write(
        &file,
        r#"
          (def prog
            (((core/effect::perform (quote core/pkg::init))
               {:workspace "legacy-ws"
                :lock "legacy.lock"
                :policy "policy:default-v0.1"
                :registry-default nil})
             (fn (r) (core/effect::pure r))))
          prog
        "#,
    )
    .unwrap();
    let caps = dir.path().join("caps_legacy_pkg.toml");
    std::fs::write(
        &caps,
        r#"
allow = ["core/pkg-low::init"]

[op."core/pkg-low::init"]
base_dir = "."
create_dirs = true
"#,
    )
    .unwrap();

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
        ])
        .assert()
        .failure()
        .code(50)
        .stderr(predicate::str::contains(
            "selfhost-only mode detected legacy semantic fallback",
        ))
        .stderr(predicate::str::contains("core/pkg::init"));
}

#[test]
fn selfhost_only_rejects_legacy_vcs_semantic_fallback_in_run_logs() {
    let dir = tempdir().unwrap();
    let artifact = build_selfhost_artifact(dir.path());
    let file = dir.path().join("legacy_vcs.gc");
    std::fs::write(
        &file,
        r#"
          (def prog
            (((core/effect::perform (quote core/vcs::log))
               {:root nil :max 5})
             (fn (r) (core/effect::pure r))))
          prog
        "#,
    )
    .unwrap();
    let caps = dir.path().join("caps_legacy_vcs.toml");
    std::fs::write(
        &caps,
        r#"
allow = ["core/vcs-low::log"]

[store]
dir = "./.genesis/store"

[refs]
path = "./.genesis/refs.gc"
"#,
    )
    .unwrap();

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
        ])
        .assert()
        .failure()
        .code(50)
        .stderr(predicate::str::contains(
            "selfhost-only mode detected legacy semantic fallback",
        ))
        .stderr(predicate::str::contains("core/vcs::log"));
}

#[test]
fn selfhost_only_rejects_legacy_gc_semantic_fallback_in_run_logs() {
    let dir = tempdir().unwrap();
    let artifact = build_selfhost_artifact(dir.path());
    let file = dir.path().join("legacy_gc.gc");
    std::fs::write(
        &file,
        r#"
          (def prog
            (((core/effect::perform (quote core/gc::pin))
               {:pins "pins.toml"
                :target "refs/heads/main"})
             (fn (r) (core/effect::pure r))))
          prog
        "#,
    )
    .unwrap();
    let caps = dir.path().join("caps_legacy_gc.toml");
    std::fs::write(
        &caps,
        r#"
allow = ["core/gc-low::pin"]

[store]
dir = "./.genesis/store"

[refs]
path = "./.genesis/refs.gc"

[op."core/gc-low::pin"]
base_dir = "."
create_dirs = true
"#,
    )
    .unwrap();

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
        ])
        .assert()
        .failure()
        .code(50)
        .stderr(predicate::str::contains(
            "selfhost-only mode detected legacy semantic fallback",
        ))
        .stderr(predicate::str::contains("core/gc::pin"));
}

#[test]
fn selfhost_only_rejects_legacy_gpk_semantic_fallback_in_run_logs() {
    let dir = tempdir().unwrap();
    let artifact = build_selfhost_artifact(dir.path());
    std::fs::write(dir.path().join("bad.gpk"), b"not-a-gpk-bundle").unwrap();
    let file = dir.path().join("legacy_gpk.gc");
    std::fs::write(
        &file,
        r#"
          (def prog
            (((core/effect::perform (quote core/gpk::import))
               {:in "bad.gpk"})
             (fn (r) (core/effect::pure r))))
          prog
        "#,
    )
    .unwrap();
    let caps = dir.path().join("caps_legacy_gpk.toml");
    std::fs::write(
        &caps,
        r#"
allow = ["core/gpk-low::import"]

[store]
dir = "./.genesis/store"

[refs]
path = "./.genesis/refs.gc"

[op."core/gpk-low::import"]
base_dir = "."
"#,
    )
    .unwrap();

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
        ])
        .assert()
        .failure()
        .code(50)
        .stderr(predicate::str::contains(
            "selfhost-only mode detected legacy semantic fallback",
        ))
        .stderr(predicate::str::contains("core/gpk::import"));
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
fn pack_uses_workspace_pinned_artifact_even_when_local_default_artifact_is_bad() {
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
        .success();
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
        .code(2)
        .stderr(predicate::str::contains(
            "invalid value 'rust' for '--engine <ENGINE>'",
        ))
        .stderr(predicate::str::contains("expected `selfhost`"));

    cargo_bin_cmd!("genesis_parity")
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
fn default_profile_rejects_rust_coreform_frontend_without_compat_opt_in() {
    let dir = tempdir().unwrap();
    let fixture = std::path::Path::new(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../tests/spec/pkg_basic"
    ));
    for name in ["basic.gc", "caps.toml", "package.toml", "pure.gcpatch"] {
        std::fs::copy(fixture.join(name), dir.path().join(name)).unwrap();
    }
    let pkg = dir.path().join("package.toml");
    let patch = dir.path().join("pure.gcpatch");

    for args in [
        vec!["pack", "--pkg", pkg.to_str().unwrap()],
        vec!["test", "--pkg", pkg.to_str().unwrap()],
        vec!["typecheck", "--pkg", pkg.to_str().unwrap()],
        vec![
            "apply-patch",
            patch.to_str().unwrap(),
            "--pkg",
            pkg.to_str().unwrap(),
        ],
    ] {
        cargo_bin_cmd!("genesis")
            .args(["--coreform-frontend", "rust"])
            .args(&args)
            .assert()
            .failure()
            .code(2)
            .stderr(predicate::str::contains(
                "invalid value 'rust' for '--coreform-frontend <COREFORM_FRONTEND>'",
            ))
            .stderr(predicate::str::contains("expected `selfhost`"));
    }

    // Explicit compat opt-in is still available.
    cargo_bin_cmd!("genesis_parity")
        .args(["--coreform-frontend", "rust", "pack", "--pkg"])
        .arg(&pkg)
        .assert()
        .success();
}

#[test]
fn fmt_default_selfhost_uses_workspace_pinned_artifact() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("m.gc");
    std::fs::write(&file, "(def x 1)\n").unwrap();

    cargo_bin_cmd!("genesis")
        .args(["fmt", file.to_str().unwrap()])
        .current_dir(dir.path())
        .assert()
        .success();
}

#[test]
fn selfhost_only_full_production_workflow_runs_without_rust_fallbacks() {
    let dir = tempdir().unwrap();
    let artifact = build_selfhost_artifact(dir.path());
    let fixture = std::path::Path::new(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../tests/spec/pkg_basic"
    ));
    for name in ["basic.gc", "caps.toml", "package.toml", "pure.gcpatch"] {
        std::fs::copy(fixture.join(name), dir.path().join(name)).unwrap();
    }

    let module = dir.path().join("basic.gc");
    let pkg = dir.path().join("package.toml");
    let patch = dir.path().join("pure.gcpatch");
    let optimized = dir.path().join("basic.opt.gc");
    let run_prog = dir.path().join("prog.gc");
    let run_caps = dir.path().join("caps_run.toml");
    let run_log = dir.path().join("run.gclog");
    std::fs::write(
        &run_prog,
        r#"
          (def prog (core/effect::pure 42))
          prog
        "#,
    )
    .unwrap();
    std::fs::write(&run_caps, "allow = []\n").unwrap();

    let common = [
        "--selfhost-only",
        "--selfhost-artifact",
        artifact.to_str().unwrap(),
    ];

    cargo_bin_cmd!("genesis")
        .args(common)
        .args(["fmt", module.to_str().unwrap(), "--engine", "selfhost"])
        .assert()
        .success();

    cargo_bin_cmd!("genesis")
        .args(common)
        .args(["eval", module.to_str().unwrap(), "--engine", "selfhost"])
        .assert()
        .success();

    cargo_bin_cmd!("genesis")
        .args(common)
        .args([
            "run",
            run_prog.to_str().unwrap(),
            "--engine",
            "selfhost",
            "--caps",
            run_caps.to_str().unwrap(),
            "--log",
            run_log.to_str().unwrap(),
        ])
        .assert()
        .success();

    cargo_bin_cmd!("genesis")
        .args(common)
        .args([
            "replay",
            run_prog.to_str().unwrap(),
            "--engine",
            "selfhost",
            "--log",
            run_log.to_str().unwrap(),
        ])
        .assert()
        .success();

    cargo_bin_cmd!("genesis")
        .args(common)
        .args(["pack", "--pkg", pkg.to_str().unwrap()])
        .assert()
        .success();

    cargo_bin_cmd!("genesis")
        .args(common)
        .args(["test", "--pkg", pkg.to_str().unwrap()])
        .assert()
        .success();

    cargo_bin_cmd!("genesis")
        .args(common)
        .args(["typecheck", "--pkg", pkg.to_str().unwrap()])
        .assert()
        .success();

    cargo_bin_cmd!("genesis")
        .args(common)
        .args([
            "optimize",
            module.to_str().unwrap(),
            "--engine",
            "selfhost",
            "--out",
            optimized.to_str().unwrap(),
        ])
        .assert()
        .success();
    assert!(optimized.exists());

    cargo_bin_cmd!("genesis")
        .args(common)
        .args([
            "apply-patch",
            patch.to_str().unwrap(),
            "--pkg",
            pkg.to_str().unwrap(),
        ])
        .assert()
        .success();

    cargo_bin_cmd!("genesis")
        .args(common)
        .args(["pack", "--pkg", pkg.to_str().unwrap()])
        .assert()
        .success();
}

#[test]
fn legacy_high_level_caps_ops_are_rejected_in_default_profile() {
    let dir = tempdir().unwrap();
    let artifact = build_selfhost_artifact(dir.path());
    let file = dir.path().join("prog.gc");
    std::fs::write(&file, "(def prog (core/effect::pure 1))\nprog\n").unwrap();
    let caps = dir.path().join("caps_legacy.toml");
    std::fs::write(
        &caps,
        r#"
allow = ["core/pkg::init"]
"#,
    )
    .unwrap();

    cargo_bin_cmd!("genesis")
        .args([
            "--json",
            "--selfhost-artifact",
            artifact.to_str().unwrap(),
            "run",
            file.to_str().unwrap(),
            "--engine",
            "selfhost",
            "--caps",
            caps.to_str().unwrap(),
        ])
        .assert()
        .failure()
        .code(10);
}
