use super::*;
use crate::Apply;

#[test]
fn inline_slot_segment_mutates_in_place_until_runtime_is_shared() {
    let mut runtime = RuntimeEnv::new(
        Env::empty(),
        CompiledModuleCells::empty(),
        Arc::new(CompiledCoverageSites::default()),
        None,
    );
    let allocation = runtime.inline_slots.as_ptr();
    for _ in 0..512 {
        runtime.push_slot(Value::data(Term::Nil));
        assert_eq!(runtime.inline_slots.as_ptr(), allocation);
    }

    let mut fork = runtime.clone();
    fork.push_slot(Value::data(Term::Nil));
    assert!(!Shared::ptr_eq(&runtime.inline_slots, &fork.inline_slots));
    assert_eq!(runtime.inline_slots.len(), 512);
    assert_eq!(fork.inline_slots.len(), 513);
}

fn run_capture_case(
    source: &str,
    engine: u8,
    ctx: &mut EvalCtx,
    env: &mut Env,
) -> Result<Value, KernelError> {
    let forms =
        gc_coreform::canonicalize_module(gc_coreform::parse_module(source).unwrap()).unwrap();
    if engine == 0 {
        crate::eval_module(ctx, env, &forms)
    } else {
        let module = compile_module(&forms).unwrap();
        let module = if engine == 2 {
            decode_compiled_module_blob(&encode_compiled_module_blob(&module).unwrap()).unwrap()
        } else {
            module
        };
        eval_compiled_module(ctx, env, &module)
    }
}

#[test]
fn named_module_scope_preserves_warm_updates_forward_rhs_and_replaced_recursion() {
    let cases = [
        (
            "(def current 1) (def retained (fn (unused) current))",
            "(def current 2)",
            "(retained nil)",
            "2",
        ),
        (
            "(def current 1) (def make (fn (unused) (fn (ignored) current))) (def retained (make nil))",
            "(def current 2)",
            "(retained nil)",
            "2",
        ),
        (
            "(def current 1)",
            "(def current (prim int/add current 1))",
            "current",
            "2",
        ),
        (
            "(def current 1) (def retained (fn (unused) current))",
            "(def current (prim int/add current 1)) (def current (prim int/add current 1))",
            "(retained nil)",
            "3",
        ),
        (
            "(def recur (fn (n) (if (prim int/eq? n 0) 0 (recur (prim int/sub n 1))))) (def retained recur)",
            "(def recur (fn (n) (prim int/add 100 n)))",
            "(retained 1)",
            "100",
        ),
    ];
    for step_limit in [Some(500), None] {
        for (initial, update, call, expected) in cases {
            let mut reference = None;
            for engine in 0..3 {
                let mut ctx = EvalCtx::with_step_limit(step_limit);
                let mut env = Env::empty();
                run_capture_case(initial, engine, &mut ctx, &mut env).unwrap();
                run_capture_case(update, engine, &mut ctx, &mut env).unwrap();
                let result = run_capture_case(call, engine, &mut ctx, &mut env).unwrap();
                assert_eq!(result.debug_repr(), expected, "engine={engine} call={call}");
                let observation = (crate::value_hash(&result), ctx.steps);
                if let Some(reference) = reference {
                    assert_eq!(observation, reference, "engine={engine} call={call}");
                } else {
                    reference = Some(observation);
                }
            }
        }
    }
}

#[test]
fn named_module_scope_preserves_parent_shadowing_isolation_and_missing_forward_names() {
    for engine in 0..3 {
        let mut parent = Env::empty();
        parent.set_local("current", Value::data(Term::Int(1.into())));
        let mut child = Env::with_binding(&parent, "unrelated", Value::data(Term::Nil));
        let mut ctx = EvalCtx::with_step_limit(Some(500));
        let result = run_capture_case(
            "(def current (prim int/add current 1)) current",
            engine,
            &mut ctx,
            &mut child,
        )
        .unwrap();
        assert_eq!(result.debug_repr(), "2");
        assert_eq!(parent.get("current").unwrap().debug_repr(), "1");
        let mut isolated = Env::empty();
        let error = run_capture_case(
            "(def retained (fn (unused) later)) (retained nil) (def later 1)",
            engine,
            &mut ctx,
            &mut isolated,
        )
        .unwrap_err();
        assert!(matches!(error.kind, KernelErrorKind::Unbound));
        assert_eq!(error.msg, "unbound symbol: later");
        assert!(isolated.get("current").is_none());
    }
}

