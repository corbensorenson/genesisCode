mod prelude;

pub use prelude::{Prelude, build_prelude};

#[cfg(test)]
mod tests {
    use gc_coreform::{Term, TermOrdKey, canonicalize_module, parse_module};
    use gc_kernel::{EvalCtx, Value, eval_module};

    use super::build_prelude;

    #[test]
    fn protocol_seals_are_unforgeable_by_user_code() {
        let src = r#"
            (def tok (seal))
            (def fake (seal 123 tok))
            (def real (core/protocol::error 456))
            {
              :fake? (core/protocol::is-error fake)
              :fake-un (core/protocol::unerror fake)
              :real? (core/protocol::is-error real)
              :real-un (core/protocol::unerror real)
            }
        "#;
        let forms = canonicalize_module(parse_module(src).unwrap()).unwrap();
        let mut ctx = EvalCtx::new();
        let prelude = build_prelude(&mut ctx);
        let mut env = prelude.env;

        let v = eval_module(&mut ctx, &mut env, &forms).unwrap();
        let Value::Map(m) = v else {
            panic!("expected map, got {}", v.debug_repr());
        };
        let fakeq = m
            .get(&TermOrdKey(Term::Symbol(":fake?".to_string())))
            .unwrap();
        assert!(matches!(fakeq, Value::Data(Term::Bool(false))));

        let fake_un = m
            .get(&TermOrdKey(Term::Symbol(":fake-un".to_string())))
            .unwrap();
        assert!(matches!(fake_un, Value::Data(Term::Nil)));

        let realq = m
            .get(&TermOrdKey(Term::Symbol(":real?".to_string())))
            .unwrap();
        assert!(matches!(realq, Value::Data(Term::Bool(true))));

        let real_un = m
            .get(&TermOrdKey(Term::Symbol(":real-un".to_string())))
            .unwrap();
        assert!(matches!(real_un, Value::Data(Term::Int(i)) if i == &456.into()));
    }

