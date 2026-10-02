//! Explicit source parity harness; never a production frontend fallback.
use gc_coreform::{Term, TermOrdKey, parse_module, validate_canonical_term};
use gc_kernel::{Env, EvalCtx, Value, eval_module, eval_module_compiled};
use gc_prelude::build_prelude;
use std::collections::BTreeMap;

fn printer_context(compiled: bool) -> (EvalCtx, Env) {
    let mut context = EvalCtx::with_step_limit(Some(2_000_000));
    let mut env = build_prelude(&mut context).env;
    let source = [
        include_str!("../../../selfhost/printer/00_core_single_line.gc"),
        include_str!("../../../selfhost/printer/01_single_line_list.gc"),
        include_str!("../../../selfhost/printer/02_fmt_structured.gc"),
        include_str!("../../../selfhost/printer/03_fmt_list_module.gc"),
        include_str!("../../../selfhost/hash.gc"),
    ]
    .join("\n");
    let forms = parse_module(&source).unwrap();
    if compiled {
        eval_module_compiled(&mut context, &mut env, &forms)
    } else {
        eval_module(&mut context, &mut env, &forms)
    }
    .unwrap();
    context.reset_counters();
    (context, env)
}

#[test]
fn selfhost_printer_matches_checked_canonical_domain_for_all_container_edges() {
    let proper = gc_coreform::parse_term("(pair <improper>)").unwrap();
    let improper = Term::Pair(Box::new(Term::Int(1.into())), Box::new(Term::Int(2.into())));
    let invalid_symbol = Term::Symbol("true".into());
    let mut bad_key = BTreeMap::new();
    bad_key.insert(TermOrdKey(invalid_symbol.clone()), Term::Nil);
    let mut bad_value = BTreeMap::new();
    bad_value.insert(TermOrdKey(Term::symbol(":key")), improper.clone());
    let cases = [
        proper,
        improper.clone(),
        invalid_symbol.clone(),
        Term::list(vec![improper.clone()]),
        Term::Vector(vec![invalid_symbol]),
        Term::Map(bad_key),
        Term::Map(bad_value),
    ];
    for compiled in [false, true] {
        let (mut context, mut env) = printer_context(compiled);
        let error_token = context.protocol.unwrap().error;
        let forms = parse_module("(selfhost/printer::print-term admission/input)").unwrap();
        let hash_forms = parse_module("(selfhost/hash::hash-term admission/input)").unwrap();
        for term in &cases {
            env.set_local("admission/input", Value::data(term.clone()));
            context.reset_counters();
            let result = if compiled {
                eval_module_compiled(&mut context, &mut env, &forms)
            } else {
                eval_module(&mut context, &mut env, &forms)
            }
            .unwrap();
            if validate_canonical_term(term).is_ok() {
                assert!(
                    matches!(result.as_data(), Some(Term::Str(text)) if text == &gc_coreform::print_term_checked(term).unwrap())
                );
            } else {
                assert!(
                    matches!(result, Value::Sealed { token, .. } if token == error_token),
                    "compiled={compiled}: {result:?}"
                );
            }
            context.reset_counters();
            let result = if compiled {
                eval_module_compiled(&mut context, &mut env, &hash_forms)
            } else {
                eval_module(&mut context, &mut env, &hash_forms)
            }
            .unwrap();
            if let Ok(hash) = gc_coreform::hash_term_checked(term) {
                let expected = hash
                    .iter()
                    .map(|byte| format!("{byte:02x}"))
                    .collect::<String>();
                assert!(matches!(result.as_data(), Some(Term::Str(text)) if text == &expected));
            } else {
                assert!(matches!(result, Value::Sealed { token, .. } if token == error_token));
            }
        }
    }
}

#[test]
fn effect_boundary_rejects_noncanonical_payload_before_policy_or_identity() {
    let workspace = tempfile::tempdir().unwrap();
    let caps = workspace.path().join("caps.toml");
    std::fs::write(&caps, "allow = []\n[store]\ndir = './store'\n").unwrap();
    let policy = gc_effects::CapsPolicy::load(&caps).unwrap();
    let improper = Term::Pair(Box::new(Term::Int(1.into())), Box::new(Term::Int(2.into())));
    let mut bad_key = BTreeMap::new();
    bad_key.insert(TermOrdKey(Term::Symbol("true".into())), Term::Nil);
    let forms = parse_module(
        "(core/effect::perform 'core/store::put {:artifact admission/input} (fn (r) (core/effect::pure r)))",
    ).unwrap();
    for compiled in [false, true] {
        for input in [
            improper.clone(),
            Term::Symbol("true".into()),
            Term::list(vec![improper.clone()]),
            Term::Vector(vec![improper.clone()]),
            Term::Map(bad_key.clone()),
        ] {
            let mut context = EvalCtx::with_step_limit(Some(1000));
            let mut env = build_prelude(&mut context).env;
            env.set_local("admission/input", Value::data(input));
            let program = if compiled {
                eval_module_compiled(&mut context, &mut env, &forms)
            } else {
                eval_module(&mut context, &mut env, &forms)
            }
            .unwrap();
            let result = gc_effects::run(
                &mut context,
                &policy,
                program,
                gc_coreform::hash_module(&forms),
                "admission-test".into(),
            );
            assert!(
                matches!(result, Err(gc_effects::EffectsError::BadPayload(_))),
                "compiled={compiled}"
            );
            assert!(
                std::fs::read_dir(workspace.path().join("store"))
                    .unwrap()
                    .next()
                    .is_none()
            );
        }
    }
}