#[test]
fn named_lexical_capture_live_units_are_exact_across_tiers_and_blob_roundtrip() {
    let source = format!(
        "(let ((captured {:?})) (fn (unused) captured))",
        "x".repeat(16384)
    );
    for engine in 0..3 {
        for limit in [u64::MAX, 16420, 16419, 2048] {
            let mut ctx = EvalCtx::with_step_limit(Some(500));
            ctx.set_mem_limits(crate::MemLimits {
                max_live_units: Some(limit),
                ..Default::default()
            });
            let mut env = Env::empty();
            let result = run_capture_case(&source, engine, &mut ctx, &mut env);
            assert_eq!(
                ctx.observed_counters().mem.live_units,
                16420,
                "engine={engine}"
            );
            if limit < 16420 {
                let resource = result.unwrap_err().resource_limit.unwrap();
                assert_eq!(
                    (resource.dimension, resource.observed, resource.limit),
                    ("live-units", 16420, limit)
                );
            } else {
                let value = result
                    .unwrap()
                    .apply(
                        &mut EvalCtx::with_step_limit(Some(500)),
                        Value::data(Term::Nil),
                    )
                    .unwrap();
                assert!(
                    matches!(value, Value::Data(ref term) if matches!(term.as_ref(), Term::Str(text) if text.len() == 16384))
                );
            }
        }
    }
}

#[test]
fn named_module_slot_inventory_rejects_inconsistent_duplicate_and_literal_names() {
    let forms = gc_coreform::parse_module("(def current 1) (def another 2) current").unwrap();
    let original = encode_compiled_module_blob(&compile_module(&forms).unwrap()).unwrap();
    assert_eq!(&original[14..21], b"current");
    assert_eq!(&original[25..32], b"another");
    for (at, replacement, message) in [
        (
            14,
            b"changed",
            "compiled def name disagrees with module slot",
        ),
        (25, b"current", "compiled module slot names are duplicated"),
        (14, b"1234567", "compiled module slot name is not canonical"),
    ] {
        let mut blob = original.clone();
        blob[at..at + 7].copy_from_slice(replacement);
        let error = decode_compiled_module_blob(&blob).unwrap_err();
        assert!(matches!(error.kind, KernelErrorKind::Internal));
        assert_eq!(error.msg, message);
    }
    assert_eq!(
        encode_compiled_module_blob(&decode_compiled_module_blob(&original).unwrap()).unwrap(),
        original
    );
}

#[test]
fn named_sparse_captures_allocate_only_retained_values_and_preserve_nested_depths() {
    let mut binds = vec!["(keep 1)".to_string()];
    binds.extend((0..5000).map(|n| format!("(unused{n} {n})")));
    let source = format!(
        "(let ({}) (fn (arg) (fn (ignored) (prim int/add keep arg))))",
        binds.join(" ")
    );
    for engine in [1, 2] {
        let mut ctx = EvalCtx::with_step_limit(Some(100000));
        let value = run_capture_case(&source, engine, &mut ctx, &mut Env::empty()).unwrap();
        let Value::CompiledClosure(closure) = &value else {
            panic!("not compiled closure")
        };
        let captured = closure.compiled_env.as_ref().unwrap();
        assert_eq!(captured.captured_value_count(), 1);
        assert_eq!(captured.slot_span(), 5001);
        assert_eq!(captured.values.capacity(), 1);
        assert_eq!(
            captured
                .named_bindings()
                .map(|(name, value)| (name, value.debug_repr()))
                .collect::<Vec<_>>(),
            vec![("keep", "1".to_string())]
        );
        let nested = value
            .apply(&mut ctx, Value::data(Term::Int(41.into())))
            .unwrap();
        assert_eq!(
            nested
                .apply(&mut ctx, Value::data(Term::Nil))
                .unwrap()
                .debug_repr(),
            "42"
        );
    }
}

