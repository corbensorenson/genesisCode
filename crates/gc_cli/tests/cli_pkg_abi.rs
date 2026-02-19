use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use assert_cmd::cargo::cargo_bin_cmd;
use gc_coreform::{Term, TermOrdKey, parse_term};

fn write_caps(dir: &Path) -> PathBuf {
    let caps = dir.join("caps.toml");
    fs::write(&caps, "allow = []\n").unwrap();
    caps
}

fn map_get<'a>(m: &'a BTreeMap<TermOrdKey, Term>, k: &str) -> Option<&'a Term> {
    m.get(&TermOrdKey(Term::symbol(k)))
}

fn map_get_str(m: &BTreeMap<TermOrdKey, Term>, k: &str) -> Option<String> {
    match map_get(m, k) {
        Some(Term::Str(s)) => Some(s.clone()),
        _ => None,
    }
}

fn map_get_vec_symbols(m: &BTreeMap<TermOrdKey, Term>, k: &str) -> Vec<String> {
    match map_get(m, k) {
        Some(Term::Vector(xs)) => xs
            .iter()
            .filter_map(|t| match t {
                Term::Symbol(s) => Some(s.clone()),
                _ => None,
            })
            .collect(),
        _ => Vec::new(),
    }
}

fn write_pkg(dir: &Path) {
    fs::write(
        dir.join("package.toml"),
        r#"
name = "ai_game"
version = "0.1.0"
dependencies = []
obligations = ["core/obligation::unit-tests", "core/obligation::typecheck"]

[[modules]]
path = "game.gc"
"#,
    )
    .unwrap();

    fs::write(
        dir.join("game.gc"),
        r#"
(def ::meta
  '{
    :intent "AI gameplay contract surface"
    :caps [core/task::spawn gfx/gpu::submit]
    :exports [game/core::api game/core::tick]
    :types {
      game/core::api
        (Contract
          [
            [game/core::spawn (Fn (Msg Int) (Prog Int (Eff [core/task::spawn] nil)) (Eff [] nil))]
            [game/core::render (Fn (Msg Int) (Prog Int (Eff [gfx/gpu::submit] nil)) (Eff [] nil))]
          ]
          nil)
      game/core::tick
        (Fn Int (Prog Int (Eff [core/task::spawn] nil)) (Eff [] nil))
    }})

(def game/core::api core/contract::genesis)

(def game/core::tick
  (fn (x)
    (core/effect::pure x)))

game/core::api
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

#[test]
fn gcpm_abi_exports_contract_ops_caps_effects_and_obligations() {
    let td = tempfile::tempdir().unwrap();
    let dir = td.path();
    let caps = write_caps(dir);
    write_pkg(dir);

    let out = cargo_bin_cmd!("genesis")
        .current_dir(dir)
        .env("GENESIS_ALLOW_RUST_ENGINE", "1")
        .args(["--json", "--coreform-frontend", "rust", "gcpm", "--caps"])
        .arg(&caps)
        .args(["abi", "--pkg", "package.toml"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let envelope: serde_json::Value = serde_json::from_slice(&out).unwrap();
    assert_eq!(
        envelope.get("kind").and_then(|x| x.as_str()),
        Some("genesis/pkg-abi-v0.1")
    );

    let value = parse_value_term(&out);
    let Term::Map(root) = value else {
        panic!("abi value must be a map");
    };
    assert_eq!(
        map_get_str(&root, ":schema").as_deref(),
        Some("genesis/pkg-abi-v0.1")
    );

    let required_caps = map_get_vec_symbols(&root, ":required-caps");
    assert!(required_caps.iter().any(|c| c == "core/task::spawn"));
    assert!(required_caps.iter().any(|c| c == "gfx/gpu::submit"));

    let obligations = map_get_vec_symbols(&root, ":obligations");
    assert!(
        obligations
            .iter()
            .any(|o| o == "core/obligation::unit-tests")
    );
    assert!(
        obligations
            .iter()
            .any(|o| o == "core/obligation::typecheck")
    );

    let Some(Term::Map(index)) = map_get(&root, ":index") else {
        panic!("abi value missing :index map");
    };
    let Some(Term::Map(api_entry)) = index.get(&TermOrdKey(Term::symbol("game/core::api"))) else {
        panic!("abi index missing game/core::api");
    };
    let Some(Term::Vector(contract_ops)) = map_get(api_entry, ":contract-ops") else {
        panic!("api entry missing :contract-ops");
    };
    let mut ops = Vec::new();
    for op_entry in contract_ops {
        let Term::Map(mm) = op_entry else {
            continue;
        };
        if let Some(Term::Symbol(op)) = map_get(mm, ":op") {
            ops.push(op.clone());
        }
    }
    assert!(ops.iter().any(|op| op == "game/core::spawn"));
    assert!(ops.iter().any(|op| op == "game/core::render"));
}

#[test]
fn gcpm_abi_output_is_deterministic_for_identical_inputs() {
    let td = tempfile::tempdir().unwrap();
    let dir = td.path();
    let caps = write_caps(dir);
    write_pkg(dir);

    let run_once = || -> String {
        String::from_utf8(
            cargo_bin_cmd!("genesis")
                .current_dir(dir)
                .env("GENESIS_ALLOW_RUST_ENGINE", "1")
                .args(["--coreform-frontend", "rust", "gcpm", "--caps"])
                .arg(&caps)
                .args(["--log", "abi.gclog", "abi", "--pkg", "package.toml"])
                .assert()
                .success()
                .get_output()
                .stdout
                .clone(),
        )
        .unwrap()
    };

    let a = run_once();
    let b = run_once();
    assert_eq!(a, b);
}
