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
}