#[test]
fn named_capture_semantic_live_graph_matches_reference_sharing_shadowing_and_cycles() {
    let cases = [
        "(let ((a [1 2]) (b a)) (fn (unused) (prim int/add (prim vec/len a) (prim vec/len b))))",
        "(let ((captured 1)) (fn (unused) (let ((captured 2)) (fn (ignored) captured))))",
        "(def recur (let ((captured 1)) (fn (n) (if (prim int/eq? n 0) captured (recur (prim int/sub n 1)))))) recur",
        "(def make (fn (left) (fn (right) (fn (unused) (prim int/add left right))))) (make 1 2)",
    ];
    for source in cases {
        let mut reference = None;
        for engine in 0..3 {
            let mut ctx = EvalCtx::with_step_limit(Some(1000));
            let mut env = Env::empty();
            let value = run_capture_case(source, engine, &mut ctx, &mut env).unwrap();
            let units = crate::logical_heap::logical_live_units(&[&value, &value], &[&env]);
            if let Some(reference) = reference {
                assert_eq!(units, reference, "engine={engine} source={source}");
            } else {
                reference = Some(units);
            }
        }
    }
}

#[test]
fn named_capture_external_scope_and_warm_cycle_roots_remain_live_then_reclaim() {
    let mut reference = None;
    for engine in 0..3 {
        let host = Env::with_binding(
            &Env::empty(),
            "host/base",
            Value::data(Term::Int(40.into())),
        );
        let mut env = Env::with_binding(&host, "sentinel", Value::data(Term::Nil));
        let mut ctx = EvalCtx::with_step_limit(Some(1000));
        let value = run_capture_case("(def retained (let ((offset 2)) (fn (unused) (prim int/add host/base offset)))) retained", engine, &mut ctx, &mut env).unwrap();
        let units = crate::logical_heap::logical_live_units(&[&value], &[&env]);
        if let Some(reference) = reference {
            assert_eq!(units, reference, "engine={engine}");
        } else {
            reference = Some(units);
        }
        let alive = match &value {
            Value::Closure(c) => c.weak_alive_probe(),
            Value::CompiledClosure(c) => c.weak_alive_probe(),
            _ => panic!("not closure"),
        };
        drop(env);
        crate::cycle::collect_cycles();
        assert!(alive(), "retained result must root its module cycle");
        assert_eq!(
            value
                .apply(&mut ctx, Value::data(Term::Nil))
                .unwrap()
                .debug_repr(),
            "42"
        );
        run_capture_case("nil", engine, &mut ctx, &mut Env::empty()).unwrap();
        assert!(!alive(), "retired module/capture cycle leaked");
    }
}

