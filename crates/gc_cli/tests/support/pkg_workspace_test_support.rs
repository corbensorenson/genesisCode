#![allow(dead_code)]

use std::fs;
use std::path::{Path, PathBuf};

use gc_coreform::{Term, TermOrdKey};

pub fn write_caps(dir: &Path) -> PathBuf {
    let caps = dir.join("caps.toml");
    fs::write(&caps, "allow = []\n").unwrap();
    caps
}

pub fn write_caps_with_store_remote(dir: &Path, remote: &str, remote_allow: &str) -> PathBuf {
    let caps = dir.join("caps.toml");
    fs::write(
        &caps,
        format!(
            r#"
allow = [
  "core/store::get"
]

[store]
dir = "./.genesis/store"
remote = "{remote}"
remote_allow = ["{remote_allow}"]
"#
        ),
    )
    .unwrap();
    caps
}

pub fn put_remote_artifact(remote_dir: &Path, hex: &str, bytes: &[u8]) {
    let store = remote_dir.join("v1").join("store");
    fs::create_dir_all(&store).unwrap();
    fs::write(store.join(hex), bytes).unwrap();
}

pub fn parse_coreform_value_map(stdout: &[u8]) -> std::collections::BTreeMap<TermOrdKey, Term> {
    let v: serde_json::Value = serde_json::from_slice(stdout).unwrap();
    let value = v.pointer("/data/value").and_then(|x| x.as_str()).unwrap();
    let t = gc_coreform::parse_term(value).unwrap();
    let Term::Map(m) = t else {
        panic!("expected map value");
    };
    m
}

pub fn map_string(map: &std::collections::BTreeMap<TermOrdKey, Term>, key: &str) -> String {
    map.get(&TermOrdKey(Term::symbol(key)))
        .and_then(|t| match t {
            Term::Str(s) => Some(s.clone()),
            _ => None,
        })
        .unwrap_or_else(|| panic!("expected string at key `{key}`"))
}

pub fn map_map<'a>(
    map: &'a std::collections::BTreeMap<TermOrdKey, Term>,
    key: &str,
) -> &'a std::collections::BTreeMap<TermOrdKey, Term> {
    map.get(&TermOrdKey(Term::symbol(key)))
        .and_then(|t| match t {
            Term::Map(m) => Some(m),
            _ => None,
        })
        .unwrap_or_else(|| panic!("expected map at key `{key}`"))
}
