use std::fs;
use std::path::{Path, PathBuf};

use assert_cmd::cargo::cargo_bin_cmd;
use gc_coreform::{Term, TermOrdKey, parse_term};

fn write_caps(dir: &Path) -> PathBuf {
    let caps = dir.join("caps.toml");
    fs::write(&caps, "allow = []\n").unwrap();
    caps
}

fn write_pkg(dir: &Path) {
    fs::write(
        dir.join("package.toml"),
        r#"
name = "mini"
version = "0.1.0"
dependencies = []
obligations = []

[[modules]]
path = "lib.gc"
"#,
    )
    .unwrap();
    fs::write(
        dir.join("lib.gc"),
        r#"
(def mini::x (prim int/add 1 0))
mini::x
"#,
    )
    .unwrap();
}

fn parse_value_term(stdout: &[u8]) -> Term {
    let json: serde_json::Value = serde_json::from_slice(stdout).unwrap();
    parse_term(
        json.pointer("/data/value")
            .and_then(|v| v.as_str())
            .expect("json data.value"),
    )
    .expect("parse value term")
}

fn map_bool(m: &std::collections::BTreeMap<TermOrdKey, Term>, k: &str) -> Option<bool> {
    m.get(&TermOrdKey(Term::symbol(k))).and_then(|t| match t {
        Term::Bool(b) => Some(*b),
        _ => None,
    })
}

#[test]
fn gcpm_self_optimize_promotes_only_after_obligation_gate() {
    let td = tempfile::tempdir().unwrap();
    let dir = td.path();
    let caps = write_caps(dir);
    write_pkg(dir);

    let out = cargo_bin_cmd!("genesis")
        .current_dir(dir)
        .env("GENESIS_ALLOW_RUST_ENGINE", "1")
        .args(["--json", "--coreform-frontend", "rust", "gcpm", "--caps"])
        .arg(&caps)
        .args(["self-optimize", "--pkg", "package.toml"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let env: serde_json::Value = serde_json::from_slice(&out).unwrap();
    assert_eq!(
        env.get("kind").and_then(|x| x.as_str()),
        Some("genesis/pkg-self-optimize-v0.1")
    );
    assert_eq!(env.get("ok").and_then(|x| x.as_bool()), Some(true));

    let value = parse_value_term(&out);
    let Term::Map(m) = value else {
        panic!("value must be map");
    };
    assert_eq!(map_bool(&m, ":promotable"), Some(true));
    assert_eq!(map_bool(&m, ":promoted"), Some(true));

    let src = fs::read_to_string(dir.join("lib.gc")).unwrap();
    assert!(
        !src.contains("int/add"),
        "optimized module should remove add identity"
    );

    let manifest = fs::read_to_string(dir.join("package.toml")).unwrap();
    assert!(
        manifest.contains("core/obligation::translation-validation"),
        "self-optimize should ensure translation-validation obligation is present"
    );
}

#[test]
fn gcpm_self_optimize_dry_run_restores_original_sources() {
    let td = tempfile::tempdir().unwrap();
    let dir = td.path();
    let caps = write_caps(dir);
    write_pkg(dir);
    let original_module = fs::read_to_string(dir.join("lib.gc")).unwrap();
    let original_manifest = fs::read_to_string(dir.join("package.toml")).unwrap();

    let out = cargo_bin_cmd!("genesis")
        .current_dir(dir)
        .env("GENESIS_ALLOW_RUST_ENGINE", "1")
        .args(["--json", "--coreform-frontend", "rust", "gcpm", "--caps"])
        .arg(&caps)
        .args(["self-optimize", "--pkg", "package.toml", "--dry-run"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let env: serde_json::Value = serde_json::from_slice(&out).unwrap();
    assert_eq!(
        env.get("kind").and_then(|x| x.as_str()),
        Some("genesis/pkg-self-optimize-v0.1")
    );
    assert_eq!(env.get("ok").and_then(|x| x.as_bool()), Some(true));

    let value = parse_value_term(&out);
    let Term::Map(m) = value else {
        panic!("value must be map");
    };
    assert_eq!(map_bool(&m, ":promotable"), Some(true));
    assert_eq!(map_bool(&m, ":promoted"), Some(false));

    let module_after = fs::read_to_string(dir.join("lib.gc")).unwrap();
    let manifest_after = fs::read_to_string(dir.join("package.toml")).unwrap();
    assert_eq!(module_after, original_module);
    assert_eq!(manifest_after, original_manifest);
}