#[test]
fn compiled_semantic_admission_rejects_name_depth_and_body_ir_contradictions() {
    fn mutate(expr: &mut Arc<CExpr>) {
        match Arc::make_mut(expr) {
            CExpr::Var { name, .. } if name == "keeper" => *name = "other0".to_string(),
            CExpr::Let(bindings, body) => {
                for (_, rhs) in bindings {
                    mutate(rhs);
                }
                mutate(body);
            }
            CExpr::FnUnary { body, .. } => mutate(body),
            CExpr::Prim { args, .. } => {
                for arg in args {
                    mutate(arg);
                }
            }
            _ => {}
        }
    }
    let source = format!(
        "(let ((keeper {:?}) (other0 1)) (fn (arg) (prim int/add (prim str/len keeper) other0)))",
        "x".repeat(16384)
    );
    let forms =
        gc_coreform::canonicalize_module(gc_coreform::parse_module(&source).unwrap()).unwrap();
    let valid = compile_module(&forms).unwrap();
    let blob = encode_compiled_module_blob(&valid).unwrap();
    assert_eq!(
        encode_compiled_module_blob(&decode_compiled_module_blob(&blob).unwrap()).unwrap(),
        blob
    );
    let mut corrupt = valid;
    for form in &mut corrupt.forms {
        if let CompiledForm::Expr(expr) = form {
            mutate(expr);
        }
    }
    let error =
        decode_compiled_module_blob(&encode_compiled_module_blob(&corrupt).unwrap()).unwrap_err();
    assert!(matches!(error.kind, KernelErrorKind::Internal));
    assert_eq!(
        error.msg,
        "compiled module semantic metadata is inconsistent"
    );
    let forms = gc_coreform::parse_module("(fn (arg) (prim int/add arg 1))").unwrap();
    let mut corrupt = compile_module(&forms).unwrap();
    let CompiledForm::Expr(expr) = &mut corrupt.forms[0] else {
        panic!("not expr")
    };
    let CExpr::FnUnary { body, .. } = Arc::make_mut(expr) else {
        panic!("not fn")
    };
    *body = Arc::new(CExpr::Atom(Term::Int(42.into())));
    assert!(decode_compiled_module_blob(&encode_compiled_module_blob(&corrupt).unwrap()).is_err());
}

#[test]
fn compiled_semantic_admission_preserves_all_source_forms_sugar_and_namespaces() {
    let sources = [
        "(fn (x) nil)",
        "(fn (x) true)",
        "(fn (x) 42)",
        "(fn (x) \"text\")",
        "(fn (x) b\"00ff\")",
        "(fn (x) x)",
        "(fn (x) [1 (quote sym)])",
        "(fn (x) {:a x :b (prim int/add x 1)})",
        "(fn (x) (quote (a . (b))))",
        "(fn (x) (if x 1 2))",
        "(fn (x) (begin x 42))",
        "(fn (x) (begin x))",
        "(fn (x) (let ((a x) (b a)) b))",
        "(fn (x) (let () x 42))",
        "(fn (x) (fn (a b) x a b))",
        "(fn (x) (prim missing/op x))",
        "(fn (x) (seal))",
        "(fn (x) (seal x x))",
        "(fn (x) (unseal x x))",
        "(fn (x) (x 1))",
        "(fn (x) (x 1 2 3))",
        "(fn (x) (((x 1) 2) 3))",
        "(fn (x) ((begin (x 1)) 2))",
        "(fn (x) ((x) 1))",
        "(def current 1) (def fn1 (fn (x) current)) (def current 2) (fn1 0)",
        "(let ((current 1)) (fn (x) (let ((current 2)) (fn (current) current))))",
    ];
    for source in sources {
        let raw = gc_coreform::parse_module(source).unwrap();
        let normalized = gc_coreform::canonicalize_module(raw.clone()).unwrap();
        for forms in [&raw, &normalized] {
            for namespace in ["", "module:qualified"] {
                let compiled = compile_module_with_site_namespace(forms, namespace).unwrap();
                let blob = encode_compiled_module_blob(&compiled).unwrap();
                let restored = decode_compiled_module_blob(&blob)
                    .unwrap_or_else(|error| panic!("{source} {namespace}: {error}"));
                assert_eq!(encode_compiled_module_blob(&restored).unwrap(), blob);
                assert_eq!(
                    compiled_module_coverage_manifest_from_compiled(&compiled),
                    compiled_module_coverage_manifest_from_compiled(&restored)
                );
            }
        }
    }
}

