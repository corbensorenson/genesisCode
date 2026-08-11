use std::collections::BTreeSet;
use std::path::Path;

use gc_coreform::{Term, TermOrdKey, canonicalize_module, hash_term, parse_module, print_module};
use gc_types::ModuleForTypecheck;
use gc_types::profile_migration::{
    MAX_MIGRATION_INTENT_BYTES, MIGRATION_PATCH_PROFILE_ID, MIGRATION_PATCH_VERSION,
    MIGRATION_PROFILE_ID, MigrationError, MigrationPlan, MigrationProvenance, MigrationStep,
    dry_run_migration, migration_package_identity,
};
use serde_json::Value as JsonValue;
use sha2::{Digest, Sha256};

const PROFILE: &str = include_str!("../../../docs/spec/MIGRATION_PROFILE_v0.1.json");
const SPEC: &[u8] = include_bytes!("../../../docs/spec/MIGRATION_PROFILE_v0.1.md");
const SCHEMA: &[u8] = include_bytes!("../../../docs/spec/MIGRATION_PROFILE_v0.1.schema.json");

fn module(path: &str, source: &str) -> ModuleForTypecheck {
    let forms = canonicalize_module(parse_module(source).expect("parse module"))
        .expect("canonicalize module");
    let meta = forms.iter().find_map(metadata_payload);
    ModuleForTypecheck {
        path: path.to_string(),
        forms,
        meta,
    }
}

fn metadata_payload(term: &Term) -> Option<Term> {
    let items = term.as_proper_list()?;
    if items.len() != 3 || !matches!(items[1], Term::Symbol(name) if name == "::meta") {
        return None;
    }
    if let Term::Map(metadata) = items[2] {
        return Some(Term::Map(metadata.clone()));
    }
    let quoted = items[2].as_proper_list()?;
    if quoted.len() != 2 || !matches!(quoted[0], Term::Symbol(head) if head == "quote") {
        return None;
    }
    match quoted[1] {
        Term::Map(metadata) => Some(Term::Map(metadata.clone())),
        _ => None,
    }
}

fn provenance(parent_receipt: Option<[u8; 32]>) -> MigrationProvenance {
    MigrationProvenance {
        producer: "agent/test-migrator".to_string(),
        source_artifact: "sha256:fixture-v0".to_string(),
        parent_receipt,
    }
}

fn plan(modules: &[ModuleForTypecheck], steps: Vec<MigrationStep>) -> MigrationPlan {
    MigrationPlan {
        migration_id: "migration/test-v0-to-v1".to_string(),
        intent: "Upgrade the fixture without ambient edits".to_string(),
        expected_source_identity: migration_package_identity(modules).expect("source identity"),
        provenance: provenance(Some([7; 32])),
        steps,
    }
}

fn map_get<'a>(term: &'a Term, key: &str) -> &'a Term {
    let Term::Map(entries) = term else {
        panic!("expected map")
    };
    entries
        .get(&TermOrdKey(Term::symbol(key)))
        .unwrap_or_else(|| panic!("missing {key}"))
}

