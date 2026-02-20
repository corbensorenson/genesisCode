use gc_coreform::{canonicalize_module, parse_module, print_module};
use gc_opt::{optimize_module_with_report, stage1_pipeline};
use proptest::prelude::*;

proptest! {
    #![proptest_config(ProptestConfig {
        cases: 192,
        failure_persistence: None,
        max_shrink_iters: 0,
        .. ProptestConfig::default()
    })]

    #[test]
    fn optimizer_rewrite_is_deterministic_on_success(
        input in proptest::collection::vec(any::<char>(), 0..4096).prop_map(|v| v.into_iter().collect::<String>())
    ) {
        let Ok(forms) = parse_module(&input) else {
            return Ok(());
        };
        let Ok(forms) = canonicalize_module(forms) else {
            return Ok(());
        };

        let (opt_a, report_a) = optimize_module_with_report(&forms);
        let (opt_b, report_b) = optimize_module_with_report(&forms);
        let Ok(opt_a) = canonicalize_module(opt_a) else {
            return Ok(());
        };
        let Ok(opt_b) = canonicalize_module(opt_b) else {
            return Ok(());
        };

        prop_assert_eq!(print_module(&opt_a), print_module(&opt_b));
        prop_assert_eq!(report_a.changed, report_b.changed);
        prop_assert_eq!(
            report_a.stats.rewrites_applied,
            report_b.stats.rewrites_applied
        );
    }

    #[test]
    fn stage1_pipeline_never_panics_on_parsed_inputs(
        input in proptest::collection::vec(any::<char>(), 0..4096).prop_map(|v| v.into_iter().collect::<String>())
    ) {
        let Ok(forms) = parse_module(&input) else {
            return Ok(());
        };
        let Ok(forms) = canonicalize_module(forms) else {
            return Ok(());
        };

        let _ = stage1_pipeline(&forms);
    }
}