fn effect_program(forms: &[Term], input: Term, compiled: bool) -> (EvalCtx, Value) {
    let mut context = EvalCtx::with_step_limit(Some(2000));
    let mut env = build_prelude(&mut context).env;
    env.set_local("admission/input", Value::data(input));
    context.reset_counters();
    let program = if compiled {
        eval_module_compiled(&mut context, &mut env, forms)
    } else {
        eval_module(&mut context, &mut env, forms)
    }
    .unwrap();
    (context, program)
}

#[test]
fn admitted_artifact_roundtrips_store_and_strict_replay_in_both_tiers() {
    use gc_effects::{CapsPolicy, EffectsError, replay_with_selfhost_authority, run};
    use gc_prelude::SelfhostBootstrapMode;
    let artifact = std::env::var_os("GENESIS_TEST_SELFHOST_ARTIFACT")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| {
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../selfhost/toolchain.gc")
        });
    let workspace = tempfile::tempdir().unwrap();
    let caps = workspace.path().join("caps.toml");
    std::fs::write(
        &caps,
        "allow = ['core/store::put', 'core/store::get']\n[store]\ndir = './store'\n",
    )
    .unwrap();
    let policy = CapsPolicy::load_with_selfhost_authority(
        &caps,
        SelfhostBootstrapMode::ArtifactOnly,
        Some(&artifact),
    )
    .unwrap();
    // This is valid, historical v0.2 data. It must never be confused with the
    // old diagnostic placeholder for an improper runtime pair.
    let input = gc_coreform::parse_term("{:data [(pair <improper>) true (a . b)]}").unwrap();
    let bytes = gc_coreform::print_term_checked(&input)
        .unwrap()
        .into_bytes();
    let hash = blake3::hash(&bytes).to_hex().to_string();
    let put_forms = parse_module(
        "(core/effect::perform 'core/store::put {:artifact admission/input} (fn (r) (core/effect::pure r)))",
    ).unwrap();
    let get_forms = parse_module(&format!(
        "(core/effect::perform 'core/store::get {{:hash \"{hash}\"}} (fn (r) (core/effect::pure r)))",
    )).unwrap();
    let mut previous_continuation = None;
    for compiled in [false, true] {
        let (mut context, program) = effect_program(&put_forms, input.clone(), compiled);
        let put = run(
            &mut context,
            &policy,
            program,
            gc_coreform::hash_module(&put_forms),
            "admission-test".into(),
        )
        .unwrap();
        let Some(Term::Map(result)) = put.value.as_data() else {
            panic!("store put failed");
        };
        assert_eq!(
            result.get(&TermOrdKey(Term::symbol(":hash"))),
            Some(&Term::Str(hash.clone()))
        );
        assert_eq!(
            std::fs::read(workspace.path().join("store").join(&hash)).unwrap(),
            bytes
        );
        if let Some(previous) = previous_continuation {
            assert_eq!(
                put.log.entries[0].cont_h, previous,
                "continuation identity must agree across tiers"
            );
        }
        previous_continuation = Some(put.log.entries[0].cont_h);

        let (mut context, program) = effect_program(&get_forms, Term::Nil, compiled);
        let get = run(
            &mut context,
            &policy,
            program,
            gc_coreform::hash_module(&get_forms),
            "admission-test".into(),
        )
        .unwrap();
        let Some(Term::Map(result)) = get.value.as_data() else {
            panic!("store get failed");
        };
        assert_eq!(
            result.get(&TermOrdKey(Term::symbol(":artifact"))),
            Some(&input)
        );
        let (mut context, program) = effect_program(&get_forms, Term::Nil, compiled);
        let replayed = replay_with_selfhost_authority(
            &mut context,
            program,
            &get.log,
            None,
            get.log.program_hash,
            SelfhostBootstrapMode::ArtifactOnly,
            Some(&artifact),
        )
        .unwrap();
        assert_eq!(replayed.as_data(), get.value.as_data());

        let improper = Term::Pair(Box::new(Term::Int(1.into())), Box::new(Term::Int(2.into())));
        let (mut context, program) = effect_program(&put_forms, improper.clone(), compiled);
        assert!(matches!(
            run(
                &mut context,
                &policy,
                program,
                put.log.program_hash,
                "admission-test".into()
            ),
            Err(EffectsError::BadPayload(_))
        ));
        let (mut context, program) = effect_program(&put_forms, improper, compiled);
        assert!(matches!(
            replay_with_selfhost_authority(
                &mut context,
                program,
                &put.log,
                None,
                put.log.program_hash,
                SelfhostBootstrapMode::ArtifactOnly,
                Some(&artifact),
            ),
            Err(EffectsError::BadPayload(_))
        ));
        assert_eq!(
            std::fs::read_dir(workspace.path().join("store"))
                .unwrap()
                .count(),
            1
        );
        assert_eq!(
            std::fs::read(workspace.path().join("store").join(&hash)).unwrap(),
            bytes
        );
    }
}
