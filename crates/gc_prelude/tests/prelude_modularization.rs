use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;

use gc_coreform::{canonicalize_module, parse_module};
use serde::Deserialize;
use sha2::{Digest, Sha256};

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("workspace root")
        .parent()
        .expect("repo root")
        .to_path_buf()
}

#[derive(Debug, Deserialize)]
struct PreludeManifest {
    version: u64,
    modules: Vec<String>,
    #[serde(default)]
    deps: BTreeMap<String, Vec<String>>,
}

#[derive(Debug, Deserialize)]
struct PreludeManifestHash {
    kind: String,
    manifest: String,
    sha256: String,
}

fn read_prelude_manifest(root: &std::path::Path) -> PreludeManifest {
    let manifest_path = root.join("prelude/modules/manifest.toml");
    let manifest_src = fs::read_to_string(&manifest_path)
        .unwrap_or_else(|e| panic!("failed to read {}: {e}", manifest_path.display()));
    let manifest: PreludeManifest = toml::from_str(&manifest_src)
        .unwrap_or_else(|e| panic!("failed to parse {}: {e}", manifest_path.display()));
    assert_eq!(
        manifest.version,
        1,
        "expected prelude manifest version=1 in {}",
        manifest_path.display()
    );
    assert!(
        !manifest.modules.is_empty(),
        "prelude manifest must list modules"
    );
    manifest
}

fn assembled_prelude_from_modules(root: &std::path::Path) -> String {
    let modules_dir = root.join("prelude/modules");
    let manifest = read_prelude_manifest(root);
    let module_index = manifest
        .modules
        .iter()
        .enumerate()
        .map(|(idx, m)| (m.as_str(), idx))
        .collect::<BTreeMap<_, _>>();

    for (module, deps) in &manifest.deps {
        let Some(&module_idx) = module_index.get(module.as_str()) else {
            panic!("deps table references unknown module `{module}`");
        };
        for dep in deps {
            let Some(&dep_idx) = module_index.get(dep.as_str()) else {
                panic!("module `{module}` depends on unknown module `{dep}`");
            };
            assert!(
                dep_idx < module_idx,
                "dependency order violation: `{module}` depends on `{dep}` but appears before it"
            );
        }
    }

    let mut out = String::new();
    for module in &manifest.modules {
        let module_path = modules_dir.join(module);
        let src = fs::read_to_string(&module_path)
            .unwrap_or_else(|e| panic!("failed to read {}: {e}", module_path.display()));
        out.push_str(&src);
        out.push('\n');
    }
    out
}

#[test]
fn prelude_manifest_hash_matches() {
    let root = repo_root();
    let manifest_path = root.join("prelude/modules/manifest.toml");
    let hash_path = root.join("prelude/prelude.manifest.sha256");
    let hash_src = fs::read_to_string(&hash_path)
        .unwrap_or_else(|e| panic!("failed to read {}: {e}", hash_path.display()));
    let payload: PreludeManifestHash = serde_json::from_str(&hash_src)
        .unwrap_or_else(|e| panic!("failed to parse {}: {e}", hash_path.display()));
    assert_eq!(
        payload.kind, "genesis/prelude-manifest-hash-v0.1",
        "unexpected prelude manifest hash payload kind"
    );
    assert_eq!(payload.manifest, "prelude/modules/manifest.toml");
    let manifest_src = fs::read_to_string(&manifest_path)
        .unwrap_or_else(|e| panic!("failed to read {}: {e}", manifest_path.display()));
    let manifest_value: toml::Value = toml::from_str(&manifest_src)
        .unwrap_or_else(|e| panic!("failed to parse {}: {e}", manifest_path.display()));
    let canonical = canonical_json_from_toml(&manifest_value);
    let observed = Sha256::digest(canonical.as_bytes());
    let observed_hex = observed
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect::<String>();
    assert_eq!(
        payload.sha256, observed_hex,
        "prelude manifest hash file is stale; run scripts/assemble_prelude.sh"
    );
}

fn canonical_json_from_toml(value: &toml::Value) -> String {
    fn to_json_sorted(value: &toml::Value) -> serde_json::Value {
        match value {
            toml::Value::String(s) => serde_json::Value::String(s.clone()),
            toml::Value::Integer(i) => serde_json::Value::Number((*i).into()),
            toml::Value::Float(f) => serde_json::Number::from_f64(*f)
                .map_or(serde_json::Value::Null, serde_json::Value::Number),
            toml::Value::Boolean(b) => serde_json::Value::Bool(*b),
            toml::Value::Datetime(dt) => serde_json::Value::String(dt.to_string()),
            toml::Value::Array(arr) => {
                serde_json::Value::Array(arr.iter().map(to_json_sorted).collect())
            }
            toml::Value::Table(tbl) => {
                let mut out = serde_json::Map::new();
                let mut keys = tbl.keys().cloned().collect::<Vec<_>>();
                keys.sort();
                for key in keys {
                    out.insert(key.clone(), to_json_sorted(&tbl[&key]));
                }
                serde_json::Value::Object(out)
            }
        }
    }
    serde_json::to_string(&to_json_sorted(value)).expect("manifest canonical json serialization")
}

#[test]
fn prelude_gc_matches_module_assembly() {
    let root = repo_root();
    let prelude_path = root.join("prelude/prelude.gc");
    let prelude_src = fs::read_to_string(&prelude_path)
        .unwrap_or_else(|e| panic!("failed to read {}: {e}", prelude_path.display()));
    let assembled = assembled_prelude_from_modules(&root);

    assert_eq!(
        prelude_src, assembled,
        "prelude/prelude.gc is out of sync with prelude/modules/*.gc; run scripts/assemble_prelude.sh"
    );
}

#[test]
fn assembled_prelude_parses_and_canonicalizes() {
    let root = repo_root();
    let assembled = assembled_prelude_from_modules(&root);
    let forms = parse_module(&assembled).expect("assembled prelude must parse");
    canonicalize_module(forms).expect("assembled prelude must canonicalize");
}
