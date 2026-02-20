use super::*;

#[test]
fn selfhost_only_accepts_run_and_replay_with_selfhost_artifact() {
    let td = tempdir().unwrap();
    let artifact = build_selfhost_artifact(td.path());
    let file = td.path().join("prog.gc");
    std::fs::write(
        &file,
        r#"
          (def prog (core/effect::pure 42))
          prog
        "#,
    )
    .unwrap();
    let caps = td.path().join("caps.toml");
    std::fs::write(&caps, "allow = []\n").unwrap();
    let log = td.path().join("out.gclog");

    cargo_bin_cmd!("genesis_wasi")
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

    cargo_bin_cmd!("genesis_wasi")
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
fn selfhost_only_rejects_legacy_pkg_semantic_fallback_in_run_logs() {
    let td = tempdir().unwrap();
    let artifact = build_selfhost_artifact(td.path());
    let file = td.path().join("legacy.gc");
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
    let caps = td.path().join("caps_legacy_pkg.toml");
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

    cargo_bin_cmd!("genesis_wasi")
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
fn selfhost_only_rejects_legacy_gc_semantic_fallback_in_run_logs() {
    let td = tempdir().unwrap();
    let artifact = build_selfhost_artifact(td.path());
    let file = td.path().join("legacy_gc.gc");
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
    let caps = td.path().join("caps_legacy_gc.toml");
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

    cargo_bin_cmd!("genesis_wasi")
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
    let td = tempdir().unwrap();
    let artifact = build_selfhost_artifact(td.path());
    std::fs::write(td.path().join("bad.gpk"), b"not-a-gpk-bundle").unwrap();
    let file = td.path().join("legacy_gpk.gc");
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
    let caps = td.path().join("caps_legacy_gpk.toml");
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

    cargo_bin_cmd!("genesis_wasi")
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
fn fmt_default_profile_allows_without_explicit_artifact() {
    let td = tempdir().unwrap();
    let file = td.path().join("m.gc");
    std::fs::write(&file, "(def x 1)\n").unwrap();

    cargo_bin_cmd!("genesis_wasi")
        .args(["fmt", file.to_str().unwrap()])
        .current_dir(td.path())
        .assert()
        .success();
}

#[test]
fn wasi_legacy_high_level_caps_ops_are_rejected_in_default_profile() {
    let td = tempdir().unwrap();
    let artifact = build_selfhost_artifact(td.path());
    let file = td.path().join("prog.gc");
    std::fs::write(&file, "(def prog (core/effect::pure 1))\nprog\n").unwrap();
    let caps = td.path().join("caps_legacy.toml");
    std::fs::write(
        &caps,
        r#"
allow = ["core/pkg::init"]
"#,
    )
    .unwrap();

    cargo_bin_cmd!("genesis_wasi")
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
