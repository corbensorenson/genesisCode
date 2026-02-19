use std::fs;
use std::path::{Path, PathBuf};

use assert_cmd::cargo::cargo_bin_cmd;
use gc_coreform::{Term, TermOrdKey};

fn cmd() -> assert_cmd::Command {
    cargo_bin_cmd!("genesis_wasi")
}

fn write_caps(dir: &Path, remote_allow: &str) -> PathBuf {
    let caps = dir.join("caps.toml");
    fs::write(
        &caps,
        format!(
            r#"
allow = ["core/sync::push", "core/sync::pull"]

[store]
dir = "./.genesis/store"

[refs]
path = "./.genesis/refs.gc"

[op."core/sync::push"]
remote_allow = ["{remote_allow}"]
wasi_network_profile = "local"

[op."core/sync::pull"]
remote_allow = ["{remote_allow}"]
wasi_network_profile = "local"
"#
        ),
    )
    .unwrap();
    caps
}

fn put_term(store_dir: &Path, term_src: &str) -> String {
    fs::create_dir_all(store_dir).unwrap();
    let term = gc_coreform::parse_term(term_src).unwrap();
    let bytes = gc_coreform::print_term(&term).into_bytes();
    let h = blake3::hash(&bytes).to_hex().to_string();
    fs::write(store_dir.join(&h), bytes).unwrap();
    h
}

fn parse_stdout_term(stdout: &[u8]) -> Term {
    let s = String::from_utf8(stdout.to_vec()).unwrap();
    gc_coreform::parse_term(s.trim()).unwrap()
}

fn map_get_int(t: &Term, key: &str) -> i64 {
    let Term::Map(m) = t else {
        panic!("expected map, got {}", gc_coreform::print_term(t));
    };
    let Some(Term::Int(n)) = m.get(&TermOrdKey(Term::symbol(key))) else {
        panic!("missing int key {key}");
    };
    n.to_string().parse().unwrap()
}