fn sha256_hex(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn content_identity(value: &JsonValue) -> String {
    let mut payload = value.clone();
    payload
        .as_object_mut()
        .expect("profile object")
        .remove("contentIdentitySha256");
    sha256_hex(
        serde_json::to_string(&payload)
            .expect("canonical profile JSON")
            .as_bytes(),
    )
}

#[test]
fn machine_profile_is_closed_content_addressed_and_source_bound() {
    let profile: JsonValue = serde_json::from_str(PROFILE).expect("valid profile JSON");
    let keys = profile
        .as_object()
        .expect("profile object")
        .keys()
        .map(String::as_str)
        .collect::<Vec<_>>();
    assert_eq!(
        keys,
        [
            "auditDate",
            "canonicalSpec",
            "canonicalSpecSha256",
            "contentIdentitySha256",
            "dryRun",
            "failure",
            "identity",
            "kind",
            "nonclaims",
            "operations",
            "patchTarget",
            "planning",
            "provenance",
            "resourceLimits",
            "schema",
            "schemaSha256",
            "sourceBindings",
            "version",
        ]
    );
    assert_eq!(profile["kind"], MIGRATION_PROFILE_ID);
    assert_eq!(
        profile["patchTarget"]["profile"],
        MIGRATION_PATCH_PROFILE_ID
    );
    assert_eq!(profile["patchTarget"]["version"], MIGRATION_PATCH_VERSION);
    assert_eq!(profile["canonicalSpecSha256"], sha256_hex(SPEC));
    assert_eq!(profile["schemaSha256"], sha256_hex(SCHEMA));
    assert_eq!(profile["contentIdentitySha256"], content_identity(&profile));

    let schema: JsonValue = serde_json::from_slice(SCHEMA).expect("valid schema JSON");
    assert_eq!(schema["additionalProperties"], false);
    let required = schema["required"]
        .as_array()
        .expect("required array")
        .iter()
        .map(|item| item.as_str().expect("required key"))
        .collect::<BTreeSet<_>>();
    assert_eq!(required, keys.into_iter().collect());

    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let bindings = profile["sourceBindings"]
        .as_array()
        .expect("source bindings");
    let ids = bindings
        .iter()
        .map(|binding| binding["id"].as_str().expect("binding id"))
        .collect::<BTreeSet<_>>();
    assert_eq!(ids.len(), bindings.len());
    for binding in bindings {
        let path = binding["path"].as_str().expect("binding path");
        assert!(root.join(path).is_file(), "missing source binding {path}");
    }
}

#[test]
fn dry_run_combines_syntax_api_and_format_changes_without_mutating_input() {
    let modules = vec![module(
        "src/main.gc",
        r#"
          (def ::meta '{
            :caps []
            :exports [pkg/old::value]
            :types {pkg/old::value Int}
            :format-v legacy/v0})
          (def pkg/old::value (if true 1 2))
          (def pkg/use::value pkg/old::value)
          'pkg/old::value
        "#,
    )];
    let original_forms = modules[0].forms.clone();
    let original_meta = modules[0].meta.clone();
    let request = plan(
        &modules,
        vec![
            MigrationStep::RewriteSyntaxHead {
                module_path: "src/main.gc".to_string(),
                from: "if".to_string(),
                to: "begin".to_string(),
                expected_rewrites: 1,
            },
            MigrationStep::RenameApiSymbol {
                from: "pkg/old::value".to_string(),
                to: "pkg/new::value".to_string(),
                expected_rewrites: 4,
            },
            MigrationStep::ReplaceFormatField {
                module_path: "src/main.gc".to_string(),
                field: ":format-v".to_string(),
                expected: Some(Term::symbol("legacy/v0")),
                replacement: Some(Term::symbol("current/v1")),
            },
        ],
    );

    let first = dry_run_migration(&modules, &request).expect("valid migration");
    let second = dry_run_migration(&modules, &request).expect("deterministic replay");

    assert_eq!(modules[0].forms, original_forms, "dry-run mutated forms");
    assert_eq!(modules[0].meta, original_meta, "dry-run mutated metadata");
    assert_eq!(first.plan_identity, second.plan_identity);
    assert_eq!(first.patch_identity, second.patch_identity);
    assert_eq!(first.receipt_identity, second.receipt_identity);
    assert_eq!(
        first.target_package_identity,
        second.target_package_identity
    );
    assert_eq!(first.patch, second.patch);
    assert_eq!(first.receipt, second.receipt);
    assert_ne!(first.source_package_identity, first.target_package_identity);

    let migrated = print_module(&first.migrated_modules[0].forms);
    assert!(migrated.contains("pkg/new::value"));
    assert!(migrated.contains("current/v1"));
    assert!(migrated.contains("(begin true 1 2)"));
    assert!(
        migrated.contains("(quote pkg/old::value)"),
        "quoted user data must not be rewritten: {migrated}"
    );
    assert_eq!(first.module_deltas[0].changed_form_indices, [0, 1, 2]);
    assert!(first.effects.added.is_empty());
    assert!(first.effects.removed.is_empty());

    assert_eq!(map_get(&first.patch, ":version"), &Term::Int(1.into()));
    let provenance = map_get(&first.patch, ":provenance");
    assert_eq!(
        map_get(provenance, ":source-package-h"),
        &Term::Bytes(first.source_package_identity.to_vec().into())
    );
    assert_eq!(
        map_get(provenance, ":target-package-h"),
        &Term::Bytes(first.target_package_identity.to_vec().into())
    );
    let request_provenance = map_get(provenance, ":request-provenance");
    assert_eq!(
        map_get(request_provenance, ":parent-receipt-h"),
        &Term::Bytes(vec![7; 32].into())
    );

    let report = map_get(&first.receipt, ":report");
    assert_eq!(hash_term(report), first.receipt_identity);
    assert_eq!(
        map_get(&first.receipt, ":receipt-h"),
        &Term::Bytes(first.receipt_identity.to_vec().into())
    );
    assert_eq!(map_get(report, ":dry-run"), &Term::Bool(true));
    assert_eq!(
        map_get(map_get(report, ":target-typecheck"), ":ok"),
        &Term::Bool(true)
    );
}

