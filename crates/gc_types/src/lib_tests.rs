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
fn strict_effects_require_closed_declared_row_for_task_exports() {
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
        r.errors.iter().any(|e| e.contains(
            "strict effect mode requires a closed declared effect row for concurrent task exports"
        )),
        "expected strict closed-row error, got {:?}",
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
