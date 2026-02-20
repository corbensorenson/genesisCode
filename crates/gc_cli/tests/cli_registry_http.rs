use std::fs;
use std::path::{Path, PathBuf};
use std::thread;
use std::time::Duration;

use assert_cmd::cargo::cargo_bin_cmd;
use gc_coreform::{Term, TermOrdKey};

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
allow_http = true
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
  "core/sync::pull",
{publish_line}
]

[store]
dir = "./.genesis/store"

[refs]
path = "./.genesis/refs.gc"

[op."core/sync::push"]
remote_allow = ["{remote_allow}"]
allow_http = true

[op."core/sync::pull"]
remote_allow = ["{remote_allow}"]
allow_http = true
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
    let out = cargo_bin_cmd!("genesis_parity")
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

fn spawn_http_registry(remote_dir: &Path) -> (gc_registry::HttpRegistryServerHandle, String) {
    fs::create_dir_all(remote_dir).unwrap();
    let handle =
        gc_registry::spawn_http_file_registry_server(gc_registry::HttpRegistryServerConfig {
            addr: "127.0.0.1:0".to_string(),
            root: remote_dir.to_path_buf(),
            max_chunk_bytes: 4_194_304,
            max_requests: None,
        })
        .unwrap();
    let remote = format!("http://{}/", handle.bound_addr());
    let client = gc_registry::RegistryClient::new(&remote, None).unwrap();
    for _ in 0..50 {
        if client.ping().is_ok() {
            return (handle, remote);
        }
        thread::sleep(Duration::from_millis(20));
    }
    panic!("registry server failed to become ready");
}

#[test]
fn pkg_publish_roundtrip_over_http_registry_server() {
    let td = tempfile::tempdir().unwrap();
    let dir = td.path();
    let remote_dir = dir.join("remote-registry");
    let (server, remote) = spawn_http_registry(&remote_dir);
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

    cargo_bin_cmd!("genesis_parity")
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
        .success();

    assert_eq!(
        get_remote_ref(&remote_dir, "refs/heads/main"),
        Some(commit_ok)
    );
    server.join().unwrap();
}

#[test]
fn sync_push_pull_roundtrip_over_http_registry_server() {
    let td = tempfile::tempdir().unwrap();
    let root = td.path();
    let src = root.join("src");
    let dst = root.join("dst");
    let remote_dir = root.join("remote-registry");
    fs::create_dir_all(&src).unwrap();
    fs::create_dir_all(&dst).unwrap();
    let (server, remote) = spawn_http_registry(&remote_dir);
    let remote_allow = format!("{remote}v1/");
    let src_caps = write_caps(&src, &remote_allow, false);
    let dst_caps = write_caps(&dst, &remote_allow, false);

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

    let push = cargo_bin_cmd!("genesis")
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

    let pull = cargo_bin_cmd!("genesis")
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
    assert!(dst_store.join(&commit_h).exists());
    assert_eq!(
        gc_effects::RefsDb::open(&dst.join(".genesis").join("refs.gc"))
            .unwrap()
            .get("refs/heads/main")
            .unwrap(),
        Some(commit_h)
    );

    server.join().unwrap();
}
