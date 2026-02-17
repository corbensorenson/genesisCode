use assert_cmd::cargo::cargo_bin_cmd;
use predicates::prelude::*;
use tempfile::tempdir;

fn cmd() -> assert_cmd::Command {
    let mut c = cargo_bin_cmd!("genesis_wasi");
    c.env("GENESIS_ALLOW_RUST_ENGINE", "1");
    c
}

fn build_selfhost_artifact(dir: &std::path::Path) -> std::path::PathBuf {
    let artifact = dir.join("selfhost_toolchain.gc");
    cmd()
        .args(["selfhost-artifact", "--out"])
        .arg(&artifact)
        .assert()
        .success();
    artifact
}

#[test]
fn run_selfhost_engine_matches_rust_engine_output_for_pure_effect_program() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("prog.gc");
    let caps = dir.path().join("caps.toml");
    let artifact = build_selfhost_artifact(dir.path());

    std::fs::write(
        &file,
        r#"
          (def prog (core/effect::pure 42))
          prog
        "#,
    )
    .unwrap();
    std::fs::write(&caps, "allow = []\n").unwrap();

    let rust_log = dir.path().join("rust.gclog");
    let rust_out = cmd()
        .args([
            "run",
            file.to_str().unwrap(),
            "--engine",
            "rust",
            "--caps",
            caps.to_str().unwrap(),
            "--log",
            rust_log.to_str().unwrap(),
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let selfhost_log = dir.path().join("selfhost.gclog");
    let selfhost_out = cmd()
        .args([
            "--no-step-limit",
            "--selfhost-artifact",
            artifact.to_str().unwrap(),
            "run",
            file.to_str().unwrap(),
            "--engine",
            "selfhost",
            "--caps",
            caps.to_str().unwrap(),
            "--log",
            selfhost_log.to_str().unwrap(),
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let rust_s = String::from_utf8(rust_out).unwrap();
    let selfhost_s = String::from_utf8(selfhost_out).unwrap();
    assert_eq!(rust_s.trim(), selfhost_s.trim());
    let rust_log_s = std::fs::read_to_string(&rust_log).unwrap();
    let selfhost_log_s = std::fs::read_to_string(&selfhost_log).unwrap();
    assert_eq!(rust_log_s, selfhost_log_s);
}

#[test]
fn run_selfhost_engine_matches_rust_engine_output_for_denied_effect_program() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("prog.gc");
    let caps = dir.path().join("caps.toml");
    let artifact = build_selfhost_artifact(dir.path());

    std::fs::write(
        &file,
        r#"
          (def prog
            (core/effect::perform
              'sys/time::now
              nil
              (fn (t) (core/effect::pure t))))
          prog
        "#,
    )
    .unwrap();
    std::fs::write(&caps, "allow = []\n").unwrap();

    let rust_log = dir.path().join("rust.gclog");
    let rust_out = cmd()
        .args([
            "run",
            file.to_str().unwrap(),
            "--engine",
            "rust",
            "--caps",
            caps.to_str().unwrap(),
            "--log",
            rust_log.to_str().unwrap(),
        ])
        .assert()
        .failure()
        .code(41)
        .get_output()
        .stdout
        .clone();

    let selfhost_log = dir.path().join("selfhost.gclog");
    let selfhost_out = cmd()
        .args([
            "--no-step-limit",
            "--selfhost-artifact",
            artifact.to_str().unwrap(),
            "run",
            file.to_str().unwrap(),
            "--engine",
            "selfhost",
            "--caps",
            caps.to_str().unwrap(),
            "--log",
            selfhost_log.to_str().unwrap(),
        ])
        .assert()
        .failure()
        .code(41)
        .get_output()
        .stdout
        .clone();

    let rust_s = String::from_utf8(rust_out).unwrap();
    let selfhost_s = String::from_utf8(selfhost_out).unwrap();
    assert_eq!(rust_s.trim(), selfhost_s.trim());
    let rust_log_s = std::fs::read_to_string(&rust_log).unwrap();
    let selfhost_log_s = std::fs::read_to_string(&selfhost_log).unwrap();
    assert_eq!(rust_log_s, selfhost_log_s);

    let rust_replay_out = cmd()
        .args([
            "replay",
            file.to_str().unwrap(),
            "--engine",
            "rust",
            "--log",
            rust_log.to_str().unwrap(),
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let selfhost_replay_out = cmd()
        .args([
            "--no-step-limit",
            "--selfhost-artifact",
            artifact.to_str().unwrap(),
            "replay",
            file.to_str().unwrap(),
            "--engine",
            "selfhost",
            "--log",
            selfhost_log.to_str().unwrap(),
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    assert_eq!(
        String::from_utf8(rust_replay_out).unwrap().trim(),
        String::from_utf8(selfhost_replay_out).unwrap().trim()
    );
}

#[test]
fn replay_selfhost_engine_matches_rust_engine_output() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("prog.gc");
    let caps = dir.path().join("caps.toml");
    let artifact = build_selfhost_artifact(dir.path());

    std::fs::write(
        &file,
        r#"
          (def prog (core/effect::pure 7))
          prog
        "#,
    )
    .unwrap();
    std::fs::write(&caps, "allow = []\n").unwrap();

    let log = dir.path().join("out.gclog");
    cmd()
        .args([
            "run",
            file.to_str().unwrap(),
            "--engine",
            "rust",
            "--caps",
            caps.to_str().unwrap(),
            "--log",
            log.to_str().unwrap(),
        ])
        .assert()
        .success();

    let rust_out = cmd()
        .args([
            "replay",
            file.to_str().unwrap(),
            "--engine",
            "rust",
            "--log",
            log.to_str().unwrap(),
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let selfhost_out = cmd()
        .args([
            "--no-step-limit",
            "--selfhost-artifact",
            artifact.to_str().unwrap(),
            "replay",
            file.to_str().unwrap(),
            "--engine",
            "selfhost",
            "--log",
            log.to_str().unwrap(),
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let rust_s = String::from_utf8(rust_out).unwrap();
    let selfhost_s = String::from_utf8(selfhost_out).unwrap();
    assert_eq!(rust_s.trim(), selfhost_s.trim());
}

#[test]
fn run_selfhost_engine_surfaces_parse_errors() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("bad.gc");
    let caps = dir.path().join("caps.toml");
    let artifact = build_selfhost_artifact(dir.path());
    std::fs::write(&file, "(def prog (core/effect::pure 1)").unwrap();
    std::fs::write(&caps, "allow = []\n").unwrap();

    cmd()
        .args([
            "--no-step-limit",
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
        .code(10)
        .stderr(predicate::str::contains("core/parse/"));
}

#[test]
fn run_prefers_selfhost_when_artifact_flag_is_set_without_engine() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("prog.gc");
    let caps = dir.path().join("caps.toml");
    let bad_artifact = dir.path().join("bad_toolchain.gc");
    std::fs::write(
        &file,
        r#"
          (def prog (core/effect::pure 1))
          prog
        "#,
    )
    .unwrap();
    std::fs::write(&caps, "allow = []\n").unwrap();
    std::fs::write(&bad_artifact, "{ :kind \"bad\" }\n").unwrap();

    cmd()
        .args([
            "--selfhost-artifact",
            bad_artifact.to_str().unwrap(),
            "run",
            file.to_str().unwrap(),
            "--caps",
            caps.to_str().unwrap(),
        ])
        .assert()
        .failure()
        .code(1)
        .stderr(predicate::str::contains(
            "selfhost artifact bootstrap required",
        ));
}

#[test]
fn replay_prefers_selfhost_when_artifact_flag_is_set_without_engine() {
    let dir = tempdir().unwrap();
    let file = dir.path().join("prog.gc");
    let caps = dir.path().join("caps.toml");
    let bad_artifact = dir.path().join("bad_toolchain.gc");
    std::fs::write(
        &file,
        r#"
          (def prog (core/effect::pure 7))
          prog
        "#,
    )
    .unwrap();
    std::fs::write(&caps, "allow = []\n").unwrap();
    std::fs::write(&bad_artifact, "{ :kind \"bad\" }\n").unwrap();
    let log = dir.path().join("out.gclog");
    cmd()
        .args([
            "run",
            file.to_str().unwrap(),
            "--engine",
            "rust",
            "--caps",
            caps.to_str().unwrap(),
            "--log",
            log.to_str().unwrap(),
        ])
        .assert()
        .success();

    cmd()
        .args([
            "--selfhost-artifact",
            bad_artifact.to_str().unwrap(),
            "replay",
            file.to_str().unwrap(),
            "--log",
            log.to_str().unwrap(),
        ])
        .assert()
        .failure()
        .code(1)
        .stderr(predicate::str::contains(
            "selfhost artifact bootstrap required",
        ));
}