fn map_get_bool(t: &Term, key: &str) -> bool {
    let Term::Map(m) = t else {
        panic!("expected map, got {}", gc_coreform::print_term(t));
    };
    let Some(Term::Bool(v)) = m.get(&TermOrdKey(Term::symbol(key))) else {
        panic!("missing bool key {key}");
    };
    *v
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
fn wasi_sync_push_then_pull_roundtrip_with_ref_update() {
    let td = tempfile::tempdir().unwrap();
    let root = td.path();
    let src = root.join("src");
    let dst = root.join("dst");
    let remote_dir = root.join("remote-registry");
    fs::create_dir_all(&src).unwrap();
    fs::create_dir_all(&dst).unwrap();
    fs::create_dir_all(&remote_dir).unwrap();

    let remote = format!("file://{}/", remote_dir.display());
    let remote_allow = format!("{remote}v1/");
    let src_caps = write_caps(&src, &remote_allow);
    let dst_caps = write_caps(&dst, &remote_allow);

    let src_store = src.join(".genesis").join("store");
    let patch_h = put_term(&src_store, "{:type :vcs/patch :v 1 :ops []}");
    let snap_h = put_term(
        &src_store,
        r#"{:type :vcs/snapshot :v 1 :kind :package :pkg/name "mini" :pkg/version "0.1.0" :members {} :exports [] :deps [] :obligations [core/obligation::unit-tests]}"#,
    );
    let evidence_h = put_term(
        &src_store,
        "{:type :vcs/evidence :v 1 :kind :unit-tests :inputs [] :outputs [] :data nil}",
    );
    let policy_h = put_term(
        &src_store,
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
    );
    let commit_h = put_term(
        &src_store,
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
  :message "sync-cli-roundtrip"
}}"#
        ),
    );

    let push = cmd()
        .current_dir(&src)
        .args(["sync", "--caps"])
        .arg(&src_caps)
        .args([
            "push", "--remote", &remote, "--root", &commit_h, "--root", &policy_h,
        ])
        .args([
            "--set-ref",
            &format!("refs/heads/main:{commit_h}:{policy_h}@nil"),
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let push_t = parse_stdout_term(&push);
    assert!(map_get_bool(&push_t, ":ok"));
    assert_eq!(map_get_int(&push_t, ":refs-updated"), 1);
    assert!(map_get_int(&push_t, ":uploaded") >= 5);

    for h in [&patch_h, &snap_h, &evidence_h, &policy_h, &commit_h] {
        assert!(remote_dir.join("v1").join("store").join(h).exists());
    }
    let remote_refs = gc_effects::RefsDb::open(&remote_dir.join("v1").join("refs.gc")).unwrap();
    assert_eq!(
        remote_refs.get("refs/heads/main").unwrap(),
        Some(commit_h.clone())
    );

    let pull = cmd()
        .current_dir(&dst)
        .args(["sync", "--caps"])
        .arg(&dst_caps)
        .args(["pull", "--remote", &remote, "--ref", "refs/heads/main"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let pull_t = parse_stdout_term(&pull);
    assert!(map_get_bool(&pull_t, ":ok"));
    assert!(map_get_int(&pull_t, ":pulled") >= 4);

    let dst_store = dst.join(".genesis").join("store");
    for h in [&patch_h, &snap_h, &evidence_h, &commit_h] {
        assert!(dst_store.join(h).exists());
    }
    let dst_refs = gc_effects::RefsDb::open(&dst.join(".genesis").join("refs.gc")).unwrap();
    assert_eq!(dst_refs.get("refs/heads/main").unwrap(), Some(commit_h));

    let pull_again = cmd()
        .current_dir(&dst)
        .args(["sync", "--caps"])
        .arg(&dst_caps)
        .args(["pull", "--remote", &remote, "--ref", "refs/heads/main"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let pull_again_t = parse_stdout_term(&pull_again);
    assert_eq!(map_get_int(&pull_again_t, ":pulled"), 0);
    assert!(map_get_int(&pull_again_t, ":present") >= 4);
}

#[test]
fn wasi_sync_push_policy_preflight_rejects_before_upload() {
    let td = tempfile::tempdir().unwrap();
    let root = td.path();
    let src = root.join("src");
    let remote_dir = root.join("remote-registry");
    fs::create_dir_all(&src).unwrap();
    fs::create_dir_all(&remote_dir).unwrap();

    let remote = format!("file://{}/", remote_dir.display());
    let remote_allow = format!("{remote}v1/");
    let caps = write_caps(&src, &remote_allow);

    let src_store = src.join(".genesis").join("store");
    let patch_h = put_term(&src_store, "{:type :vcs/patch :v 1 :ops []}");
    let snap_h = put_term(
        &src_store,
        r#"{:type :vcs/snapshot :v 1 :kind :package :pkg/name "mini" :pkg/version "0.1.0" :members {} :exports [] :deps [] :obligations []}"#,
    );
    let policy_h = put_term(
        &src_store,
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
    );
    let bad_commit_h = put_term(
        &src_store,
        &format!(
            r#"{{
  :type :vcs/commit
  :v 1
  :parents []
  :target {{ :kind :package :name "mini" }}
  :base nil
  :patch "{patch_h}"
  :result "{snap_h}"
  :obligations []
  :evidence []
  :attestations []
  :message "missing-obligation"
}}"#
        ),
    );

    cmd()
        .current_dir(&src)
        .args(["sync", "--caps"])
        .arg(&caps)
        .args([
            "push",
            "--remote",
            &remote,
            "--root",
            &bad_commit_h,
            "--root",
            &policy_h,
        ])
        .args([
            "--set-ref",
            &format!("refs/heads/main:{bad_commit_h}:{policy_h}@nil"),
        ])
        .assert()
        .code(20);

    assert_eq!(get_remote_ref(&remote_dir, "refs/heads/main"), None);
    assert!(
        !remote_dir
            .join("v1")
            .join("store")
            .join(&bad_commit_h)
            .exists()
    );
    assert!(!remote_dir.join("v1").join("store").join(&policy_h).exists());
}

#[test]
fn wasi_sync_pull_ref_conflict_requires_force() {
    let td = tempfile::tempdir().unwrap();
    let root = td.path();
    let src = root.join("src");
    let dst = root.join("dst");
    let remote_dir = root.join("remote-registry");
    fs::create_dir_all(&src).unwrap();
    fs::create_dir_all(&dst).unwrap();
    fs::create_dir_all(&remote_dir).unwrap();

    let remote = format!("file://{}/", remote_dir.display());
    let remote_allow = format!("{remote}v1/");
    let src_caps = write_caps(&src, &remote_allow);
    let dst_caps = write_caps(&dst, &remote_allow);

    let src_store = src.join(".genesis").join("store");
    let patch_h = put_term(&src_store, "{:type :vcs/patch :v 1 :ops []}");
    let snap_h = put_term(
        &src_store,
        r#"{:type :vcs/snapshot :v 1 :kind :package :pkg/name "mini" :pkg/version "0.1.0" :members {} :exports [] :deps [] :obligations [core/obligation::unit-tests]}"#,
    );
    let evidence_h = put_term(
        &src_store,
        "{:type :vcs/evidence :v 1 :kind :unit-tests :inputs [] :outputs [] :data nil}",
    );
    let policy_h = put_term(
        &src_store,
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
    );
    let commit_h = put_term(
        &src_store,
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
  :message "sync-conflict"
}}"#
        ),
    );

    cmd()
        .current_dir(&src)
        .args(["sync", "--caps"])
        .arg(&src_caps)
        .args([
            "push", "--remote", &remote, "--root", &commit_h, "--root", &policy_h,
        ])
        .args([
            "--set-ref",
            &format!("refs/heads/main:{commit_h}:{policy_h}@nil"),
        ])
        .assert()
        .success();

    let conflicting = "a".repeat(64);
    let dst_refs_path = dst.join(".genesis").join("refs.gc");
    let dst_refs = gc_effects::RefsDb::open(&dst_refs_path).unwrap();
    let _ = dst_refs
        .set("refs/heads/main", Some(&conflicting), None)
        .unwrap();

    cmd()
        .current_dir(&dst)
        .args(["sync", "--caps"])
        .arg(&dst_caps)
        .args(["pull", "--remote", &remote, "--ref", "refs/heads/main"])
        .assert()
        .code(20);
    let dst_refs = gc_effects::RefsDb::open(&dst_refs_path).unwrap();
    assert_eq!(
        dst_refs.get("refs/heads/main").unwrap(),
        Some(conflicting.clone())
    );

    cmd()
        .current_dir(&dst)
        .args(["sync", "--caps"])
        .arg(&dst_caps)
        .args([
            "pull",
            "--remote",
            &remote,
            "--ref",
            "refs/heads/main",
            "--force",
        ])
        .assert()
        .success();
    let dst_refs = gc_effects::RefsDb::open(&dst_refs_path).unwrap();
    assert_eq!(dst_refs.get("refs/heads/main").unwrap(), Some(commit_h));
}