#[test]
fn api_migration_reports_an_introduced_effect_exactly() {
    let modules = vec![module(
        "effect.gc",
        r#"
          (def ::meta '{
            :caps [demo/op]
            :exports [pkg/app::run]
            :types {pkg/app::run ?}})
          (def pkg/app::run
            (fn (payload)
              (pkg/old::perform 'demo/op payload (fn (result) result))))
        "#,
    )];
    let request = plan(
        &modules,
        vec![MigrationStep::RenameApiSymbol {
            from: "pkg/old::perform".to_string(),
            to: "core/effect::perform".to_string(),
            expected_rewrites: 1,
        }],
    );

    let result = dry_run_migration(&modules, &request).expect("effect migration");
    assert_eq!(
        result.effects.added,
        BTreeSet::from(["demo/op".to_string()])
    );
    assert!(result.effects.removed.is_empty());
    assert!(!result.effects.before.unknown);
    assert!(!result.effects.after.unknown);
    assert_eq!(result.module_deltas[0].effects, result.effects);
}

#[test]
fn stale_source_and_detached_metadata_fail_before_planning() {
    let modules = vec![module("main.gc", "(def pkg/app::value 1)")];
    let mut stale = plan(
        &modules,
        vec![MigrationStep::RenameApiSymbol {
            from: "pkg/app::value".to_string(),
            to: "pkg/app::next".to_string(),
            expected_rewrites: 1,
        }],
    );
    stale.expected_source_identity = [0; 32];
    assert!(matches!(
        dry_run_migration(&modules, &stale),
        Err(MigrationError::StaleSource { .. })
    ));

    let mut detached = module(
        "main.gc",
        "(def ::meta '{:format-v legacy/v0})\n(def pkg/app::value 1)",
    );
    detached.meta = None;
    let error = migration_package_identity(&[detached]).expect_err("detached metadata must fail");
    assert!(error.to_string().contains("metadata does not match"));
}

#[test]
fn rewrite_counts_format_preconditions_and_target_collisions_fail_closed() {
    let modules = vec![module(
        "main.gc",
        r#"
          (def ::meta '{:format-v legacy/v0})
          (def pkg/app::old 1)
          (def pkg/app::new 2)
        "#,
    )];

    let count_drift = plan(
        &modules,
        vec![MigrationStep::RenameApiSymbol {
            from: "pkg/app::old".to_string(),
            to: "pkg/app::next".to_string(),
            expected_rewrites: 2,
        }],
    );
    assert!(
        dry_run_migration(&modules, &count_drift)
            .expect_err("count drift")
            .to_string()
            .contains("expected 2 rewrites, found 1")
    );

    let wrong_format = plan(
        &modules,
        vec![MigrationStep::ReplaceFormatField {
            module_path: "main.gc".to_string(),
            field: ":format-v".to_string(),
            expected: Some(Term::symbol("other/v0")),
            replacement: Some(Term::symbol("current/v1")),
        }],
    );
    assert!(
        dry_run_migration(&modules, &wrong_format)
            .expect_err("format precondition")
            .to_string()
            .contains("format field :format-v expected coreform-term-h:")
    );

    let collision = plan(
        &modules,
        vec![MigrationStep::RenameApiSymbol {
            from: "pkg/app::old".to_string(),
            to: "pkg/app::new".to_string(),
            expected_rewrites: 1,
        }],
    );
    assert!(
        dry_run_migration(&modules, &collision)
            .expect_err("definition collision")
            .to_string()
            .contains("already has a package definition")
    );
}

#[test]
fn noncanonical_and_chained_plans_and_invalid_targets_are_rejected() {
    let modules = vec![module("main.gc", "(def pkg/app::value (if true 1 2))")];

    let out_of_order = plan(
        &modules,
        vec![
            MigrationStep::RenameApiSymbol {
                from: "pkg/app::value".to_string(),
                to: "pkg/app::next".to_string(),
                expected_rewrites: 1,
            },
            MigrationStep::RewriteSyntaxHead {
                module_path: "main.gc".to_string(),
                from: "if".to_string(),
                to: "begin".to_string(),
                expected_rewrites: 1,
            },
        ],
    );
    assert!(
        dry_run_migration(&modules, &out_of_order)
            .expect_err("canonical order")
            .to_string()
            .contains("canonical syntax/API/format order")
    );

    let chained = plan(
        &modules,
        vec![
            MigrationStep::RenameApiSymbol {
                from: "pkg/app::a".to_string(),
                to: "pkg/app::b".to_string(),
                expected_rewrites: 1,
            },
            MigrationStep::RenameApiSymbol {
                from: "pkg/app::b".to_string(),
                to: "pkg/app::c".to_string(),
                expected_rewrites: 1,
            },
        ],
    );
    assert!(
        dry_run_migration(&modules, &chained)
            .expect_err("rewrite chain")
            .to_string()
            .contains("rewrite chains are ambiguous")
    );

    let invalid_target = plan(
        &modules,
        vec![MigrationStep::RewriteSyntaxHead {
            module_path: "main.gc".to_string(),
            from: "if".to_string(),
            to: "pkg/missing::form".to_string(),
            expected_rewrites: 1,
        }],
    );
    assert!(matches!(
        dry_run_migration(&modules, &invalid_target),
        Err(MigrationError::InvalidTarget(_))
    ));
}

#[test]
fn resource_and_portability_bounds_fail_before_identity_or_rewrite() {
    let non_nfc = module("cafe\u{301}.gc", "(def pkg/app::value 1)");
    assert!(
        migration_package_identity(&[non_nfc])
            .expect_err("non-NFC path")
            .to_string()
            .contains("portable and base-relative")
    );

    let modules = vec![module("main.gc", "(def pkg/app::value 1)")];
    let mut oversized = plan(
        &modules,
        vec![MigrationStep::RenameApiSymbol {
            from: "pkg/app::value".to_string(),
            to: "pkg/app::next".to_string(),
            expected_rewrites: 1,
        }],
    );
    oversized.intent = "x".repeat(MAX_MIGRATION_INTENT_BYTES + 1);
    assert!(
        dry_run_migration(&modules, &oversized)
            .expect_err("oversized intent")
            .to_string()
            .contains("at most 16384 bytes")
    );
}
