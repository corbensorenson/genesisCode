use super::*;
use gc_kernel::{compile_module, eval_compiled_module};

#[test]
fn non_artifact_bootstrap_mode_is_dev_only() {
    let err = enforce_bootstrap_mode_allowed_with_flag(SelfhostBootstrapMode::Embedded, false)
        .expect_err("embedded mode must be rejected outside development mode");
    assert!(format!("{err}").contains("development-only"));
    enforce_bootstrap_mode_allowed_with_flag(SelfhostBootstrapMode::Embedded, true)
        .expect("embedded mode should be allowed in development mode");
}

#[test]
fn trusted_bootstrap_budget_is_bounded_and_profile_controlled() {
    set_bootstrap_runtime_profile_parity_harness(false);
    let production = trusted_bootstrap_budget();
    assert_eq!(production.profile, "production");
    assert!(production.step_limit > 0);
    assert!(production.mem_limits.max_pair_cells.is_some());
    assert!(production.mem_limits.max_vec_len.is_some());
    assert!(production.mem_limits.max_map_len.is_some());
    assert!(production.mem_limits.max_bytes_len.is_some());
    assert!(production.mem_limits.max_string_len.is_some());

    set_bootstrap_runtime_profile_parity_harness(true);
    let parity = trusted_bootstrap_budget();
    assert_eq!(parity.profile, "parity-harness");
    assert!(parity.step_limit >= production.step_limit);
    assert!(
        parity.mem_limits.max_pair_cells.unwrap_or(0)
            >= production.mem_limits.max_pair_cells.unwrap_or(0)
    );
    set_bootstrap_runtime_profile_parity_harness(false);
}

#[test]
fn compiled_cache_blob_roundtrip_preserves_modules() {
    let artifact_h = [7u8; 32];
    let manifest = ToolchainManifest {
        module_paths: vec!["selfhost/a.gc".to_string(), "selfhost/b.gc".to_string()],
        required_symbols: Vec::new(),
    };
    let m1 =
        compile_module(&parse_module("(def selfhost/a::x 11)\nselfhost/a::x\n").expect("parse a"))
            .expect("compile a");
    let m2 =
        compile_module(&parse_module("(def selfhost/b::x 31)\nselfhost/b::x\n").expect("parse b"))
            .expect("compile b");
    let mods = vec![
        (manifest.module_paths[0].clone(), m1),
        (manifest.module_paths[1].clone(), m2),
    ];

    let blob = selfhost_compiled_cache::encode_compiled_cache_blob(artifact_h, &mods)
        .expect("encode cache blob");
    let decoded = selfhost_compiled_cache::decode_compiled_cache_blob(&blob, artifact_h, &manifest)
        .expect("decode cache blob");
    assert_eq!(decoded.len(), 2);
    assert_eq!(decoded[0].0, "selfhost/a.gc");
    assert_eq!(decoded[1].0, "selfhost/b.gc");

    let mut ctx = EvalCtx::new();
    let mut env = Env::empty();
    let out1 = eval_compiled_module(&mut ctx, &mut env, &decoded[0].1).expect("eval module a");
    let out2 = eval_compiled_module(&mut ctx, &mut env, &decoded[1].1).expect("eval module b");
    assert_eq!(out1.debug_repr(), "11");
    assert_eq!(out2.debug_repr(), "31");
}
