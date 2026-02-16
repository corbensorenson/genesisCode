use proptest::prelude::*;

use gc_coreform::parse_term;
use gc_effects::EffectLog;

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
    fn effect_log_from_term_does_not_panic(input in proptest::collection::vec(any::<char>(), 0..4096).prop_map(|v| v.into_iter().collect::<String>())) {
        let Ok(t) = parse_term(&input) else {
            return Ok(());
        };
        let _ = EffectLog::from_term(&t);
    }
}
