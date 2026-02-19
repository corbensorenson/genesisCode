use std::fs;
use std::path::{Path, PathBuf};

use assert_cmd::cargo::cargo_bin_cmd;
use gc_coreform::{Term, TermOrdKey, parse_term};

mod common;

fn cmd() -> assert_cmd::Command {
    cargo_bin_cmd!("genesis_wasi_parity")
}

fn write_caps(dir: &Path, remote_allow: &str, include_pkg_publish: bool) -> PathBuf {
    let publish_line = if include_pkg_publish {
        "  \"core/pkg-low::publish\",\n"
    } else {
        ""
    };
    let publish_op = if include_pkg_publish {
        format!(
            r#"
[op."core/pkg-low::publish"]
remote_allow = ["{remote_allow}"]
wasi_network_profile = "local"
"#
        )
    } else {
        "".to_string()
    };
    let caps = dir.join("caps.toml");
    fs::write(
        &caps,
        format!(
            r#"
allow = [
  "core/store::put",
  "core/store::get",
  "core/refs::get",
  "core/sync::push",
{publish_line}
]

[store]
dir = "./.genesis/store"

[refs]
path = "./.genesis/refs.gc"

[op."core/sync::push"]
remote_allow = ["{remote_allow}"]
wasi_network_profile = "local"
{publish_op}
"#
        ),
    )
    .unwrap();
    caps
}

