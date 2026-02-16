use std::path::PathBuf;

use assert_cmd::cargo::cargo_bin_cmd;
use predicates::prelude::*;

fn demo_file(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../examples/gfx_demos")
        .join(name)
}

#[test]
fn eval_ui_app_demo_succeeds() {
    cargo_bin_cmd!("genesis")
        .args(["eval"])
        .arg(demo_file("ui_app.gc"))
        .assert()
        .success()
        .stdout(predicate::str::contains("gfx/ui-app"));
}

#[test]
fn eval_scene3d_demo_succeeds() {
    cargo_bin_cmd!("genesis")
        .args(["eval"])
        .arg(demo_file("scene3d.gc"))
        .assert()
        .success()
        .stdout(predicate::str::contains("gfx/scene3d"));
}

#[test]
fn eval_hybrid_web_demo_succeeds() {
    cargo_bin_cmd!("genesis")
        .args(["eval"])
        .arg(demo_file("hybrid_web.gc"))
        .assert()
        .success()
        .stdout(predicate::str::contains("gfx/hybrid-web"));
}
