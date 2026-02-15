mod prelude;

pub use prelude::{Prelude, build_prelude};

#[cfg(test)]
mod tests {
    use gc_coreform::{Term, TermOrdKey, canonicalize_module, parse_module};
    use gc_kernel::{Apply, EffectProgram, EffectRequest, EvalCtx, Value, eval_module};

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

    #[test]
    fn coreform_bootstrap_api_fmt_and_hash_match_rust() {
        let src = r#"
            (def t (quote {:b 2 :a 1}))
            {
              :ht (core/coreform::hash-term t)
              :pt (core/coreform::print-term t)
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

        let ht = match m.get(&TermOrdKey(Term::Symbol(":ht".to_string()))).unwrap() {
            Value::Data(Term::Str(s)) => s.clone(),
            other => panic!("expected :ht string, got {}", other.debug_repr()),
        };
        let pt = match m.get(&TermOrdKey(Term::Symbol(":pt".to_string()))).unwrap() {
            Value::Data(Term::Str(s)) => s.clone(),
            other => panic!("expected :pt string, got {}", other.debug_repr()),
        };

        let t = Term::Map(
            [
                (
                    TermOrdKey(Term::Symbol(":a".to_string())),
                    Term::Int(1.into()),
                ),
                (
                    TermOrdKey(Term::Symbol(":b".to_string())),
                    Term::Int(2.into()),
                ),
            ]
            .into_iter()
            .collect(),
        );
        let want_ht = {
            let h = gc_coreform::hash_term(&t);
            let mut s = String::new();
            for b in h {
                s.push_str(&format!("{b:02x}"));
            }
            s
        };
        let want_pt = gc_coreform::print_term(&t);
        assert_eq!(ht, want_ht);
        assert_eq!(pt, want_pt);
    }

    #[test]
    fn coreform_bootstrap_api_fmt_module_matches_rust() {
        let messy = r#"
            (def x   1)
            (def y (fn (a b) a))
            (y x 2)
        "#;
        let src = format!(
            r#"
            (core/coreform::fmt-module "{s}")
        "#,
            // Keep the test source simple: embed as a CoreForm string literal.
            s = messy
                .replace('\\', "\\\\")
                .replace('"', "\\\"")
                .replace('\n', "\\n")
        );
        let forms = canonicalize_module(parse_module(&src).unwrap()).unwrap();
        let mut ctx = EvalCtx::new();
        let prelude = build_prelude(&mut ctx);
        let mut env = prelude.env;

        let v = eval_module(&mut ctx, &mut env, &forms).unwrap();
        let got = match v {
            Value::Data(Term::Str(s)) => s,
            other => panic!("expected string, got {}", other.debug_repr()),
        };

        let want = {
            let p = gc_coreform::parse_module(messy).unwrap();
            let c = gc_coreform::canonicalize_module(p).unwrap();
            gc_coreform::print_module(&c)
        };
        assert_eq!(got, want);
    }

    #[test]
    fn effect_bind_chains_continuations() {
        let src = r#"
            (def prog
              (core/effect::bind
                (core/effect::perform
                  'io/fs::read
                  {:path "x"}
                  (fn (b) (core/effect::pure b)))
                (fn (b) (core/effect::pure (prim bytes/len b)))))
            prog
        "#;
        let forms = canonicalize_module(parse_module(src).unwrap()).unwrap();
        let mut ctx = EvalCtx::new();
        let prelude = build_prelude(&mut ctx);
        let mut env = prelude.env;

        let prog = eval_module(&mut ctx, &mut env, &forms).unwrap();

        let Value::EffectProgram(p) = prog else {
            panic!("expected effect program, got {}", prog.debug_repr());
        };
        let EffectProgram::Perform { request } = p.as_ref() else {
            panic!("expected perform");
        };

        let tok = ctx.protocol.unwrap().effect;
        let Value::Sealed { token, payload } = request.as_ref() else {
            panic!("expected sealed request");
        };
        assert_eq!(*token, tok);

        let Value::EffectRequest(EffectRequest { k, .. }) = payload.as_ref() else {
            panic!("expected effect request payload");
        };

        let resp = Value::Data(Term::Bytes(vec![1, 2, 3, 4]));
        let next = (*k).clone().apply(&mut ctx, resp).unwrap();
        let Value::EffectProgram(p2) = next else {
            panic!("expected effect program");
        };
        let EffectProgram::Pure(v) = p2.as_ref() else {
            panic!("expected pure");
        };
        assert!(matches!(v.as_ref(), Value::Data(Term::Int(i)) if i == &4.into()));
    }

    #[test]
    fn foundation_list_utilities_work_and_validate_proper_lists() {
        let src = r#"
            (def xs (quote (1 2 3)))
            (def ys (quote (4 5)))
            (def imp (prim pair/cons 1 2))

            {
              :len (core/list::len xs)
              :rev (core/list::reverse xs)
              :app (core/list::append xs ys)
              :map (core/list::map xs (fn (x) (core/int::add x 1)))
              :fil (core/list::filter xs (fn (x) (core/int::lt? 1 x)))
              :sum (core/list::foldl xs 0 (fn (acc x) (core/int::add acc x)))
              :bad (core/list::len imp)
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

        assert!(matches!(
            m.get(&TermOrdKey(Term::symbol(":len"))),
            Some(Value::Data(Term::Int(i))) if i == &3.into()
        ));
        assert!(matches!(
            m.get(&TermOrdKey(Term::symbol(":sum"))),
            Some(Value::Data(Term::Int(i))) if i == &6.into()
        ));

        let want_rev = parse_module("(quote (3 2 1))").unwrap();
        let want_rev = want_rev[0].clone();
        let want_rev = eval_module(&mut ctx, &mut env, &[want_rev]).unwrap();
        assert_eq!(
            m.get(&TermOrdKey(Term::symbol(":rev")))
                .unwrap()
                .debug_repr(),
            want_rev.debug_repr()
        );

        let want_app = parse_module("(quote (1 2 3 4 5))").unwrap();
        let want_app = want_app[0].clone();
        let want_app = eval_module(&mut ctx, &mut env, &[want_app]).unwrap();
        assert_eq!(
            m.get(&TermOrdKey(Term::symbol(":app")))
                .unwrap()
                .debug_repr(),
            want_app.debug_repr()
        );

        let want_map = parse_module("(quote (2 3 4))").unwrap();
        let want_map = want_map[0].clone();
        let want_map = eval_module(&mut ctx, &mut env, &[want_map]).unwrap();
        assert_eq!(
            m.get(&TermOrdKey(Term::symbol(":map")))
                .unwrap()
                .debug_repr(),
            want_map.debug_repr()
        );

        let want_fil = parse_module("(quote (2 3))").unwrap();
        let want_fil = want_fil[0].clone();
        let want_fil = eval_module(&mut ctx, &mut env, &[want_fil]).unwrap();
        assert_eq!(
            m.get(&TermOrdKey(Term::symbol(":fil")))
                .unwrap()
                .debug_repr(),
            want_fil.debug_repr()
        );

        let bad = m.get(&TermOrdKey(Term::symbol(":bad"))).unwrap();
        let p = ctx.protocol.expect("protocol tokens reserved");
        match bad {
            Value::Sealed { token, payload } => {
                assert_eq!(*token, p.error);
                let Value::Data(Term::Map(pm)) = payload.as_ref() else {
                    panic!("expected error payload map");
                };
                assert!(matches!(
                    pm.get(&TermOrdKey(Term::symbol(":error/code"))),
                    Some(Term::Str(s)) if s == "core/type-error"
                ));
            }
            other => panic!("expected sealed error, got {}", other.debug_repr()),
        }
    }

    #[test]
    fn effect_catch_handles_error_values() {
        let src = r#"
            (def e (core/error::make2 "my-lib/boom" "boom"))
            (def p (core/effect::pure e))
            (def q (core/effect::catch p (fn (_err) (core/effect::pure 42))))
            q
        "#;
        let forms = canonicalize_module(parse_module(src).unwrap()).unwrap();
        let mut ctx = EvalCtx::new();
        let prelude = build_prelude(&mut ctx);
        let mut env = prelude.env;

        let v = eval_module(&mut ctx, &mut env, &forms).unwrap();
        let Value::EffectProgram(p) = v else {
            panic!("expected effect program, got {}", v.debug_repr());
        };
        let EffectProgram::Pure(v) = p.as_ref() else {
            panic!("expected pure");
        };
        assert!(matches!(v.as_ref(), Value::Data(Term::Int(i)) if i == &42.into()));
    }
}
