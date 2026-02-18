use std::fs;
use std::path::{Path, PathBuf};

use assert_cmd::cargo::cargo_bin_cmd;
use gc_coreform::{Term, TermOrdKey, parse_term};
use gc_effects::{EffectLog, RefsDb};
use serde_json::Value as JsonValue;
use tempfile::tempdir;

mod support;

fn build_selfhost_artifact(dir: &Path) -> PathBuf {
    support::copy_repo_toolchain_artifact(dir)
}

fn write_caps(dir: &Path, remote_allow: &str) -> PathBuf {
    let caps = dir.join("caps.toml");
    fs::write(
        &caps,
        format!(
            r#"
allow = [
  "core/pkg-low::save-lock",
  "core/pkg-low::load-lock",
  "core/pkg::install",
  "core/pkg::verify",
  "core/pkg::publish",
  "core/store::put",
  "core/store::get",
  "core/store::has",
  "core/sync::push",
  "core/refs::get"
]

[store]
dir = "./.genesis/store"

[refs]
path = "./.genesis/refs.gc"

[op."core/pkg-low::save-lock"]
base_dir = "."
create_dirs = true

[op."core/pkg-low::load-lock"]
base_dir = "."

[op."core/pkg::install"]
base_dir = "."

[op."core/pkg::verify"]
base_dir = "."

[op."core/pkg::publish"]
base_dir = "."
remote_allow = ["{remote_allow}"]

[op."core/sync::push"]
remote_allow = ["{remote_allow}"]
"#
        ),
    )
    .unwrap();
    caps
}

fn write_pkg(dir: &Path) {
    fs::write(dir.join("lib.gc"), "(def mini::x 1)\nmini::x\n").unwrap();
    fs::write(
        dir.join("package.toml"),
        r#"
name = "mini"
version = "0.1.0"
obligations = []
dependencies = []

[[modules]]
path = "lib.gc"
"#,
    )
    .unwrap();
}

fn json_frontend_name(out: &[u8]) -> String {
    let v: JsonValue = serde_json::from_slice(out).unwrap();
    v.pointer("/data/coreform_frontend/name")
        .and_then(|x| x.as_str())
        .unwrap()
        .to_string()
}