fn cli_store_put(dir: &Path, caps: &Path, term_src: &str, filename: &str) -> String {
    let p = dir.join(filename);
    fs::write(&p, term_src).unwrap();
    let out = cmd()
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

fn build_selfhost_artifact(dir: &Path) -> PathBuf {
    common::copy_repo_selfhost_toolchain_artifact(dir)
}

fn json_value(stdout: &[u8]) -> String {
    let v: serde_json::Value = serde_json::from_slice(stdout).unwrap();
    v.get("data")
        .and_then(|d| d.get("value"))
        .and_then(|x| x.as_str())
        .unwrap()
        .to_string()
}

fn json_frontend_name(stdout: &[u8]) -> String {
    let v: serde_json::Value = serde_json::from_slice(stdout).unwrap();
    v.get("data")
        .and_then(|d| d.get("coreform_frontend"))
        .and_then(|cf| cf.get("name"))
        .and_then(|x| x.as_str())
        .unwrap()
        .to_string()
}

fn normalize_publish_value(s: &str) -> Term {
    fn walk(t: &Term) -> Term {
        match t {
            Term::Map(m) => {
                let mut out = std::collections::BTreeMap::new();
                for (k, v) in m {
                    if k.0 == Term::symbol(":remote") {
                        continue;
                    }
                    out.insert(k.clone(), walk(v));
                }
                Term::Map(out)
            }
            Term::Vector(xs) => Term::Vector(xs.iter().map(walk).collect()),
            Term::Pair(a, d) => Term::Pair(Box::new(walk(a)), Box::new(walk(d))),
            other => other.clone(),
        }
    }
    walk(&parse_term(s).unwrap())
}

#[test]
fn wasi_pkg_publish_is_policy_gated_and_advances_remote_ref_on_success() {
    let td = tempfile::tempdir().unwrap();
    let dir = td.path();

    let remote_dir = dir.join("remote-registry");
    fs::create_dir_all(&remote_dir).unwrap();
    let remote = format!("file://{}/", remote_dir.display());
    let remote_allow = format!("{remote}v1/");

    let caps = write_caps(dir, &remote_allow, true);

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

    cmd()
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

    cmd()
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

fn setup_publish_ok_fixture(
    dir: &Path,
    include_pkg_publish: bool,
) -> (PathBuf, String, String, PathBuf) {
    let remote_dir = dir.join("remote-registry");
    fs::create_dir_all(&remote_dir).unwrap();
    let remote = format!("file://{}/", remote_dir.display());
    let remote_allow = format!("{remote}v1/");
    let caps = write_caps(dir, &remote_allow, include_pkg_publish);

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
    (caps, remote, policy_hex, remote_dir)
}

#[test]
fn wasi_pkg_publish_value_matches_between_frontends() {
    let td = tempfile::tempdir().unwrap();
    let rust_dir = td.path().join("rust");
    let self_dir = td.path().join("self");
    fs::create_dir_all(&rust_dir).unwrap();
    fs::create_dir_all(&self_dir).unwrap();

    let (rust_caps, rust_remote, rust_policy_hex, rust_remote_dir) =
        setup_publish_ok_fixture(&rust_dir, true);
    let (self_caps, self_remote, self_policy_hex, self_remote_dir) =
        setup_publish_ok_fixture(&self_dir, false);
    let artifact = build_selfhost_artifact(&self_dir);

    let rust_out = cmd()
        .current_dir(&rust_dir)
        .arg("--json")
        .args(["--coreform-frontend", "rust"])
        .args(["pkg", "--caps"])
        .arg(&rust_caps)
        .args([
            "publish",
            "--remote",
            &rust_remote,
            "--ref",
            "refs/heads/main",
            "--policy",
            &rust_policy_hex,
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let self_out = cmd()
        .current_dir(&self_dir)
        .arg("--json")
        .args(["--coreform-frontend", "selfhost"])
        .args(["--selfhost-artifact", artifact.to_str().unwrap()])
        .args(["pkg", "--caps"])
        .arg(&self_caps)
        .args([
            "publish",
            "--remote",
            &self_remote,
            "--ref",
            "refs/heads/main",
            "--policy",
            &self_policy_hex,
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    assert_eq!(json_frontend_name(&rust_out), "rust");
    assert_eq!(json_frontend_name(&self_out), "selfhost");
    assert_eq!(
        normalize_publish_value(&json_value(&rust_out)),
        normalize_publish_value(&json_value(&self_out))
    );

    let rust_commit = match normalize_publish_value(&json_value(&rust_out)) {
        Term::Map(m) => match m.get(&TermOrdKey(Term::symbol(":commit"))) {
            Some(Term::Str(s)) => s.clone(),
            _ => panic!("missing :commit"),
        },
        _ => panic!("publish value must be map"),
    };
    let self_commit = match normalize_publish_value(&json_value(&self_out)) {
        Term::Map(m) => match m.get(&TermOrdKey(Term::symbol(":commit"))) {
            Some(Term::Str(s)) => s.clone(),
            _ => panic!("missing :commit"),
        },
        _ => panic!("publish value must be map"),
    };
    assert_eq!(
        get_remote_ref(&rust_remote_dir, "refs/heads/main"),
        Some(rust_commit)
    );
    assert_eq!(
        get_remote_ref(&self_remote_dir, "refs/heads/main"),
        Some(self_commit)
    );
}

#[test]
fn wasi_selfhost_publish_works_without_core_pkg_publish_capability() {
    let td = tempfile::tempdir().unwrap();
    let dir = td.path();
    let (caps, remote, policy_hex, remote_dir) = setup_publish_ok_fixture(dir, false);
    let artifact = build_selfhost_artifact(dir);

    let out = cmd()
        .current_dir(dir)
        .arg("--json")
        .args(["--coreform-frontend", "selfhost"])
        .args(["--selfhost-artifact", artifact.to_str().unwrap()])
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
        .get_output()
        .stdout
        .clone();

    let commit = match normalize_publish_value(&json_value(&out)) {
        Term::Map(m) => match m.get(&TermOrdKey(Term::symbol(":commit"))) {
            Some(Term::Str(s)) => s.clone(),
            _ => panic!("missing :commit"),
        },
        _ => panic!("publish value must be map"),
    };
    assert_eq!(get_remote_ref(&remote_dir, "refs/heads/main"), Some(commit));
}
