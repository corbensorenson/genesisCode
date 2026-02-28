use super::*;

fn poison_patch_refactor_migrate_contract_signature_forms(artifact: &Path) {
    let src = fs::read_to_string(artifact).expect("read toolchain artifact");
    let mut term = parse_term(&src).expect("parse toolchain artifact");
    let Term::Map(root) = &mut term else {
        panic!("artifact root must be map");
    };
    let modules = root
        .get_mut(&TermOrdKey(Term::symbol(":modules")))
        .expect("artifact :modules");
    let Term::Vector(entries) = modules else {
        panic!("artifact :modules must be vector");
    };
    let patch_mod = entries
        .iter_mut()
        .find_map(|entry| match entry {
            Term::Map(mm)
                if matches!(
                    mm.get(&TermOrdKey(Term::symbol(":path"))),
                    Some(Term::Str(path)) if path == "selfhost/patch_schema_refactor_v1.gc"
                ) =>
            {
                Some(mm)
            }
            _ => None,
        })
        .expect("selfhost/patch_schema_refactor_v1.gc entry");

    let module_src = match patch_mod.get(&TermOrdKey(Term::symbol(":source"))) {
        Some(Term::Str(src)) => src.clone(),
        _ => panic!("patch refactor module missing :source"),
    };
    let poisoned_src = format!(
        "{module_src}\n(def core/cli::migrate-contract-signature-forms (fn (req) ((core/error::make2 \"core/patch-schema\") \"migrate-contract-signature poisoned\")))\n"
    );
    let poisoned_forms = canonicalize_module(parse_module(&poisoned_src).expect("parse poisoned"))
        .expect("canonicalize poisoned");
    let poisoned_hash = hash_module(&poisoned_forms);
    patch_mod.insert(TermOrdKey(Term::symbol(":source")), Term::Str(poisoned_src));
    patch_mod.insert(
        TermOrdKey(Term::symbol(":forms")),
        Term::Vector(poisoned_forms),
    );
    patch_mod.insert(
        TermOrdKey(Term::symbol(":module-h")),
        Term::Bytes(poisoned_hash.to_vec().into()),
    );
    fs::write(artifact, print_term(&term)).expect("write poisoned artifact");
}

#[test]
fn apply_patch_selfhost_migrate_contract_signature_uses_refactor_contract() {
    let td = tempfile::tempdir().unwrap();
    let pkg = write_refactor_pkg(td.path());
    let patch = write_patch(
        td.path(),
        r#"
          {
            :version 1
            :intent "migrate contract signature"
            :provenance {}
            :ops [
              {
                :op :migrate-contract-signature
                :module-path "mod.gc"
                :contract-symbol pkg/refactor::contract
                :from-param msg
                :to-param request
              }
            ]
          }
        "#,
    );
    let artifact = copy_repo_toolchain_artifact(td.path());
    poison_patch_refactor_migrate_contract_signature_forms(&artifact);
    let frontend =
        gc_obligations::CoreformFrontend::Selfhost(gc_obligations::SelfhostFrontendConfig {
            bootstrap_mode: gc_prelude::SelfhostBootstrapMode::ArtifactOnly,
            artifact: Some(artifact),
        });

    let err = gc_patches::apply_patch_with_step_limit_and_frontend(
        &patch,
        &pkg,
        None,
        StepLimit::Default,
        MemLimits::default(),
        frontend,
    )
    .unwrap_err();
    match err {
        gc_patches::PatchError::Validate(msg) => assert!(
            msg.contains("migrate-contract-signature poisoned"),
            "expected poisoned migrate error, got: {msg}"
        ),
        other => panic!("expected PatchError::Validate, got {other}"),
    }
}
