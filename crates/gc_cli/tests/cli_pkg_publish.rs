use std::fs;
use std::path::{Path, PathBuf};

use assert_cmd::cargo::cargo_bin_cmd;
use gc_coreform::{Term, TermOrdKey, parse_term};

mod support;

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

fn build_selfhost_artifact(dir: &Path) -> PathBuf {
    support::copy_repo_toolchain_artifact(dir)
}

fn keygen_public_key_b64(dir: &Path) -> String {
    let key_path = dir.join("publish_signature_key.toml");
    cargo_bin_cmd!("genesis")
        .current_dir(dir)
        .args(["keygen", "--out"])
        .arg(&key_path)
        .assert()
        .success();
    let key_s = fs::read_to_string(&key_path).unwrap();
    key_s
        .lines()
        .find_map(|l| {
            let l = l.trim();
            l.strip_prefix("pk_b64 = \"")
                .and_then(|rest| rest.strip_suffix('\"'))
        })
        .expect("pk_b64 in key file")
        .to_string()
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
fn pkg_publish_is_policy_gated_and_advances_remote_ref_on_success() {
    let td = tempfile::tempdir().unwrap();
    let dir = td.path();

    // Remote registry is a file-backed registry at <remote_dir>/v1/{store,refs.gc}.
    let remote_dir = dir.join("remote-registry");
    fs::create_dir_all(&remote_dir).unwrap();
    let remote = format!("file://{}/", remote_dir.display());
    let remote_allow = format!("{remote}v1/");

    let caps = write_caps(dir, &remote_allow, true);

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
        .success()
        .stdout(predicates::str::is_match("^[0-9a-f]{64}\n$").unwrap());

    assert_eq!(
        get_remote_ref(&remote_dir, "refs/heads/main"),
        Some(commit_ok)
    );
}

#[test]
fn pkg_publish_enforces_required_attestation_roles_on_protected_refs() {
    let td = tempfile::tempdir().unwrap();
    let dir = td.path();

    let remote_dir = dir.join("remote-registry");
    fs::create_dir_all(&remote_dir).unwrap();
    let remote = format!("file://{}/", remote_dir.display());
    let remote_allow = format!("{remote}v1/");
    let caps = write_caps(dir, &remote_allow, true);
    let pk_b64 = keygen_public_key_b64(dir);

    let policy_hex = cli_store_put(
        dir,
        &caps,
        &format!(
            r#"
{{
  :type :vcs/policy
  :v 1
  :name "policy:roles"
  :refs {{ :frozen-prefixes [] }}
  :classes {{
    :main {{
      :patterns ["refs/**/heads/main"]
      :required-obligations [core/obligation::unit-tests]
      :require-signatures true
      :min-signatures 0
      :allowed-public-keys ["{pk_b64}"]
      :required-attestation-roles [:reviewer :verifier]
      :role-min-signatures {{:reviewer 1 :verifier 1}}
      :independent-role-pairs [{{:left :reviewer :right :verifier}}]
    }}
  }}
}}
"#
        ),
        "policy_roles.gc",
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
    let commit_h = cli_store_put(
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
  :message "missing-roles"
}}"#
        ),
        "commit_missing_roles.gc",
    );
    set_local_ref(dir, &commit_h);

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
        .code(30);

    assert_eq!(get_remote_ref(&remote_dir, "refs/heads/main"), None);
}

