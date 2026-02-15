use proptest::prelude::*;

use gc_coreform::{canonicalize_module, parse_module, parse_term, print_module, print_term};

proptest! {
    #![proptest_config(ProptestConfig {
        cases: 256,
        max_shrink_iters: 0,
        .. ProptestConfig::default()
    })]

    #[test]
    fn module_parse_print_parse_is_idempotent_on_success(input in proptest::collection::vec(any::<char>(), 0..4096).prop_map(|v| v.into_iter().collect::<String>())) {
        let Ok(forms1) = parse_module(&input) else {
            return Ok(());
        };
        let Ok(canon1) = canonicalize_module(forms1) else {
            return Ok(());
        };
        let out1 = print_module(&canon1);

        let forms2 = parse_module(&out1).expect("printed module must parse");
        let canon2 = canonicalize_module(forms2).expect("printed module must canonicalize");
        let out2 = print_module(&canon2);
        prop_assert_eq!(out1, out2);
    }

    #[test]
    fn term_parse_print_parse_is_idempotent_on_success(input in proptest::collection::vec(any::<char>(), 0..4096).prop_map(|v| v.into_iter().collect::<String>())) {
        let Ok(t1) = parse_term(&input) else {
            return Ok(());
        };
        let out1 = print_term(&t1);
        let t2 = parse_term(&out1).expect("printed term must parse");
        let out2 = print_term(&t2);
        prop_assert_eq!(out1, out2);
    }
}
