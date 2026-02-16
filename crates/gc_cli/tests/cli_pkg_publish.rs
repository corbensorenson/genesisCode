use std::fs;
use std::path::{Path, PathBuf};

use assert_cmd::cargo::cargo_bin_cmd;

fn write_caps(dir: &Path, remote_allow: &str) -> PathBuf {
    let caps = dir.join("caps.toml");
    fs::write(
        &caps,
        format!(
            r#"
allow = [
  "core/store::put",
  "core/sync::push"
]

[store]
dir = "./.genesis/store"

[refs]
path = "./.genesis/refs.gc"

[op."core/sync::push"]
remote_allow = ["{remote_allow}"]
"#
        ),
    )
    .unwrap();
    caps
}

fn cli_store_put(dir: &Path, caps: &Path, term_src: &str, filename: &str) -> String {
    let p = dir.join(filename);
    fs::write(&p, term_src).unwrap();
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

fn set_local_ref(dir: &Path, commit_hex: &str) {
    let refs_path = dir.join(".genesis").join("refs.gc");
    let rdb = gc_effects::RefsDb::open(&refs_path).unwrap();
    let _ = rdb.set("refs/heads/main", Some(commit_hex), None).unwrap();
}

fn get_remote_ref(remote_dir: &Path, name: &str) -> Option<String> {
    let refs_path = remote_dir.join("v1").join("refs.gc");
    if !refs_path.exists() {
        return None;
    }
    let rdb = gc_effects::RefsDb::open(&refs_path).unwrap();
    rdb.get(name).unwrap()
}

#[test]
fn pkg_publish_is_policy_gated_and_advances_remote_ref_on_success() {
    let td = tempfile::tempdir().unwrap();
    let dir = td.path();

    // Remote registry is a file-backed registry at <remote_dir>/v1/{store,refs.gc}.
    let remote_dir = dir.join("remote-registry");
    fs::create_dir_all(&remote_dir).unwrap();
    let remote = format!("file://{}/", remote_dir.display());
    let remote_allow = format!("{remote}v1/");

    let caps = write_caps(dir, &remote_allow);

    // Policy: main requires unit-tests.
    let policy_hex = cli_store_put(
        dir,
        &caps,
        r#"
{
  :type :vcs/policy
  :v 1
  :name "policy:test"
  :refs { :frozen-prefixes [] }
  :classes {
    :dev  { :patterns ["refs/**/heads/*"] :exclude ["refs/**/heads/main"] :required-obligations [] }
    :main { :patterns ["refs/**/heads/main"] :required-obligations [core/obligation::unit-tests] :require-signatures false }
    :tags { :patterns ["refs/**/tags/*"] :required-obligations [core/obligation::unit-tests] :require-signatures false }
  }
}
"#,
        "policy.gc",
    );

    let patch_hex = cli_store_put(dir, &caps, r#"{:type :vcs/patch :v 1 :ops []}"#, "patch.gc");
    let snap_hex = cli_store_put(
        dir,
        &caps,
        r#"{:type :vcs/snapshot :v 1 :kind :package :pkg/name "x" :pkg/version "0" :modules [] :obligations []}"#,
        "snap.gc",
    );

    // Commit missing evidence -> publish must refuse locally.
    let commit_bad = cli_store_put(
        dir,
        &caps,
        &format!(
            r#"{{
  :type :vcs/commit
  :v 1
  :parents []
  :target {{ :kind :package :name "x" }}
  :base nil
  :patch "{patch_hex}"
  :result "{snap_hex}"
  :obligations [core/obligation::unit-tests]
  :evidence []
  :attestations []
  :message "bad"
}}"#
        ),
        "commit_bad.gc",
    );
    set_local_ref(dir, &commit_bad);

    cargo_bin_cmd!("genesis")
        .current_dir(dir)
        .args(["pkg", "--caps"])
        .arg(&caps)
        .args([
            "publish",
            "--remote",
            &remote,
            "--ref",
            "refs/heads/main",
            "--policy",
            &policy_hex,
        ])
        .assert()
        .code(30);
    assert_eq!(get_remote_ref(&remote_dir, "refs/heads/main"), None);

    // Commit with evidence -> publish succeeds and advances remote.
    let evidence_hex = cli_store_put(
        dir,
        &caps,
        r#"{:type :vcs/evidence :v 1 :kind :unit-tests :inputs [] :outputs [] :data nil}"#,
        "evidence.gc",
    );
    let commit_ok = cli_store_put(
        dir,
        &caps,
        &format!(
            r#"{{
  :type :vcs/commit
  :v 1
  :parents []
  :target {{ :kind :package :name "x" }}
  :base nil
  :patch "{patch_hex}"
  :result "{snap_hex}"
  :obligations [core/obligation::unit-tests]
  :evidence ["{evidence_hex}"]
  :attestations []
  :message "ok"
}}"#
        ),
        "commit_ok.gc",
    );
    set_local_ref(dir, &commit_ok);

    cargo_bin_cmd!("genesis")
        .current_dir(dir)
        .args(["pkg", "--caps"])
        .arg(&caps)
        .args([
            "publish",
            "--remote",
            &remote,
            "--ref",
            "refs/heads/main",
            "--policy",
            &policy_hex,
        ])
        .assert()
        .success()
        .stdout(predicates::str::is_match("^[0-9a-f]{64}\n$").unwrap());

    assert_eq!(
        get_remote_ref(&remote_dir, "refs/heads/main"),
        Some(commit_ok)
    );
}