fn setup_publish_ok_fixture(dir: &Path) -> (PathBuf, String, String, PathBuf) {
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
fn selfhost_publish_works_without_core_pkg_publish_capability() {
    let td = tempfile::tempdir().unwrap();
    let dir = td.path();
    let remote_dir = dir.join("remote-registry");
    fs::create_dir_all(&remote_dir).unwrap();
    let remote = format!("file://{}/", remote_dir.display());
    let remote_allow = format!("{remote}v1/");
    let caps = write_caps(dir, &remote_allow, false);
    let artifact = build_selfhost_artifact(dir);

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
        .success();
    assert_eq!(
        get_remote_ref(&remote_dir, "refs/heads/main"),
        Some(commit_ok)
    );
}

#[test]
fn pkg_publish_value_matches_between_frontends() {
    let td = tempfile::tempdir().unwrap();
    let rust_dir = td.path().join("rust");
    let self_dir = td.path().join("self");
    fs::create_dir_all(&rust_dir).unwrap();
    fs::create_dir_all(&self_dir).unwrap();

    let (rust_caps, rust_remote, rust_policy_hex, rust_remote_dir) =
        setup_publish_ok_fixture(&rust_dir);
    let (self_caps, self_remote, self_policy_hex, self_remote_dir) =
        setup_publish_ok_fixture(&self_dir);
    let artifact = build_selfhost_artifact(&self_dir);

    let rust_out = cargo_bin_cmd!("genesis_parity")
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
    let self_out = cargo_bin_cmd!("genesis_parity")
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
    let rust_norm = normalize_publish_value(&json_value(&rust_out));
    let Term::Map(rust_map) = &rust_norm else {
        panic!("publish value must be map");
    };
    let Term::Map(prov) = rust_map
        .get(&TermOrdKey(Term::symbol(":provenance")))
        .expect("publish provenance")
    else {
        panic!("publish provenance must be map");
    };
    let Term::Vector(evidence) = prov
        .get(&TermOrdKey(Term::symbol(":evidence")))
        .expect("publish provenance evidence")
    else {
        panic!("publish provenance evidence must be vector");
    };
    assert_eq!(evidence.len(), 1);
    assert_eq!(
        prov.get(&TermOrdKey(Term::symbol(":result")))
            .and_then(|t| match t {
                Term::Str(s) => Some(s.len()),
                _ => None,
            }),
        Some(64)
    );

    let rust_commit = match rust_norm {
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
fn pkg_publish_rejects_invalid_requirements_trace_when_policy_requires_it() {
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
  :name "policy:trace"
  :refs { :frozen-prefixes [] }
  :classes {
    :main {
      :patterns ["refs/**/heads/main"]
      :required-obligations [core/obligation::unit-tests]
      :required-evidence-kinds [:requirements-trace]
      :require-signatures false
    }
  }
}
"#,
        "policy_trace.gc",
    );
    let patch_hex = cli_store_put(dir, &caps, r#"{:type :vcs/patch :v 1 :ops []}"#, "patch.gc");
    let snap_hex = cli_store_put(
        dir,
        &caps,
        r#"{:type :vcs/snapshot :v 1 :kind :package :pkg/name "x" :pkg/version "0" :modules [] :obligations []}"#,
        "snap.gc",
    );
    let trace_hex = cli_store_put(
        dir,
        &caps,
        r#"
{
  :type :vcs/evidence
  :v 1
  :kind :requirements-trace
  :status :pending
  :graph-h "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
  :release {:commit nil :snapshot "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb" :policy nil}
  :requirements [{:id "SYS-1" :level :system :parents [] :hazards [] :links {:evidence-kinds [:requirements-trace]}}]
}
"#,
        "trace_bad.gc",
    );
    let commit_hex = cli_store_put(
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
  :evidence ["{trace_hex}"]
  :attestations []
  :message "trace-bad"
}}"#
        ),
        "commit_trace_bad.gc",
    );
    set_local_ref(dir, &commit_hex);

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
        .code(30);
    assert_eq!(get_remote_ref(&remote_dir, "refs/heads/main"), None);
}

#[test]
fn pkg_publish_rejects_invalid_tool_qualification_when_policy_requires_it() {
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
  :name "policy:qual"
  :refs { :frozen-prefixes [] }
  :classes {
    :main {
      :patterns ["refs/**/heads/main"]
      :required-obligations [core/obligation::unit-tests]
      :required-evidence-kinds [:tool-qualification]
      :require-signatures false
    }
  }
}
"#,
        "policy_qual.gc",
    );
    let patch_hex = cli_store_put(dir, &caps, r#"{:type :vcs/patch :v 1 :ops []}"#, "patch.gc");
    let snap_hex = cli_store_put(
        dir,
        &caps,
        r#"{:type :vcs/snapshot :v 1 :kind :package :pkg/name "x" :pkg/version "0" :modules [] :obligations []}"#,
        "snap.gc",
    );
    let unit_evidence_hex = cli_store_put(
        dir,
        &caps,
        r#"{:type :vcs/evidence :v 1 :kind :unit-tests :inputs [] :outputs [] :data nil}"#,
        "unit_evidence.gc",
    );
    let qual_hex = cli_store_put(
        dir,
        &caps,
        r#"
{
  :type :vcs/evidence
  :v 1
  :kind :tool-qualification
  :status :qualified
  :release {:commit nil :policy nil}
  :requirements ["TQ-1"]
  :tools [{:name "genesis" :path "./genesis" :blake3 "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa" :size-bytes 1}]
  :qualification-tests [{:id "selfhost-boundary" :artifact "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb" :result :fail}]
}
"#,
        "qualification_bad.gc",
    );
    let commit_hex = cli_store_put(
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
  :evidence ["{unit_evidence_hex}" "{qual_hex}"]
  :attestations []
  :message "qual-bad"
}}"#
        ),
        "commit_qual_bad.gc",
    );
    set_local_ref(dir, &commit_hex);

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
        .code(30);
    assert_eq!(get_remote_ref(&remote_dir, "refs/heads/main"), None);
}
