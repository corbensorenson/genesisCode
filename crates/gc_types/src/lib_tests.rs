use gc_coreform::{Term, canonicalize_module, parse_module};

use super::*;
use crate::infer::infer_module_types;
use crate::ty::{EffRow, RowTail};

fn extract_meta(forms: &[Term]) -> Option<Term> {
    forms.iter().find_map(|t| {
        let items = t.as_proper_list()?;
        if items.len() == 3
            && matches!(items[0], Term::Symbol(s) if s == "def")
            && matches!(items[1], Term::Symbol(s) if s == "::meta")
        {
            let q = items[2].as_proper_list()?;
            if q.len() == 2 && matches!(q[0], Term::Symbol(s) if s == "quote") {
                return Some(q[1].clone());
            }
        }
        None
    })
}

#[test]
fn infers_literal_effect_ops() {
    let src = r#"
            (def ::meta '{:exports [] :caps [sys/time::now] :types {}})
            (def x
              (core/effect::perform 'sys/time::now nil (fn (t) (core/effect::pure t))))
            x
        "#;
    let forms = canonicalize_module(parse_module(src).unwrap()).unwrap();
    let inf = infer_effects(&forms);
    assert!(inf.ops.contains("sys/time::now"));
    assert!(!inf.unknown);
}

#[test]
fn marks_unknown_when_op_is_not_literal() {
    let src = r#"
            (def ::meta '{:exports [] :caps [?] :types {}})
            (def op 'sys/time::now)
            (def x (core/effect::perform op nil (fn (t) (core/effect::pure t))))
            x
        "#;
    let forms = canonicalize_module(parse_module(src).unwrap()).unwrap();
    let inf = infer_effects(&forms);
    assert!(inf.ops.is_empty());
    assert!(inf.unknown);
}

#[test]
fn infers_caps_perform_literal_ops() {
    let src = r#"
            (def ::meta '{:exports [] :caps [editor/task::poll] :types {}})
            (def x ((core/caps::perform 'editor/task::poll) {:task-id "task-1"}))
            x
        "#;
    let forms = canonicalize_module(parse_module(src).unwrap()).unwrap();
    let inf = infer_effects(&forms);
    assert!(inf.ops.contains("editor/task::poll"));
    assert!(!inf.unknown);
}

#[test]
fn infers_task_wrapper_ops_without_inlining() {
    let src = r#"
            (def ::meta '{:exports [] :caps [core/task::await] :types {}})
            (def x (core/task::await "task-1"))
            x
        "#;
    let forms = canonicalize_module(parse_module(src).unwrap()).unwrap();
    let inf = infer_effects(&forms);
    assert!(inf.ops.contains("core/task::await"));
    assert!(!inf.unknown);
}

#[test]
fn typecheck_requires_types_for_exports() {
    let src = r#"
            (def ::meta '{:exports [m::x] :caps [] :types {}})
            (def m::x 1)
            m::x
        "#;
    let forms = canonicalize_module(parse_module(src).unwrap()).unwrap();
    let meta = extract_meta(&forms);
    let m = ModuleForTypecheck {
        path: "x.gc".to_string(),
        forms,
        meta,
    };
    let r = typecheck_package(&[m]);
    assert!(!r.ok);
    assert!(
        r.errors
            .iter()
            .any(|e| e.contains("exported symbol m::x has no type"))
    );
}

#[test]
fn contract_row_typing_accepts_declared_method() {
    let src = r#"
          (def ::meta
            '{
              :exports [pkg/t::c]
              :caps []
              :types {
                pkg/t::c
                  (Contract
                    [[foo/bar::x (Fn (Msg ?) Int (Eff [] nil))]]
                    nil)}})

          (def pkg/t::c
            (core/contract::extend
              core/contract::genesis
              {foo/bar::x (fn (m) 10)}
              {}))

          pkg/t::c
        "#;
    let forms = canonicalize_module(parse_module(src).unwrap()).unwrap();
    let meta = extract_meta(&forms);
    let m = ModuleForTypecheck {
        path: "t.gc".to_string(),
        forms,
        meta,
    };
    let r = typecheck_package(&[m]);
    assert!(r.ok, "expected ok, errors: {:?}", r.errors);
}

#[test]
fn contract_row_typing_rejects_missing_declared_method() {
    let src = r#"
          (def ::meta
            '{
              :exports [pkg/t::c]
              :caps []
              :types {
                pkg/t::c
                  (Contract
                    [[foo/bar::y (Fn (Msg ?) Int (Eff [] nil))]]
                    nil)}})

          (def pkg/t::c
            (core/contract::extend
              core/contract::genesis
              {foo/bar::x (fn (m) 10)}
              {}))

          pkg/t::c
        "#;
    let forms = canonicalize_module(parse_module(src).unwrap()).unwrap();
    let meta = extract_meta(&forms);
    let m = ModuleForTypecheck {
        path: "t.gc".to_string(),
        forms,
        meta,
    };
    let r = typecheck_package(&[m]);
    assert!(!r.ok);
    assert!(
        r.errors
            .iter()
            .any(|e| e.contains("declared type mismatch")),
        "expected declared type mismatch error, got {:?}",
        r.errors
    );
}

#[test]
fn infer_perform_returns_prog_of_continuation_prog() {
    let src = r#"
            (def ::meta '{:exports [] :caps [sys/time::now] :types {}})
            (def m::p
              (core/effect::perform
                'sys/time::now
                nil
                (fn (t) (core/effect::pure 1))))
            m::p
        "#;
    let forms = canonicalize_module(parse_module(src).unwrap()).unwrap();
    let mut sess = InferSession::default();
    let (_env, defs) = infer_module_types(&forms, &mut sess, &BTreeMap::new());
    assert!(
        sess.errors.is_empty(),
        "unexpected infer errors: {:?}",
        sess.errors
    );
    let ty = defs.get("m::p").cloned().unwrap_or(Ty::Any);
    match ty {
        Ty::Prog { ret, eff } => {
            assert_eq!(*ret, Ty::Int);
            assert!(eff.ops.contains("sys/time::now"));
            assert!(matches!(eff.tail, RowTail::Closed));
        }
        other => panic!("expected Prog, got {}", print_term(&other.to_term())),
    }
}

#[test]
fn infer_contract_extend_preserves_row_tail_var() {
    let src = r#"
          (def ::meta '{:exports [] :caps [] :types {}})
          (def m::c
            (core/contract::extend
              core/contract::genesis
              {foo/bar::x (fn (m) 10)}
              {}))
          m::c
        "#;
    let forms = canonicalize_module(parse_module(src).unwrap()).unwrap();
    let mut sess = InferSession::default();
    let (_env, defs) = infer_module_types(&forms, &mut sess, &BTreeMap::new());
    assert!(
        sess.errors.is_empty(),
        "unexpected infer errors: {:?}",
        sess.errors
    );
    let ty = defs.get("m::c").cloned().unwrap_or(Ty::Any);
    match ty {
        Ty::Contract { tail, methods } => {
            assert!(matches!(tail, RowTail::Var(ref s) if s == "r"));
            assert!(methods.contains_key("foo/bar::x"));
        }
        other => panic!("expected Contract, got {}", print_term(&other.to_term())),
    }
}

#[test]
fn infer_effect_bind_with_map_rows_returns_precise_prog_type() {
    let src = r#"
          (def ::meta '{:exports [] :caps [] :types {}})
          (def m::prog
            ((core/effect::bind
               (core/effect::pure
                 (prim map/merge
                   (prim map/put {:seed 40} ':value 41)
                   {:ok true})))
              (fn (row)
                (core/effect::pure (prim map/get row ':value)))))
          m::prog
        "#;
    let forms = canonicalize_module(parse_module(src).unwrap()).unwrap();
    let mut sess = InferSession::default();
    let (_env, defs) = infer_module_types(&forms, &mut sess, &BTreeMap::new());
    assert!(
        sess.errors.is_empty(),
        "unexpected infer errors: {:?}",
        sess.errors
    );
    let ty = defs.get("m::prog").cloned().unwrap_or(Ty::Any);
    match ty {
        Ty::Prog { ret, eff } => {
            assert_eq!(*ret, Ty::Int);
            assert!(eff.ops.is_empty());
            assert!(matches!(eff.tail, RowTail::Closed));
        }
        other => panic!("expected Prog Int, got {}", print_term(&other.to_term())),
    }
}

#[test]
fn infer_application_uses_declared_function_types() {
    let src = r#"
          (def ::meta '{:exports [] :caps [] :types {}})
          (def m::out (m::id 7))
          m::out
        "#;
    let forms = canonicalize_module(parse_module(src).unwrap()).unwrap();
    let mut sess = InferSession::default();
    let mut declared = BTreeMap::new();
    declared.insert(
        "m::id".to_string(),
        Ty::Fn {
            param: Box::new(Ty::Int),
            ret: Box::new(Ty::Int),
            eff: EffRow::empty(),
        },
    );
    let (_env, defs) = infer_module_types(&forms, &mut sess, &declared);
    assert!(
        sess.errors.is_empty(),
        "unexpected infer errors: {:?}",
        sess.errors
    );
    let ty = defs.get("m::out").cloned().unwrap_or(Ty::Any);
    assert_eq!(ty, Ty::Int);
}

#[test]
fn numeric_profile_types_are_precise() {
    let dec = parse_type_term(&Term::symbol("Dec")).expect("parse Dec");
    assert_eq!(dec, Ty::Dec);
    assert_eq!(dec.to_term(), Term::symbol("Dec"));

    let forms = canonicalize_module(
        parse_module(
            r#"
              (def m::decimal (prim dec/parse "1.25"))
              (def m::rendered (prim dec/to-str m::decimal))
              (def m::quotient (prim int/div 7 3))
              (def m::remainder (prim int/mod -7 3))
              m::rendered
            "#,
        )
        .expect("parse"),
    )
    .expect("canonicalize");
    let mut session = InferSession::default();
    let (_env, defs) = infer_module_types(&forms, &mut session, &BTreeMap::new());
    assert!(
        session.errors.is_empty(),
        "unexpected errors: {:?}",
        session.errors
    );
    assert_eq!(defs.get("m::decimal"), Some(&Ty::Dec));
    assert_eq!(defs.get("m::rendered"), Some(&Ty::Str));
    assert_eq!(defs.get("m::quotient"), Some(&Ty::Int));
    assert_eq!(defs.get("m::remainder"), Some(&Ty::Int));

    let invalid = canonicalize_module(
        parse_module("(def m::bad (prim dec/add (prim dec/parse \"1\") 2))")
            .expect("parse invalid"),
    )
    .expect("canonicalize invalid");
    let mut invalid_session = InferSession::default();
    infer_module_types(&invalid, &mut invalid_session, &BTreeMap::new());
    assert_eq!(invalid_session.errors, ["prim dec/add expects Dec, Dec"]);
}

#[test]
fn strict_effects_reject_unknown_effect_ops() {
    let src = r#"
          (def ::meta
            '{
              :exports [m::x]
              :caps [core/task::spawn]
              :strict-effects true
              :types {m::x ?}})
          (def m::op 'core/task::spawn)
          (def m::x
            (core/effect::perform m::op {:payload 1} (fn (resp) (core/effect::pure resp))))
          m::x
        "#;
    let forms = canonicalize_module(parse_module(src).unwrap()).unwrap();
    let m = ModuleForTypecheck {
        path: "strict.gc".to_string(),
        meta: extract_meta(&forms),
        forms,
    };
    let r = typecheck_package(&[m]);
    assert!(!r.ok);
    assert!(
        r.errors
            .iter()
            .any(|e| e.contains("strict effect mode forbids unknown effect ops")),
        "expected strict unknown-op error, got {:?}",
        r.errors
    );
}

#[test]
fn strict_effects_require_closed_declared_row_for_exports() {
    let src = r#"
          (def ::meta
            '{
              :exports [m::x]
              :caps [core/task::await]
              :strict-effects true
              :types {m::x (Prog ? (Eff [core/task::await] ?))}})
          (def m::x
            (core/effect::perform
              'core/task::await
              {:task-id "task-1"}
              (fn (resp) (core/effect::pure resp))))
          m::x
        "#;
    let forms = canonicalize_module(parse_module(src).unwrap()).unwrap();
    let m = ModuleForTypecheck {
        path: "strict-row.gc".to_string(),
        meta: extract_meta(&forms),
        forms,
    };
    let r = typecheck_package(&[m]);
    assert!(!r.ok);
    assert!(
        r.errors
            .iter()
            .any(|e| e.contains("strict effect mode requires a closed declared effect row")),
        "expected strict closed-row error, got {:?}",
        r.errors
    );
}

#[test]
fn strict_shapes_reject_unresolved_contract_op_signatures() {
    let src = r#"
          (def ::meta
            '{
              :exports [m::send]
              :caps []
              :strict-shapes true
              :types {m::send (Fn Symbol (Msg Int) (Eff [] nil))}})
          (def m::send (fn (op) (core/msg::make op 1)))
          m::send
        "#;
    let forms = canonicalize_module(parse_module(src).unwrap()).unwrap();
    let m = ModuleForTypecheck {
        path: "strict-shapes.gc".to_string(),
        meta: extract_meta(&forms),
        forms,
    };
    let r = typecheck_package(&[m]);
    assert!(!r.ok);
    assert!(
        r.errors
            .iter()
            .any(|e| e.contains("strict shape mode forbids unresolved contract op signatures")),
        "expected unresolved contract-op signature error, got {:?}",
        r.errors
    );
}

#[test]
fn task_wrapper_inference_tracks_spawn_wrappers_and_pure_dsl_helpers() {
    let src = r#"
          (def ::meta '{:exports [] :caps [core/task::spawn] :types {}})
          (def m::prog (core/task::program []))
          (def m::prog2 (core/task::program-with-initial 0 []))
          (def m::step (core/task::step/map-put ':k 42))
          (def m::spawn (core/task::spawn-evaln 'scope 'job '(fn (args) args) [1 2 3]))
          m::spawn
        "#;
    let forms = canonicalize_module(parse_module(src).unwrap()).unwrap();
    let inf = infer_effects(&forms);
    assert!(inf.ops.contains("core/task::spawn"));
    assert!(!inf.unknown, "unexpected unknown effect ops: {:?}", inf.ops);
}

#[test]
fn strict_shapes_reject_open_inferred_contract_for_closed_declared_contract() {
    let src = r#"
          (def ::meta
            '{
              :exports [pkg/t::c]
              :strict-shapes true
              :caps []
              :types {
                pkg/t::c
                  (Contract
                    [[foo/bar::x (Fn (Msg ?) Int (Eff [] nil))]]
                    nil)}})

          (def pkg/t::c
            (core/contract::extend
              core/contract::genesis
              {foo/bar::x (fn (m) 10)}
              {}))

          pkg/t::c
        "#;
    let forms = canonicalize_module(parse_module(src).unwrap()).unwrap();
    let meta = extract_meta(&forms);
    let m = ModuleForTypecheck {
        path: "strict-shapes-contract.gc".to_string(),
        forms,
        meta,
    };
    let r = typecheck_package(&[m]);
    assert!(!r.ok);
    assert!(
        r.errors
            .iter()
            .any(|e| e.contains("declared type mismatch")),
        "expected declared type mismatch error, got {:?}",
        r.errors
    );
}

#[test]
fn strict_shapes_accept_open_declared_contract_tail() {
    let src = r#"
          (def ::meta
            '{
              :exports [pkg/t::c]
              :strict-shapes true
              :caps []
              :types {
                pkg/t::c
                  (Contract
                    [[foo/bar::x (Fn (Msg ?) Int (Eff [] nil))]]
                    r)}})

          (def pkg/t::c
            (core/contract::extend
              core/contract::genesis
              {foo/bar::x (fn (m) 10)}
              {}))

          pkg/t::c
        "#;
    let forms = canonicalize_module(parse_module(src).unwrap()).unwrap();
    let meta = extract_meta(&forms);
    let m = ModuleForTypecheck {
        path: "strict-shapes-contract-open.gc".to_string(),
        forms,
        meta,
    };
    let r = typecheck_package(&[m]);
    assert!(
        r.ok,
        "expected open-tail contract to pass, got {:?}",
        r.errors
    );
}

#[test]
fn strict_shapes_reject_extra_record_fields_for_closed_declared_record() {
    let src = r#"
          (def ::meta
            '{
              :exports [m::row]
              :strict-shapes true
              :caps []
              :types {m::row (Rec [[:a Int]] nil)}})
          (def m::row {:a 1 :b 2})
          m::row
        "#;
    let forms = canonicalize_module(parse_module(src).unwrap()).unwrap();
    let m = ModuleForTypecheck {
        path: "strict-shapes-record.gc".to_string(),
        meta: extract_meta(&forms),
        forms,
    };
    let r = typecheck_package(&[m]);
    assert!(!r.ok);
    assert!(
        r.errors
            .iter()
            .any(|e| e.contains("declared type mismatch")),
        "expected strict closed-row mismatch, got {:?}",
        r.errors
    );
}

#[test]
fn repeated_effect_row_variable_requires_one_consistent_binding() {
    let inferred = Ty::Fn {
        param: Box::new(Ty::Prog {
            ret: Box::new(Ty::Int),
            eff: EffRow {
                ops: BTreeSet::from(["sys/time::now".to_string()]),
                tail: RowTail::Closed,
            },
        }),
        ret: Box::new(Ty::Prog {
            ret: Box::new(Ty::Int),
            eff: EffRow {
                ops: BTreeSet::from(["sys/random::bytes".to_string()]),
                tail: RowTail::Closed,
            },
        }),
        eff: EffRow::empty(),
    };
    let declared = Ty::Fn {
        param: Box::new(Ty::Prog {
            ret: Box::new(Ty::Int),
            eff: EffRow {
                ops: BTreeSet::new(),
                tail: RowTail::Var("e".to_string()),
            },
        }),
        ret: Box::new(Ty::Prog {
            ret: Box::new(Ty::Int),
            eff: EffRow {
                ops: BTreeSet::new(),
                tail: RowTail::Var("e".to_string()),
            },
        }),
        eff: EffRow::empty(),
    };

    assert!(
        !type_compatible(&inferred, &declared, false),
        "one row variable must not match two different effect remainders"
    );
}

#[test]
fn effect_row_variable_is_instantiated_per_application() {
    let src = r#"
          (def ::meta '{:exports [] :caps [sys/time::now] :types {}})
          (def m::out
            (m::carry
              (core/effect::perform
                'sys/time::now
                nil
                (fn (_) (core/effect::pure 1)))))
          m::out
        "#;
    let forms = canonicalize_module(parse_module(src).unwrap()).unwrap();
    let mut declared = BTreeMap::new();
    declared.insert(
        "m::carry".to_string(),
        Ty::Fn {
            param: Box::new(Ty::Prog {
                ret: Box::new(Ty::Any),
                eff: EffRow {
                    ops: BTreeSet::new(),
                    tail: RowTail::Var("e".to_string()),
                },
            }),
            ret: Box::new(Ty::Prog {
                ret: Box::new(Ty::Any),
                eff: EffRow {
                    ops: BTreeSet::new(),
                    tail: RowTail::Var("e".to_string()),
                },
            }),
            eff: EffRow::empty(),
        },
    );
    let mut sess = InferSession::default();
    let (_env, defs) = infer_module_types(&forms, &mut sess, &declared);
    assert!(
        sess.errors.is_empty(),
        "unexpected errors: {:?}",
        sess.errors
    );

    let Ty::Prog { eff, .. } = defs.get("m::out").expect("m::out inferred") else {
        panic!("m::out must infer as Prog")
    };
    assert_eq!(
        eff,
        &EffRow {
            ops: BTreeSet::from(["sys/time::now".to_string()]),
            tail: RowTail::Closed,
        },
        "the call result must contain the argument row, not the declaration variable"
    );
}

#[test]
fn unknown_argument_instantiates_effect_row_variable_as_unknown() {
    let src = r#"
          (def ::meta '{:exports [] :caps [?] :types {}})
          (def m::out (m::carry m::unknown))
          m::out
        "#;
    let forms = canonicalize_module(parse_module(src).unwrap()).unwrap();
    let mut declared = BTreeMap::new();
    declared.insert(
        "m::carry".to_string(),
        Ty::Fn {
            param: Box::new(Ty::Prog {
                ret: Box::new(Ty::Any),
                eff: EffRow {
                    ops: BTreeSet::new(),
                    tail: RowTail::Var("e".to_string()),
                },
            }),
            ret: Box::new(Ty::Prog {
                ret: Box::new(Ty::Any),
                eff: EffRow {
                    ops: BTreeSet::new(),
                    tail: RowTail::Var("e".to_string()),
                },
            }),
            eff: EffRow::empty(),
        },
    );
    let mut sess = InferSession::default();
    let (_env, defs) = infer_module_types(&forms, &mut sess, &declared);
    assert!(
        sess.errors.is_empty(),
        "unexpected errors: {:?}",
        sess.errors
    );

    let Ty::Prog { eff, .. } = defs.get("m::out").expect("m::out inferred") else {
        panic!("m::out must infer as Prog")
    };
    assert_eq!(
        eff.tail,
        RowTail::Any,
        "an unknown argument must not leave a symbolic row variable in the result"
    );
}

#[test]
fn unknown_and_concrete_repetitions_widen_one_effect_row_binding() {
    let src = r#"
          (def ::meta '{:exports [] :caps [] :types {}})
          (def m::out (m::accept m::mixed))
          m::out
        "#;
    let forms = canonicalize_module(parse_module(src).unwrap()).unwrap();
    let repeated = EffRow {
        ops: BTreeSet::new(),
        tail: RowTail::Var("e".to_string()),
    };
    let mut declared = BTreeMap::new();
    declared.insert(
        "m::accept".to_string(),
        Ty::Fn {
            param: Box::new(Ty::Fn {
                param: Box::new(Ty::Prog {
                    ret: Box::new(Ty::Int),
                    eff: repeated.clone(),
                }),
                ret: Box::new(Ty::Prog {
                    ret: Box::new(Ty::Int),
                    eff: repeated,
                }),
                eff: EffRow::empty(),
            }),
            ret: Box::new(Ty::Int),
            eff: EffRow::empty(),
        },
    );
    declared.insert(
        "m::mixed".to_string(),
        Ty::Fn {
            param: Box::new(Ty::Prog {
                ret: Box::new(Ty::Int),
                eff: EffRow {
                    ops: BTreeSet::new(),
                    tail: RowTail::Any,
                },
            }),
            ret: Box::new(Ty::Prog {
                ret: Box::new(Ty::Int),
                eff: EffRow {
                    ops: BTreeSet::from(["sys/time::now".to_string()]),
                    tail: RowTail::Closed,
                },
            }),
            eff: EffRow::empty(),
        },
    );
    let mut sess = InferSession::default();
    let (_env, defs) = infer_module_types(&forms, &mut sess, &declared);

    assert!(
        sess.errors.is_empty(),
        "unknown consistency must widen rather than reject: {:?}",
        sess.errors
    );
    assert_eq!(defs.get("m::out"), Some(&Ty::Int));
}

#[test]
fn duplicate_effect_operations_are_rejected() {
    let forms = parse_module("(Fn Int Int (Eff [sys/time::now sys/time::now] nil))").unwrap();
    let error = parse_type_term(&forms[0]).expect_err("duplicate effect op must fail");
    assert_eq!(error, "duplicate effect op sys/time::now");
}

#[test]
fn strict_package_boundary_propagates_imported_effects() {
    let provider_src = r#"
          (def ::meta
            '{:exports [pkg/a::clock]
              :caps [sys/time::now]
              :strict-effects true
              :types {
                pkg/a::clock
                  (Fn Nil (Prog Int (Eff [sys/time::now] nil))
                    (Eff [sys/time::now] nil))}})
          (def pkg/a::clock
            (fn (_)
              (core/effect::perform
                'sys/time::now nil (fn (_) (core/effect::pure 1)))))
          pkg/a::clock
        "#;
    let consumer_src = r#"
          (def ::meta
            '{:exports [pkg/b::main]
              :caps []
              :strict-effects true
              :types {pkg/b::main ?}})
          (def pkg/b::main (pkg/a::clock nil))
          pkg/b::main
        "#;
    let provider_forms = canonicalize_module(parse_module(provider_src).unwrap()).unwrap();
    let consumer_forms = canonicalize_module(parse_module(consumer_src).unwrap()).unwrap();
    let report = typecheck_package(&[
        ModuleForTypecheck {
            path: "a.gc".to_string(),
            meta: extract_meta(&provider_forms),
            forms: provider_forms,
        },
        ModuleForTypecheck {
            path: "b.gc".to_string(),
            meta: extract_meta(&consumer_forms),
            forms: consumer_forms,
        },
    ]);

    assert!(!report.ok, "consumer must not hide a provider effect");
    assert!(
        report
            .errors
            .iter()
            .any(|error| error.contains("pkg/b::main") && error.contains("sys/time::now")),
        "expected an imported-effect diagnostic, got {:?}",
        report.errors
    );
}

#[test]
fn strict_package_boundary_rejects_unknown_imported_signature() {
    let provider_src = r#"
          (def ::meta '{:exports [pkg/a::opaque] :caps [] :types {pkg/a::opaque ?}})
          (def pkg/a::opaque (fn (_) 1))
          pkg/a::opaque
        "#;
    let consumer_src = r#"
          (def ::meta
            '{:exports [pkg/b::main]
              :caps []
              :strict-effects true
              :types {pkg/b::main ?}})
          (def pkg/b::main (pkg/a::opaque nil))
          pkg/b::main
        "#;
    let provider_forms = canonicalize_module(parse_module(provider_src).unwrap()).unwrap();
    let consumer_forms = canonicalize_module(parse_module(consumer_src).unwrap()).unwrap();
    let report = typecheck_package(&[
        ModuleForTypecheck {
            path: "a.gc".to_string(),
            meta: extract_meta(&provider_forms),
            forms: provider_forms,
        },
        ModuleForTypecheck {
            path: "b.gc".to_string(),
            meta: extract_meta(&consumer_forms),
            forms: consumer_forms,
        },
    ]);

    assert!(
        !report.ok,
        "strict consumers require a concrete imported signature"
    );
    assert!(
        report.errors.iter().any(|error| {
            error.contains("pkg/b::main") && error.contains("unknown imported effect signature")
        }),
        "expected an unknown-import diagnostic, got {:?}",
        report.errors
    );
}

#[test]
fn strict_package_boundary_respects_parameter_shadowing() {
    let provider_src = r#"
          (def ::meta '{:exports [pkg/a::opaque] :caps [] :types {pkg/a::opaque ?}})
          (def pkg/a::opaque (fn (_) 1))
          pkg/a::opaque
        "#;
    let consumer_src = r#"
          (def ::meta
            '{:exports [pkg/b::main]
              :caps []
              :strict-effects true
              :types {
                pkg/b::main
                  (Fn (Fn Nil Int (Eff [] nil)) Int (Eff [] nil))}})
          (def pkg/b::main
            (fn (pkg/a::opaque) (pkg/a::opaque nil)))
          pkg/b::main
        "#;
    let provider_forms = canonicalize_module(parse_module(provider_src).unwrap()).unwrap();
    let consumer_forms = canonicalize_module(parse_module(consumer_src).unwrap()).unwrap();
    let report = typecheck_package(&[
        ModuleForTypecheck {
            path: "a.gc".to_string(),
            meta: extract_meta(&provider_forms),
            forms: provider_forms,
        },
        ModuleForTypecheck {
            path: "b.gc".to_string(),
            meta: extract_meta(&consumer_forms),
            forms: consumer_forms,
        },
    ]);

    assert!(
        report.ok,
        "a local parameter must shadow the imported unknown signature: {:?}",
        report.errors
    );
}

#[test]
fn package_effect_inference_respects_let_shadowing() {
    let provider_src = r#"
          (def ::meta
            '{:exports [pkg/a::clock]
              :caps [sys/time::now]
              :strict-effects true
              :types {
                pkg/a::clock
                  (Fn Nil Int (Eff [sys/time::now] nil))}})
          (def pkg/a::clock (fn (_) 1))
          pkg/a::clock
        "#;
    let consumer_src = r#"
          (def ::meta
            '{:exports [pkg/b::main]
              :caps []
              :strict-effects true
              :types {pkg/b::main Int}})
          (def pkg/b::main
            (let ((pkg/a::clock (fn (_) 1)))
              (pkg/a::clock nil)))
          pkg/b::main
        "#;
    let provider_forms = canonicalize_module(parse_module(provider_src).unwrap()).unwrap();
    let consumer_forms = canonicalize_module(parse_module(consumer_src).unwrap()).unwrap();
    let report = typecheck_package(&[
        ModuleForTypecheck {
            path: "a.gc".to_string(),
            meta: extract_meta(&provider_forms),
            forms: provider_forms,
        },
        ModuleForTypecheck {
            path: "b.gc".to_string(),
            meta: extract_meta(&consumer_forms),
            forms: consumer_forms,
        },
    ]);

    assert!(
        report.ok,
        "a local let binding must shadow the imported effectful signature: {:?}",
        report.errors
    );
}

#[test]
fn package_rejects_duplicate_export_ownership() {
    fn module(path: &str) -> ModuleForTypecheck {
        let src = r#"
              (def ::meta '{:exports [pkg/shared::x] :caps [] :types {pkg/shared::x Int}})
              (def pkg/shared::x 1)
              pkg/shared::x
            "#;
        let forms = canonicalize_module(parse_module(src).unwrap()).unwrap();
        ModuleForTypecheck {
            path: path.to_string(),
            meta: extract_meta(&forms),
            forms,
        }
    }

    let report = typecheck_package(&[module("a.gc"), module("b.gc")]);
    assert!(!report.ok);
    assert!(
        report.errors.iter().any(|error| {
            error.contains("duplicate export pkg/shared::x")
                && error.contains("a.gc")
                && error.contains("b.gc")
        }),
        "expected deterministic duplicate ownership diagnostic, got {:?}",
        report.errors
    );
}

#[test]
fn strict_effects_accept_parameter_bound_named_row_variable() {
    let src = r#"
          (def ::meta
            '{:exports [pkg/poly::carry]
              :caps []
              :strict-effects true
              :types {
                pkg/poly::carry
                  (Fn
                    (Prog Int (Eff [] e))
                    (Prog Int (Eff [] e))
                    (Eff [] nil))}})
          (def pkg/poly::carry (fn (program) program))
          pkg/poly::carry
        "#;
    let forms = canonicalize_module(parse_module(src).unwrap()).unwrap();
    let report = typecheck_package(&[ModuleForTypecheck {
        path: "poly.gc".to_string(),
        meta: extract_meta(&forms),
        forms,
    }]);

    assert!(
        report.ok,
        "parameter-bound row polymorphism must remain strict and capability-neutral: {:?}",
        report.errors
    );
}

#[test]
fn effect_row_variable_cannot_escape_without_parameter_binding() {
    let src = r#"
          (def ::meta
            '{:exports [pkg/poly::bad]
              :caps []
              :types {
                pkg/poly::bad
                  (Fn Int (Prog Int (Eff [] e)) (Eff [] nil))}})
          (def pkg/poly::bad (fn (_) (core/effect::pure 1)))
          pkg/poly::bad
        "#;
    let forms = canonicalize_module(parse_module(src).unwrap()).unwrap();
    let report = typecheck_package(&[ModuleForTypecheck {
        path: "bad-poly.gc".to_string(),
        meta: extract_meta(&forms),
        forms,
    }]);

    assert!(!report.ok);
    assert!(
        report.errors.iter().any(|error| {
            error.contains("pkg/poly::bad") && error.contains("unbound effect row variable(s) e")
        }),
        "expected an escaping-row diagnostic, got {:?}",
        report.errors
    );
}

#[test]
fn nested_return_function_cannot_introduce_effect_row_variable() {
    let src = r#"
          (def ::meta
            '{:exports [pkg/poly::nested]
              :caps []
              :types {
                pkg/poly::nested
                  (Fn Int
                    (Fn
                      (Prog Int (Eff [] e))
                      (Prog Int (Eff [] e))
                      (Eff [] nil))
                    (Eff [] nil))}})
          (def pkg/poly::nested (fn (_) (fn (program) program)))
          pkg/poly::nested
        "#;
    let forms = canonicalize_module(parse_module(src).unwrap()).unwrap();
    let report = typecheck_package(&[ModuleForTypecheck {
        path: "nested-poly.gc".to_string(),
        meta: extract_meta(&forms),
        forms,
    }]);

    assert!(!report.ok);
    assert!(
        report.errors.iter().any(|error| {
            error.contains("pkg/poly::nested")
                && error.contains("unbound effect row variable(s) e")
                && error.contains("outermost function parameter")
        }),
        "expected a rank-1 scope diagnostic, got {:?}",
        report.errors
    );
}

#[test]
fn standalone_contract_method_cannot_introduce_effect_row_variable() {
    let src = r#"
          (def ::meta
            '{:exports [pkg/poly::contract]
              :caps []
              :types {
                pkg/poly::contract
                  (Contract
                    [[pkg/poly::run
                      (Fn
                        (Prog Int (Eff [] e))
                        (Prog Int (Eff [] e))
                        (Eff [] nil))]]
                    nil)}})
          (def pkg/poly::contract
            (core/contract::extend
              core/contract::genesis
              {pkg/poly::run (fn (program) program)}
              {}))
          pkg/poly::contract
        "#;
    let forms = canonicalize_module(parse_module(src).unwrap()).unwrap();
    let report = typecheck_package(&[ModuleForTypecheck {
        path: "contract-poly.gc".to_string(),
        meta: extract_meta(&forms),
        forms,
    }]);

    assert!(!report.ok);
    assert!(
        report.errors.iter().any(|error| {
            error.contains("pkg/poly::contract")
                && error.contains("unbound effect row variable(s) e")
        }),
        "expected unsupported per-method polymorphism to fail, got {:?}",
        report.errors
    );
}

#[test]
fn unicode_text_primitives_infer_precise_types() {
    let src = r#"
          (def m::scalar-count (prim str/scalar-len "é"))
          (def m::grapheme-count (prim str/grapheme-len "é"))
          (def m::slice (prim str/grapheme-slice "a👩‍👩‍👧‍👦z" 1 1))
          (def m::normalized (prim str/nfc "é"))
          m::normalized
        "#;
    let forms = canonicalize_module(parse_module(src).unwrap()).unwrap();
    let mut sess = InferSession::default();
    let (_env, defs) = infer_module_types(&forms, &mut sess, &BTreeMap::new());

    assert!(
        sess.errors.is_empty(),
        "unexpected errors: {:?}",
        sess.errors
    );
    assert_eq!(defs.get("m::scalar-count"), Some(&Ty::Int));
    assert_eq!(defs.get("m::grapheme-count"), Some(&Ty::Int));
    assert_eq!(defs.get("m::slice"), Some(&Ty::Str));
    assert_eq!(defs.get("m::normalized"), Some(&Ty::Str));
}

#[test]
fn unicode_text_primitives_reject_wrong_arity_and_types() {
    let src = r#"
          (def m::bad-scalar (prim str/scalar-len 1))
          (def m::bad-grapheme (prim str/grapheme-len "a" "b"))
          (def m::bad-slice (prim str/grapheme-slice "abc" 0 "1"))
          (def m::bad-nfc (prim str/nfc false))
          m::bad-nfc
        "#;
    let forms = canonicalize_module(parse_module(src).unwrap()).unwrap();
    let mut sess = InferSession::default();
    let _ = infer_module_types(&forms, &mut sess, &BTreeMap::new());

    for expected in [
        "prim str/scalar-len expects Str",
        "prim str/grapheme-len expects 1 arg, got 2",
        "prim str/grapheme-slice expects Str, Int, Int",
        "prim str/nfc expects Str",
    ] {
        assert!(
            sess.errors.iter().any(|error| error == expected),
            "missing {expected:?} in {:?}",
            sess.errors
        );
    }
}