fn store_put(dir: &Path, artifact: &Path, caps: &Path, term_src: &str, filename: &str) -> String {
    let p = dir.join(filename);
    fs::write(&p, term_src).unwrap();
    let out = cargo_bin_cmd!("genesis")
        .current_dir(dir)
        .args([
            "--selfhost-only",
            "--selfhost-artifact",
            artifact.to_str().unwrap(),
            "store",
            "--caps",
        ])
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

fn get_remote_ref(remote_dir: &Path, name: &str) -> Option<String> {
    let refs_path = remote_dir.join("v1").join("refs.gc");
    if !refs_path.exists() {
        return None;
    }
    let rdb = RefsDb::open(&refs_path).unwrap();
    rdb.get(name).unwrap()
}

#[test]
fn selfhost_only_gcpm_lifecycle_is_deterministic_and_policy_gated() {
    let td = tempdir().unwrap();
    let dir = td.path();
    let artifact = build_selfhost_artifact(dir);
    let remote_dir = dir.join("remote-registry");
    fs::create_dir_all(&remote_dir).unwrap();
    let remote = format!("file://{}/", remote_dir.display());
    let remote_allow = format!("{remote}v1/");
    let caps = write_caps(dir, &remote_allow);

    write_pkg(dir);

    // init
    cargo_bin_cmd!("genesis")
        .current_dir(dir)
        .args([
            "--json",
            "--selfhost-only",
            "--selfhost-artifact",
            artifact.to_str().unwrap(),
            "gcpm",
            "--caps",
        ])
        .arg(&caps)
        .args(["init", "--workspace", "ws"])
        .assert()
        .success();

    // add + lock(strict) + install(strict)
    let dep_snap_h = store_put(
        dir,
        &artifact,
        &caps,
        r#"{:type :vcs/snapshot :v 1 :kind :package :pkg/name "dep" :pkg/version "1" :modules [] :obligations []}"#,
        "dep_snap.gc",
    );
    cargo_bin_cmd!("genesis")
        .current_dir(dir)
        .args([
            "--selfhost-only",
            "--selfhost-artifact",
            artifact.to_str().unwrap(),
            "gcpm",
            "--caps",
        ])
        .arg(&caps)
        .args(["add", &format!("dep@snapshot:{dep_snap_h}")])
        .assert()
        .success();
    let lock_before = fs::read_to_string(dir.join("genesis.lock")).unwrap();

    let lock_log_a = dir.join("lock_a.gclog");
    let lock_log_b = dir.join("lock_b.gclog");
    let lock_out_a = cargo_bin_cmd!("genesis")
        .current_dir(dir)
        .args([
            "--json",
            "--selfhost-only",
            "--selfhost-artifact",
            artifact.to_str().unwrap(),
            "gcpm",
            "--caps",
        ])
        .arg(&caps)
        .args(["--log", lock_log_a.to_str().unwrap(), "lock", "--strict"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    // Reset lock to the same input state before the second run to verify determinism.
    fs::write(dir.join("genesis.lock"), lock_before).unwrap();
    let lock_out_b = cargo_bin_cmd!("genesis")
        .current_dir(dir)
        .args([
            "--json",
            "--selfhost-only",
            "--selfhost-artifact",
            artifact.to_str().unwrap(),
            "gcpm",
            "--caps",
        ])
        .arg(&caps)
        .args(["--log", lock_log_b.to_str().unwrap(), "lock", "--strict"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    assert_eq!(json_frontend_name(&lock_out_a), "selfhost");
    assert_eq!(json_frontend_name(&lock_out_b), "selfhost");
    let log_a = fs::read_to_string(&lock_log_a).unwrap();
    let log_b = fs::read_to_string(&lock_log_b).unwrap();
    assert_eq!(log_a, log_b);
    let log_term = parse_term(log_a.trim()).unwrap();
    EffectLog::from_term(&log_term).expect("parse deterministic lock log");

    cargo_bin_cmd!("genesis")
        .current_dir(dir)
        .args([
            "--selfhost-only",
            "--selfhost-artifact",
            artifact.to_str().unwrap(),
            "gcpm",
            "--caps",
        ])
        .arg(&caps)
        .args(["install", "--frozen", "--strict"])
        .assert()
        .success();

    // test + run task lifecycle (migrate creates workspace tasks).
    cargo_bin_cmd!("genesis")
        .current_dir(dir)
        .args(["--selfhost-only", "pack", "--pkg", "package.toml"])
        .assert()
        .success();

    cargo_bin_cmd!("genesis")
        .current_dir(dir)
        .args([
            "--selfhost-only",
            "--selfhost-artifact",
            artifact.to_str().unwrap(),
            "gcpm",
            "--caps",
        ])
        .arg(&caps)
        .args(["test", "--pkg", "package.toml"])
        .assert()
        .success();

    cargo_bin_cmd!("genesis")
        .current_dir(dir)
        .args([
            "--selfhost-only",
            "--selfhost-artifact",
            artifact.to_str().unwrap(),
            "gcpm",
            "--caps",
        ])
        .arg(&caps)
        .args(["migrate", "--pkg", "package.toml", "--workspace", "ws"])
        .assert()
        .success();

    cargo_bin_cmd!("genesis")
        .current_dir(dir)
        .args([
            "--selfhost-only",
            "--selfhost-artifact",
            artifact.to_str().unwrap(),
            "gcpm",
            "--caps",
        ])
        .arg(&caps)
        .args(["run", "test"])
        .assert()
        .success();

    // publish policy gate: bad commit rejected, good commit accepted.
    let policy_h = store_put(
        dir,
        &artifact,
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
    let patch_h = store_put(
        dir,
        &artifact,
        &caps,
        r#"{:type :vcs/patch :v 1 :ops []}"#,
        "patch.gc",
    );
    let snap_h = store_put(
        dir,
        &artifact,
        &caps,
        r#"{:type :vcs/snapshot :v 1 :kind :package :pkg/name "mini" :pkg/version "0" :modules [] :obligations []}"#,
        "snap.gc",
    );
    let bad_commit_h = store_put(
        dir,
        &artifact,
        &caps,
        &format!(
            r#"{{
  :type :vcs/commit
  :v 1
  :parents []
  :target {{ :kind :package :name "mini" }}
  :base nil
  :patch "{patch_h}"
  :result "{snap_h}"
  :obligations [core/obligation::unit-tests]
  :evidence []
  :attestations []
  :message "bad"
}}"#
        ),
        "bad_commit.gc",
    );

    cargo_bin_cmd!("genesis")
        .current_dir(dir)
        .args([
            "--selfhost-only",
            "--selfhost-artifact",
            artifact.to_str().unwrap(),
            "gcpm",
            "--caps",
        ])
        .arg(&caps)
        .args([
            "publish",
            "--remote",
            &remote,
            "--ref",
            "refs/heads/main",
            "--policy",
            &policy_h,
            "--commit",
            &bad_commit_h,
        ])
        .assert()
        .failure()
        .code(30);

    let evidence_h = store_put(
        dir,
        &artifact,
        &caps,
        r#"{:type :vcs/evidence :v 1 :kind :unit-tests :inputs [] :outputs [] :data nil}"#,
        "evidence.gc",
    );
    let good_commit_h = store_put(
        dir,
        &artifact,
        &caps,
        &format!(
            r#"{{
  :type :vcs/commit
  :v 1
  :parents []
  :target {{ :kind :package :name "mini" }}
  :base nil
  :patch "{patch_h}"
  :result "{snap_h}"
  :obligations [core/obligation::unit-tests]
  :evidence ["{evidence_h}"]
  :attestations []
  :message "good"
}}"#
        ),
        "good_commit.gc",
    );
    let publish_log = dir.join("publish.gclog");
    let publish_out = cargo_bin_cmd!("genesis")
        .current_dir(dir)
        .args([
            "--json",
            "--selfhost-only",
            "--selfhost-artifact",
            artifact.to_str().unwrap(),
            "gcpm",
            "--caps",
        ])
        .arg(&caps)
        .args([
            "--log",
            publish_log.to_str().unwrap(),
            "publish",
            "--remote",
            &remote,
            "--ref",
            "refs/heads/main",
            "--policy",
            &policy_h,
            "--commit",
            &good_commit_h,
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    assert_eq!(json_frontend_name(&publish_out), "selfhost");
    let publish_value = {
        let v: JsonValue = serde_json::from_slice(&publish_out).unwrap();
        parse_term(v.pointer("/data/value").and_then(|x| x.as_str()).unwrap()).unwrap()
    };
    let Term::Map(mm) = publish_value else {
        panic!("publish value must be a map");
    };
    assert!(mm.contains_key(&TermOrdKey(Term::symbol(":provenance"))));
    assert_eq!(
        get_remote_ref(&remote_dir, "refs/heads/main"),
        Some(good_commit_h)
    );
}