#[test]
fn compiled_semantic_admission_checks_every_redundant_body_field() {
    type Mutation = fn(&mut CExpr);
    let cases: &[(&str, Mutation)] = &[
        ("(fn (x) 42)", |body| {
            *body = CExpr::Atom(Term::Int(43.into()));
        }),
        ("(fn (x) x)", |body| {
            let CExpr::Var { resolution, .. } = body else {
                panic!("not var")
            };
            *resolution = VarResolution::Local { depth: 0, slot: 1 };
        }),
        ("(fn (x) x)", |body| {
            let CExpr::Var { resolution, .. } = body else {
                panic!("not var")
            };
            *resolution = VarResolution::Local { depth: 1, slot: 0 };
        }),
        ("(fn (x) x)", |body| {
            let CExpr::Var { resolution, .. } = body else {
                panic!("not var")
            };
            *resolution = VarResolution::External;
        }),
        ("(fn (x) [1 2])", |body| {
            let CExpr::Vector(items) = body else {
                panic!("not vector")
            };
            items.reverse();
        }),
        ("(fn (x) {:a 1 :b 2})", |body| {
            let CExpr::Map(items) = body else {
                panic!("not map")
            };
            items.reverse();
        }),
        ("(fn (x) (quote (1 2)))", |body| {
            *body = CExpr::Quote(Term::Int(42.into()));
        }),
        ("(fn (x) (if x 1 2))", |body| {
            let CExpr::If {
                then_expr,
                else_expr,
                ..
            } = body
            else {
                panic!("not if")
            };
            std::mem::swap(then_expr, else_expr);
        }),
        ("(fn (x) (begin 1 2))", |body| {
            let CExpr::Begin(items) = body else {
                panic!("not begin")
            };
            items.reverse();
        }),
        ("(fn (x) (let ((a 1)) a))", |body| {
            let CExpr::Let(bindings, _) = body else {
                panic!("not let")
            };
            bindings[0].0 = "changed".to_string();
        }),
        ("(fn (x) (fn (a) a))", |body| {
            let CExpr::FnUnary { param, .. } = body else {
                panic!("not fn")
            };
            *param = "changed".to_string();
        }),
        ("(fn (x) (prim int/add x 1))", |body| {
            let CExpr::Prim { op, .. } = body else {
                panic!("not prim")
            };
            *op = PrimOp::IntMul;
        }),
        ("(fn (x) (prim missing/op x))", |body| {
            let CExpr::PrimUnknown { op, .. } = body else {
                panic!("not unknown")
            };
            *op = "changed/op".to_string();
        }),
        ("(fn (x) (seal))", |body| {
            *body = CExpr::Atom(Term::Nil);
        }),
        ("(fn (x) (seal x x))", |body| {
            let CExpr::Seal(left, right) = body else {
                panic!("not seal")
            };
            *body = CExpr::Unseal(left.clone(), right.clone());
        }),
        ("(fn (x) (unseal x x))", |body| {
            let CExpr::Unseal(left, right) = body else {
                panic!("not unseal")
            };
            *body = CExpr::Seal(left.clone(), right.clone());
        }),
        ("(fn (x) (x 1))", |body| {
            let CExpr::App(left, right) = body else {
                panic!("not app")
            };
            std::mem::swap(left, right);
        }),
        ("(fn (x) ((x 1) 2))", |body| {
            let CExpr::AppN {
                extra_app_ticks, ..
            } = body
            else {
                panic!("not appn")
            };
            *extra_app_ticks = 0;
        }),
    ];
    for (source, mutate) in cases {
        let mut module = compile_module(&gc_coreform::parse_module(source).unwrap()).unwrap();
        let CompiledForm::Expr(expr) = &mut module.forms[0] else {
            panic!("not expr")
        };
        let CExpr::FnUnary { body, .. } = Arc::make_mut(expr) else {
            panic!("not fn")
        };
        mutate(Arc::make_mut(body));
        let error = decode_compiled_module_blob(&encode_compiled_module_blob(&module).unwrap())
            .unwrap_err();
        assert!(matches!(error.kind, KernelErrorKind::Internal), "{source}");
        assert_eq!(
            error.msg, "compiled module semantic metadata is inconsistent",
            "{source}"
        );
    }
}

