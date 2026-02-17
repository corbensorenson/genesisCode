use std::fs;
use std::path::{Path, PathBuf};

use assert_cmd::cargo::cargo_bin_cmd;
use gc_coreform::{Term, TermOrdKey, parse_term};

fn cmd() -> assert_cmd::Command {
    cargo_bin_cmd!("genesis_wasi")
}

fn write_caps(dir: &Path) -> PathBuf {
    let caps = dir.join("caps.toml");
    fs::write(
        &caps,
        r#"
allow = [
  "core/pkg::snapshot",
  "core/gpk::export",
  "core/gpk::import",
  "core/store::put",
  "core/store::has",
  "core/store::get",
  "core/refs::set"
]

[store]
dir = "./.genesis/store"

[refs]
path = "./.genesis/refs.gc"

[op."core/pkg::snapshot"]
base_dir = "."

[op."core/gpk::export"]
base_dir = "."

[op."core/gpk::import"]
base_dir = "."
"#,
    )
    .unwrap();
    caps
}

fn store_put(dir: &Path, caps: &Path, term_src: &str, filename: &str) -> String {
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

#[test]
fn wasi_pkg_export_full_from_ref_with_include_evidence_none_excludes_evidence_artifacts() {
    let td = tempfile::tempdir().unwrap();
    let dir = td.path();
    let caps = write_caps(dir);

    fs::write(
        dir.join("package.toml"),
        r#"
name = "mini"
version = "0.0.1"
dependencies = []
obligations = []

[[modules]]
path = "mini.gc"
"#,
    )
    .unwrap();
    fs::write(
        dir.join("mini.gc"),
        r#"
          (def mini::x 1)
          mini::x
        "#,
    )
    .unwrap();

    let snap_out = cmd()
        .current_dir(dir)
        .args(["pkg", "--caps"])
        .arg(&caps)
        .args(["snapshot", "--pkg"])
        .arg("package.toml")
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let snapshot_h = String::from_utf8(snap_out).unwrap().trim().to_string();

    let extra_patch_h = store_put(
        dir,
        &caps,
        r#"{:kind "extra" :v 1 :note "patch-ref"}"#,
        "extra_patch.gc",
    );
    let extra_data_h = store_put(
        dir,
        &caps,
        r#"{:kind "extra" :v 1 :note "evidence-data"}"#,
        "extra_data.gc",
    );
    let patch_h = store_put(
        dir,
        &caps,
        &format!(
            r#"{{
  :type :vcs/patch
  :v 1
  :ops [{{:op :replace :path [] :value "{extra_patch_h}"}}]
}}"#
        ),
        "patch.gc",
    );
    let evidence_h = store_put(
        dir,
        &caps,
        &format!(
            r#"{{
  :type :vcs/evidence
  :v 1
  :kind :unit-tests
  :inputs []
  :outputs []
  :data "{extra_data_h}"
}}"#
        ),
        "evidence.gc",
    );
    let commit_h = store_put(
        dir,
        &caps,
        &format!(
            r#"{{
  :type :vcs/commit
  :v 1
  :parents []
  :target {{:kind :package :name "mini"}}
  :base nil
  :patch "{patch_h}"
  :result "{snapshot_h}"
  :obligations [core/obligation::unit-tests]
  :evidence ["{evidence_h}"]
  :attestations []
  :message "full bundle evidence-none"
}}"#
        ),
        "commit.gc",
    );
    let policy_h = store_put(
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

    cmd()
        .current_dir(dir)
        .args(["refs", "--caps"])
        .arg(&caps)
        .args(["set"])
        .arg("refs/heads/main")
        .arg(&commit_h)
        .args(["--policy"])
        .arg(&policy_h)
        .args(["--expected-old", "nil"])
        .assert()
        .success();

    let bundle = dir.join("mini-full-noev.gpk");
    cmd()
        .current_dir(dir)
        .args(["pkg", "--caps"])
        .arg(&caps)
        .args(["export", "--root", "refs/heads/main"])
        .args(["--out"])
        .arg(&bundle)
        .args([
            "--full",
            "--depth",
            "0",
            "--include-evidence",
            "none",
            "--include-ref",
            "refs/heads/main",
        ])
        .assert()
        .success();
    assert!(bundle.exists());

    let store_dir = dir.join(".genesis").join("store");
    fs::remove_dir_all(&store_dir).unwrap();

    let out = cmd()
        .current_dir(dir)
        .arg("--json")
        .args(["pkg", "--caps"])
        .arg(&caps)
        .args(["import", "--input"])
        .arg(&bundle)
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let env: serde_json::Value = serde_json::from_slice(&out).unwrap();
    let value_s = env["data"]["value"].as_str().expect("data.value");
    let v = parse_term(value_s).expect("parse import value");
    let Term::Map(m) = v else {
        panic!("import value must be a map")
    };
    assert_eq!(
        m.get(&TermOrdKey(Term::symbol(":root"))),
        Some(&Term::Str(commit_h.clone()))
    );

    for h in [&commit_h, &snapshot_h, &patch_h, &extra_patch_h] {
        cmd()
            .current_dir(dir)
            .args(["store", "--caps"])
            .arg(&caps)
            .args(["has"])
            .arg(h)
            .assert()
            .success()
            .stdout("true\n");
    }
    for h in [&evidence_h, &extra_data_h] {
        cmd()
            .current_dir(dir)
            .args(["store", "--caps"])
            .arg(&caps)
            .args(["has"])
            .arg(h)
            .assert()
            .success()
            .stdout("false\n");
    }
}