#[test]
fn wasi_sync_rejects_missing_wasi_network_profile() {
    let td = tempfile::tempdir().unwrap();
    let root = td.path();
    let src = root.join("src");
    let remote_dir = root.join("remote-registry");
    fs::create_dir_all(&src).unwrap();
    fs::create_dir_all(&remote_dir).unwrap();

    let caps = src.join("caps.toml");
    fs::write(
        &caps,
        format!(
            r#"
allow = ["core/sync::pull"]

[store]
dir = "./.genesis/store"

[refs]
path = "./.genesis/refs.gc"

[op."core/sync::pull"]
remote_allow = ["file://{}/v1/"]
"#,
            remote_dir.display()
        ),
    )
    .unwrap();

    cmd()
        .current_dir(&src)
        .args(["sync", "--caps"])
        .arg(&caps)
        .args([
            "pull",
            "--remote",
            &format!("file://{}/", remote_dir.display()),
            "--ref",
            "refs/heads/main",
        ])
        .assert()
        .failure()
        .code(20)
        .stdout(predicates::str::contains(
            "WASI remote sync access is disabled",
        ));
}

#[test]
fn wasi_sync_local_profile_rejects_https_remote() {
    let td = tempfile::tempdir().unwrap();
    let root = td.path();
    let src = root.join("src");
    fs::create_dir_all(&src).unwrap();

    let caps = src.join("caps.toml");
    fs::write(
        &caps,
        r#"
allow = ["core/sync::pull"]

[store]
dir = "./.genesis/store"

[refs]
path = "./.genesis/refs.gc"

[op."core/sync::pull"]
remote_allow = ["https://registry.example/v1/"]
wasi_network_profile = "local"
"#,
    )
    .unwrap();

    cmd()
        .current_dir(&src)
        .args(["sync", "--caps"])
        .arg(&caps)
        .args([
            "pull",
            "--remote",
            "https://registry.example/",
            "--ref",
            "refs/heads/main",
        ])
        .assert()
        .failure()
        .code(20)
        .stdout(predicates::str::contains(
            "wasi_network_profile=local only allows file:// or inproc:// remotes",
        ));
}
