use std::fs;
use std::path::PathBuf;

use gc_coreform::{canonicalize_module, parse_module};

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("workspace root")
        .parent()
        .expect("repo root")
        .to_path_buf()
}

fn assembled_prelude_from_modules(root: &std::path::Path) -> String {
    let modules_dir = root.join("prelude/modules");
    let mut entries = fs::read_dir(&modules_dir)
        .unwrap_or_else(|e| panic!("failed to read {}: {e}", modules_dir.display()))
        .map(|res| res.expect("dir entry"))
        .filter(|entry| entry.path().extension().is_some_and(|ext| ext == "gc"))
        .map(|entry| entry.path())
        .collect::<Vec<_>>();
    entries.sort();

    assert!(
        !entries.is_empty(),
        "no .gc modules found in {}",
        modules_dir.display()
    );

    let mut out = String::new();
    for module in entries {
        let src = fs::read_to_string(&module)
            .unwrap_or_else(|e| panic!("failed to read {}: {e}", module.display()));
        out.push_str(&src);
        out.push('\n');
    }
    out
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
