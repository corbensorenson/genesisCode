use proptest::prelude::*;

use gc_coreform::parse_term;
use gc_kernel::{MemLimits, StepLimit};
use gc_obligations::rust_coreform_frontend;

proptest! {
    #![proptest_config(ProptestConfig {
        cases: 256,
        // These are fuzz-style invariants; persisting failures to disk is noisy in workspace test crates
        // (and can emit warnings when `PROPTEST_FAILURE_PERSISTENCE` is set in the environment).
        failure_persistence: None,
        max_shrink_iters: 0,
        .. ProptestConfig::default()
    })]

    #[test]
    fn validate_patch_term_does_not_panic(input in proptest::collection::vec(any::<char>(), 0..4096).prop_map(|v| v.into_iter().collect::<String>())) {
        let Ok(t) = parse_term(&input) else {
            return Ok(());
        };
        let frontend = rust_coreform_frontend();
        let _ = gc_patches::validate_patch_term_with_frontend(
            &t,
            &frontend,
            StepLimit::Default,
            MemLimits::default(),
        );
    }
}
