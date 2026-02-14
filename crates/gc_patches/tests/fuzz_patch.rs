use proptest::prelude::*;

use gc_coreform::parse_term;

proptest! {
    #![proptest_config(ProptestConfig {
        cases: 256,
        max_shrink_iters: 0,
        .. ProptestConfig::default()
    })]

    #[test]
    fn validate_patch_term_does_not_panic(input in proptest::collection::vec(any::<char>(), 0..4096).prop_map(|v| v.into_iter().collect::<String>())) {
        let Ok(t) = parse_term(&input) else {
            return Ok(());
        };
        let _ = gc_patches::validate_patch_term(&t);
    }
}

