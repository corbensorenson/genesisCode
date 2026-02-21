    use gc_coreform::{Term, canonicalize_module, parse_module, print_module};

    use super::{
        optimize_module, optimize_module_with_report, stage1_pipeline, stage1_validation_report,
    };

    #[test]
    fn folds_int_prim_constants() {
        let src = r#"
            (def x (prim int/add 1 2))
            x
        "#;
        let forms = canonicalize_module(parse_module(src).unwrap()).unwrap();
        let opt = optimize_module(&forms);
        let opt = canonicalize_module(opt).unwrap();

        // Find (def x <expr>) and check it became 3.
        let def = opt
            .iter()
            .find(|t| {
                t.as_proper_list().is_some_and(|xs| {
                    xs.len() == 3
                        && matches!(xs[0], Term::Symbol(s) if s == "def")
                        && matches!(xs[1], Term::Symbol(s) if s == "x")
                })
            })
            .expect("def x");
        let xs = def.as_proper_list().unwrap();
        assert!(matches!(xs[2], Term::Int(i) if i == &3.into()));
    }

    #[test]
    fn does_not_optimize_inside_quote() {
        let src = r#"
            (def x '(prim int/add 1 2))
            x
        "#;
        let forms = canonicalize_module(parse_module(src).unwrap()).unwrap();
        let opt = optimize_module(&forms);
        let opt = canonicalize_module(opt).unwrap();

        let def = opt
            .iter()
            .find(|t| {
                t.as_proper_list().is_some_and(|xs| {
                    xs.len() == 3
                        && matches!(xs[0], Term::Symbol(s) if s == "def")
                        && matches!(xs[1], Term::Symbol(s) if s == "x")
                })
            })
            .expect("def x");
        let xs = def.as_proper_list().unwrap();
        // Still a (quote ...) term, not folded to 3.
        assert!(
            matches!(xs[2].as_proper_list(), Some(q) if q.len() == 2 && matches!(q[0], Term::Symbol(s) if s == "quote"))
        );
    }

    #[test]
    fn egg_optimizer_eliminates_identities_deterministically() {
        let src = r#"
          (def x (prim int/add 0 (prim int/add y 0)))
          x
        "#;
        let forms = canonicalize_module(parse_module(src).unwrap()).unwrap();
        let (opt1, r1) = optimize_module_with_report(&forms);
        let (opt2, r2) = optimize_module_with_report(&forms);
        assert_eq!(
            print_module(&canonicalize_module(opt1.clone()).unwrap()),
            print_module(&canonicalize_module(opt2.clone()).unwrap())
        );
        assert!(r1.stats.egg_runs > 0);
        assert_eq!(r1.stats.egg_runs, r2.stats.egg_runs);

        let opt = canonicalize_module(opt1).unwrap();
        let def = opt
            .iter()
            .find(|t| {
                t.as_proper_list().is_some_and(|xs| {
                    xs.len() == 3
                        && matches!(xs[0], Term::Symbol(s) if s == "def")
                        && matches!(xs[1], Term::Symbol(s) if s == "x")
                })
            })
            .expect("def x");
        let xs = def.as_proper_list().unwrap();
        assert!(matches!(xs[2], Term::Symbol(s) if s == "y"));
    }

    #[test]
    fn stage1_validation_reports_ok_for_pure_equivalent_module() {
        let src = r#"
          (def x (prim int/add 0 (prim int/add 41 1)))
          x
        "#;
        let forms = canonicalize_module(parse_module(src).unwrap()).unwrap();
        let out = stage1_pipeline(&forms).expect("stage1 pipeline");
        assert!(
            out.gate_report.ok,
            "expected gate ok: {:?}",
            out.gate_report
        );
        assert!(out.gate_report.original_value_hash.is_some());
        assert!(out.gate_report.transformed_value_hash.is_some());
    }

    #[test]
    fn stage1_validation_fails_for_effectful_module() {
        let src = r#"
          (core/effect::perform
            'sys/time::now
            nil
            (fn (t) (core/effect::pure t)))
        "#;
        let forms = canonicalize_module(parse_module(src).unwrap()).unwrap();
        let gate = stage1_validation_report(&forms, &forms);
        assert!(!gate.ok);
        assert!(
            gate.errors
                .iter()
                .any(|e| e.contains("effect program produced")),
            "expected effect-related gate error, got {:?}",
            gate.errors
        );
    }
