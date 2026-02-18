use std::fs;
use std::path::{Path, PathBuf};

use assert_cmd::cargo::cargo_bin_cmd;
use predicates::prelude::*;

fn write_caps(dir: &Path) -> PathBuf {
    let caps = dir.join("caps.toml");
    fs::write(
        &caps,
        r#"
allow = [
  "core/store::put",
  "core/store::get",
  "core/vcs-low::diff-terms",
  "core/vcs-low::apply-patch",
  "io/fs::read",
  "io/fs::write",
  "core/vcs-low::diff",
  "core/vcs-low::apply"
]

[store]
dir = "./.genesis/store"

[refs]
path = "./.genesis/refs.gc"

[op."core/vcs-low::diff"]
base_dir = "."
create_dirs = true

[op."core/vcs-low::apply"]
base_dir = "."
create_dirs = true

[op."io/fs::read"]
base_dir = "."

[op."io/fs::write"]
base_dir = "."
create_dirs = true
"#,
    )
    .unwrap();
    caps
}

fn store_put(dir: &Path, caps: &Path, term_src: &str, filename: &str) -> String {
    fs::write(dir.join(filename), term_src).unwrap();
    let out = cargo_bin_cmd!("genesis")
        .current_dir(dir)
        .args(["store", "--caps"])
        .arg(caps)
        .args(["put", "--input"])
        .arg(filename)
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    String::from_utf8(out).unwrap().trim().to_string()
}

#[test]
fn vcs_diff_then_apply_roundtrips_snapshot_bytes() {
    let td = tempfile::tempdir().unwrap();
    let dir = td.path();
    let caps = write_caps(dir);

    let h1 = "1".repeat(64);
    let h2 = "2".repeat(64);
    let h3 = "3".repeat(64);

    let base_snap = format!(
        r#"
{{
  :type :vcs/snapshot
  :v 1
  :kind :contract
  :proto nil
  :overrides {{
    my/op::a "{h1}"
  }}
}}
"#
    );
    let to_snap = format!(
        r#"
{{
  :type :vcs/snapshot
  :v 1
  :kind :contract
  :proto nil
  :overrides {{
    my/op::a "{h2}"
    my/op::b "{h3}"
  }}
}}
"#
    );

    let base_h = store_put(dir, &caps, &base_snap, "base.gc");
    let to_h = store_put(dir, &caps, &to_snap, "to.gc");
    assert!(
        predicate::str::is_match("^[0-9a-f]{64}$")
            .unwrap()
            .eval(&base_h)
    );
    assert!(
        predicate::str::is_match("^[0-9a-f]{64}$")
            .unwrap()
            .eval(&to_h)
    );

    let patch_path = dir.join("delta.patch");
    let patch_h = String::from_utf8(
        cargo_bin_cmd!("genesis")
            .current_dir(dir)
            .args(["vcs", "--caps"])
            .arg(&caps)
            .args([
                "diff",
                "--base",
                &base_h,
                "--to",
                &to_h,
                "--out",
                patch_path.to_str().unwrap(),
            ])
            .assert()
            .success()
            .get_output()
            .stdout
            .clone(),
    )
    .unwrap()
    .trim()
    .to_string();
    assert!(
        predicate::str::is_match("^[0-9a-f]{64}$")
            .unwrap()
            .eval(&patch_h)
    );
    assert!(patch_path.exists());

    // patch file is canonical; storing it should yield the same patch hash.
    cargo_bin_cmd!("genesis")
        .current_dir(dir)
        .args(["store", "--caps"])
        .arg(&caps)
        .args(["put", "--input"])
        .arg(&patch_path)
        .assert()
        .success()
        .stdout(format!("{patch_h}\n"));

    let out_snap_path = dir.join("out-snap.gc");
    let got_h = String::from_utf8(
        cargo_bin_cmd!("genesis")
            .current_dir(dir)
            .args(["vcs", "--caps"])
            .arg(&caps)
            .args([
                "apply",
                "--base",
                &base_h,
                "--patch",
                &patch_h,
                "--out",
                out_snap_path.to_str().unwrap(),
            ])
            .assert()
            .success()
            .get_output()
            .stdout
            .clone(),
    )
    .unwrap()
    .trim()
    .to_string();
    assert_eq!(got_h, to_h);
    assert!(out_snap_path.exists());

    // Apply can also take a patch file path.
    let got2_h = String::from_utf8(
        cargo_bin_cmd!("genesis")
            .current_dir(dir)
            .args(["vcs", "--caps"])
            .arg(&caps)
            .args([
                "apply",
                "--base",
                &base_h,
                "--patch",
                patch_path.to_str().unwrap(),
            ])
            .assert()
            .success()
            .get_output()
            .stdout
            .clone(),
    )
    .unwrap()
    .trim()
    .to_string();
    assert_eq!(got2_h, to_h);
}
