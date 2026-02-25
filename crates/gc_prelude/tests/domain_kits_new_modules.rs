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

#[test]
fn new_domain_kit_modules_parse_and_canonicalize() {
    let root = repo_root();
    let modules = [
        "prelude/modules/37_multi_agent_orchestration.gc",
        "prelude/modules/38_realtime_collaboration.gc",
        "prelude/modules/39_ml_pipeline.gc",
        "prelude/modules/40_backend_topology.gc",
    ];
    for rel in modules {
        let path = root.join(rel);
        let src = fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("failed to read {}: {e}", path.display()));
        let forms =
            parse_module(&src).unwrap_or_else(|e| panic!("{} parse failed: {e}", path.display()));
        canonicalize_module(forms)
            .unwrap_or_else(|e| panic!("{} canonicalize failed: {e}", path.display()));
    }
}