    #[test]
    fn contracts_dispatch_and_explain_follow_proto_chain() {
        let src = r#"
            (def msg (core/msg::make 'foo/bar::x nil))

            (def base
              (core/contract::extend
                core/contract::genesis
                {foo/bar::x (fn (m) 10)}
                {}))

            (def c1 (core/contract::extend base {} {}))
            (def r1 (core/contract::dispatch c1 msg))

            (def c2
              (core/contract::extend
                base
                {foo/bar::x (fn (m) 20)}
                {}))
            (def r2 (core/contract::dispatch c2 msg))

            (def tr (core/contract::explain c1 msg))

            { :r1 r1 :r2 r2 :tr tr }
        "#;
        let forms = canonicalize_module(parse_module(src).unwrap()).unwrap();
        let mut ctx = EvalCtx::new();
        let prelude = build_prelude(&mut ctx);
        let mut env = prelude.env;

        let v = eval_module(&mut ctx, &mut env, &forms).unwrap();
        let Value::Map(m) = v else {
            panic!("expected map, got {}", v.debug_repr());
        };
        let r1 = m.get(&TermOrdKey(Term::Symbol(":r1".to_string()))).unwrap();
        assert!(matches!(r1, Value::Data(Term::Int(i)) if i == &10.into()));

        let r2 = m.get(&TermOrdKey(Term::Symbol(":r2".to_string()))).unwrap();
        assert!(matches!(r2, Value::Data(Term::Int(i)) if i == &20.into()));

        let tr = m.get(&TermOrdKey(Term::Symbol(":tr".to_string()))).unwrap();
        let Value::Data(Term::Map(tm)) = tr else {
            panic!("expected trace map datum, got {}", tr.debug_repr());
        };
        let Term::Vector(steps) = tm
            .get(&TermOrdKey(Term::Symbol(":steps".to_string())))
            .unwrap()
        else {
            panic!("trace missing :steps");
        };
        assert_eq!(steps.len(), 2, "expected c1 then base");
    }

    #[test]
    fn contract_explain_includes_stable_step_fields_and_semantics() {
        let src = r#"
            (def msg (core/msg::make 'foo/bar::x nil))

            (def base
              (core/contract::extend
                core/contract::genesis
                {foo/bar::x (fn (m) 10)}
                {}))

            (def c1 (core/contract::extend base {} {}))
            (def tr1 (core/contract::explain c1 msg))

            (def c2
              (core/contract::extend
                base
                {foo/bar::x (fn (m) 20)}
                {}))
            (def tr2 (core/contract::explain c2 msg))

            { :tr1 tr1 :tr2 tr2 }
        "#;
        let forms = canonicalize_module(parse_module(src).unwrap()).unwrap();
        let mut ctx = EvalCtx::new();
        let prelude = build_prelude(&mut ctx);
        let mut env = prelude.env;

        let v = eval_module(&mut ctx, &mut env, &forms).unwrap();
        let Value::Map(m) = v else {
            panic!("expected map, got {}", v.debug_repr());
        };

        for (k, expect_steps, expect_result) in [(":tr1", 2usize, 10i64), (":tr2", 1usize, 20i64)] {
            let tr = m.get(&TermOrdKey(Term::Symbol(k.to_string()))).unwrap();
            let Value::Data(Term::Map(tm)) = tr else {
                panic!("expected trace map datum, got {}", tr.debug_repr());
            };
            assert!(matches!(
                tm.get(&TermOrdKey(Term::Symbol(":op".to_string()))),
                Some(Term::Symbol(s)) if s == "foo/bar::x"
            ));

            let Term::Vector(steps) = tm
                .get(&TermOrdKey(Term::Symbol(":steps".to_string())))
                .expect("trace missing :steps")
            else {
                panic!("trace :steps must be vector");
            };
            assert_eq!(steps.len(), expect_steps);

            for (i, st) in steps.iter().enumerate() {
                let Term::Map(sm) = st else {
                    panic!("step must be map")
                };
                let cid = match sm.get(&TermOrdKey(Term::Symbol(":contract-id".to_string()))) {
                    Some(Term::Str(s)) => s.as_str(),
                    _ => panic!("step missing :contract-id"),
                };
                assert_eq!(cid.len(), 64, "contract-id must be 32-byte hex");

                let sid = match sm.get(&TermOrdKey(Term::Symbol(":shape-id".to_string()))) {
                    Some(Term::Str(s)) => s.as_str(),
                    _ => panic!("step missing :shape-id"),
                };
                assert_eq!(sid.len(), 64, "shape-id must be 32-byte hex");

                let ov = match sm.get(&TermOrdKey(Term::Symbol(":override".to_string()))) {
                    Some(Term::Bool(b)) => *b,
                    _ => panic!("step missing :override"),
                };
                let unhandled = match sm.get(&TermOrdKey(Term::Symbol(":unhandled".to_string()))) {
                    Some(Term::Bool(b)) => *b,
                    _ => panic!("step missing :unhandled"),
                };
                let has_proto = match sm.get(&TermOrdKey(Term::Symbol(":has-proto".to_string()))) {
                    Some(Term::Bool(b)) => *b,
                    _ => panic!("step missing :has-proto"),
                };

                // Semantics: when there are 2 steps, the first step is a miss (unhandled) that
                // delegates to proto; the last step handles it.
                if expect_steps == 2 {
                    if i == 0 {
                        assert!(!ov);
                        assert!(unhandled);
                        assert!(has_proto);
                    } else {
                        assert!(ov);
                        assert!(!unhandled);
                    }
                } else {
                    assert!(ov);
                    assert!(!unhandled);
                    assert!(has_proto);
                }
            }

            let res = tm
                .get(&TermOrdKey(Term::Symbol(":result".to_string())))
                .expect("trace missing :result");
            assert!(matches!(res, Term::Int(i) if i == &expect_result.into()));
        }
    }

    #[test]
    fn embedded_prelude_wrappers_work() {
        let src = r#"
            (core/int::add 1 2)
        "#;
        let forms = canonicalize_module(parse_module(src).unwrap()).unwrap();
        let mut ctx = EvalCtx::new();
        let prelude = build_prelude(&mut ctx);
        let mut env = prelude.env;

        let v = eval_module(&mut ctx, &mut env, &forms).unwrap();
        assert!(matches!(v, Value::Data(Term::Int(i)) if i == 3.into()));
    }
}