#[test]
fn compiled_semantic_admission_checks_root_scope_inventory_and_tags() {
    let forms = gc_coreform::parse_module("(def current 1) (fn (arg) current)").unwrap();
    let original = compile_module(&forms).unwrap();
    let mut extra = original.clone();
    extra.module_names.push("unused".to_string());
    assert!(decode_compiled_module_blob(&encode_compiled_module_blob(&extra).unwrap()).is_err());
    let mut slot = original;
    let CompiledForm::Expr(expr) = &mut slot.forms[1] else {
        panic!("not expr")
    };
    let CExpr::FnUnary { body, .. } = Arc::make_mut(expr) else {
        panic!("not fn")
    };
    let CExpr::Var { resolution, .. } = Arc::make_mut(body) else {
        panic!("not var")
    };
    *resolution = VarResolution::Module { slot: 1 };
    assert!(decode_compiled_module_blob(&encode_compiled_module_blob(&slot).unwrap()).is_err());
    for source in ["(fn (arg) nil)", "(let ((arg 1)) arg)", "arg"] {
        let mut module = compile_module(&gc_coreform::parse_module(source).unwrap()).unwrap();
        let CompiledForm::Expr(expr) = &mut module.forms[0] else {
            panic!("not expr")
        };
        match Arc::make_mut(expr) {
            CExpr::FnUnary { param, .. } => *param = "1".to_string(),
            CExpr::Let(bindings, body) => {
                bindings[0].0 = "1".to_string();
                let CExpr::Var { name, .. } = Arc::make_mut(body) else {
                    panic!("not var")
                };
                *name = "1".to_string();
            }
            body @ CExpr::Var { .. } => *body = CExpr::Atom(Term::Symbol("arg".to_string())),
            _ => panic!("unexpected form"),
        }
        assert!(
            decode_compiled_module_blob(&encode_compiled_module_blob(&module).unwrap()).is_err(),
            "{source}"
        );
    }
}

#[test]
fn compiled_writer_rejects_unsupported_term_domain_without_changing_direct_values() {
    let invalid = [
        Term::Pair(
            Box::new(Term::Int(1.into())),
            Box::new(Term::Int(11.into())),
        ),
        Term::Vector(vec![Term::Symbol("true".to_string())]),
        Term::Map([(TermOrdKey(Term::Symbol("false".to_string())), Term::Nil)].into()),
        Term::Vector(vec![Term::Map(
            [(TermOrdKey(Term::Nil), Term::Symbol("123tail".to_string()))].into(),
        )]),
    ];
    for datum in invalid {
        let quoted = Term::list(vec![Term::Symbol("quote".to_string()), datum.clone()]);
        let compiled = compile_module(&[quoted]).unwrap();
        let value =
            eval_compiled_module(&mut EvalCtx::new(), &mut Env::empty(), &compiled).unwrap();
        assert!(matches!(value, Value::Data(ref actual) if actual.as_ref() == &datum));
        let error = encode_compiled_module_blob(&compiled).unwrap_err();
        assert!(matches!(error.kind, KernelErrorKind::BadForm));
        assert!(
            error
                .msg
                .starts_with("compiled module term is outside canonical serialization domain:")
        );
    }
    for source in [
        "(quote (pair <improper>))",
        "(quote [true])",
        "(quote {:a [1 2]})",
        "(fn (arg) (quote [true]))",
    ] {
        let compiled = compile_module(&gc_coreform::parse_module(source).unwrap()).unwrap();
        let blob = encode_compiled_module_blob(&compiled).unwrap();
        assert_eq!(
            encode_compiled_module_blob(&decode_compiled_module_blob(&blob).unwrap()).unwrap(),
            blob
        );
    }
    let raw_body = Term::list(vec![
        Term::Symbol("fn".to_string()),
        Term::list(vec![Term::Symbol("arg".to_string())]),
        Term::list(vec![
            Term::Symbol("quote".to_string()),
            Term::Symbol("true".to_string()),
        ]),
    ]);
    assert!(encode_compiled_module_blob(&compile_module(&[raw_body]).unwrap()).is_err());
}
