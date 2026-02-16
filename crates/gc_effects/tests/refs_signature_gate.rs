use base64ct::{Base64, Encoding};
use ed25519_dalek::{Signer, SigningKey};
use gc_coreform::{Term, hash_module, parse_module, parse_term, print_term};
use gc_effects::{ArtifactStore, CapsPolicy, run};
use gc_kernel::{EvalCtx, Value, eval_module};
use gc_prelude::build_prelude;
use rand_core::OsRng;

fn eval_prog(forms: &[Term]) -> (EvalCtx, Value) {
    let mut ctx = EvalCtx::new();
    let prelude = build_prelude(&mut ctx);
    let mut env = prelude.env;
    let prog = eval_module(&mut ctx, &mut env, forms).expect("eval module");
    (ctx, prog)
}

#[test]
fn refs_set_enforces_signature_policy_for_tags() {
    let td = tempfile::tempdir().unwrap();
    let caps_path = td.path().join("caps.toml");
    std::fs::write(
        &caps_path,
        r#"
allow = ["core/refs::set"]

[store]
dir = "./.genesis/store"

[refs]
path = "./.genesis/refs.gc"
"#,
    )
    .unwrap();
    let pol = CapsPolicy::load(&caps_path).unwrap();

    let store_dir = td.path().join(".genesis").join("store");
    let store = ArtifactStore::open(&store_dir).unwrap();

    // Signing key and policy allowlist.
    let sk = SigningKey::generate(&mut OsRng);
    let pk_b64 = Base64::encode_string(&sk.verifying_key().to_bytes());

    let policy_term = parse_term(&format!(
        r#"
        {{
          :type :vcs/policy
          :v 1
          :classes {{
            :tags {{
              :patterns ["refs/**/tags/*"]
              :required-obligations ["core/obligation::unit-tests"]
              :require-signatures true
              :min-signatures 1
              :allowed-public-keys ["{pk_b64}"]
            }}
          }}
        }}
        "#
    ))
    .unwrap();
    let policy_h = store
        .put_bytes(print_term(&policy_term).as_bytes())
        .unwrap();

    let evidence_term =
        parse_term(r#"{:type :vcs/evidence :v 1 :kind :unit-tests :data nil}"#).unwrap();
    let evidence_h = store
        .put_bytes(print_term(&evidence_term).as_bytes())
        .unwrap();

    let commit_base_term = parse_term(&format!(
        r#"
        {{
          :type :vcs/commit
          :v 1
          :parents []
          :base nil
          :patch "{z}"
          :result "{z}"
          :obligations ["core/obligation::unit-tests"]
          :evidence ["{evidence_h}"]
          :attestations []
          :message "t"
        }}
        "#,
        z = "0".repeat(64)
    ))
    .unwrap();

    let signing_h = gc_vcs::commit_signing_hash(&commit_base_term).unwrap();
    let msg = gc_vcs::commit_attestation_message(&signing_h);
    let sig = sk.sign(&msg);

    let att_term = Term::Map(
        [
            (
                gc_coreform::TermOrdKey(Term::symbol(":type")),
                Term::symbol(":vcs/attestation"),
            ),
            (
                gc_coreform::TermOrdKey(Term::symbol(":v")),
                Term::Int(1.into()),
            ),
            (
                gc_coreform::TermOrdKey(Term::symbol(":alg")),
                Term::Str("ed25519".to_string()),
            ),
            (
                gc_coreform::TermOrdKey(Term::symbol(":signing-h")),
                Term::Bytes(signing_h.to_vec().into()),
            ),
            (
                gc_coreform::TermOrdKey(Term::symbol(":pk")),
                Term::Bytes(sk.verifying_key().to_bytes().to_vec().into()),
            ),
            (
                gc_coreform::TermOrdKey(Term::symbol(":sig")),
                Term::Bytes(sig.to_bytes().to_vec().into()),
            ),
        ]
        .into_iter()
        .collect(),
    );
    let att_h = store.put_bytes(print_term(&att_term).as_bytes()).unwrap();

    let commit_term = parse_term(&format!(
        r#"
        {{
          :type :vcs/commit
          :v 1
          :parents []
          :base nil
          :patch "{z}"
          :result "{z}"
          :obligations ["core/obligation::unit-tests"]
          :evidence ["{evidence_h}"]
          :attestations ["{att_h}"]
          :message "t"
        }}
        "#,
        z = "0".repeat(64)
    ))
    .unwrap();
    let commit_h = store
        .put_bytes(print_term(&commit_term).as_bytes())
        .unwrap();

    // Signature-required policy: should accept.
    let set_src = format!(
        r#"
        (def prog
          (core/effect::perform
            'core/refs::set
            {{:name "refs/tags/v1.0.0" :hash "{commit_h}" :policy "{policy_h}"}}
            (fn (r) (core/effect::pure r))))
        prog
        "#
    );
    let set_forms = parse_module(&set_src).unwrap();
    let set_hash = hash_module(&set_forms);
    let (mut ctx, prog) = eval_prog(&set_forms);
    let r = run(
        &mut ctx,
        &pol,
        prog,
        set_hash,
        "gc_effects-test".to_string(),
    )
    .unwrap();
    match r.value {
        Value::Data(_) => {}
        other => panic!("expected ok value, got {}", other.debug_repr()),
    }

    // If we remove attestations, policy should reject.
    let commit_no_sig_term = parse_term(&format!(
        r#"
        {{
          :type :vcs/commit
          :v 1
          :parents []
          :base nil
          :patch "{z}"
          :result "{z}"
          :obligations ["core/obligation::unit-tests"]
          :evidence ["{evidence_h}"]
          :attestations []
          :message "t"
        }}
        "#,
        z = "0".repeat(64)
    ))
    .unwrap();
    let commit_no_sig_h = store
        .put_bytes(print_term(&commit_no_sig_term).as_bytes())
        .unwrap();

    let set2_src = format!(
        r#"
        (def prog
          (core/effect::perform
            'core/refs::set
            {{:name "refs/tags/v1.0.1" :hash "{commit_no_sig_h}" :policy "{policy_h}"}}
            (fn (r) (core/effect::pure r))))
        prog
        "#
    );
    let set2_forms = parse_module(&set2_src).unwrap();
    let set2_hash = hash_module(&set2_forms);
    let (mut ctx2, prog2) = eval_prog(&set2_forms);
    let r2 = run(
        &mut ctx2,
        &pol,
        prog2,
        set2_hash,
        "gc_effects-test".to_string(),
    )
    .unwrap();
    match r2.value {
        Value::Sealed { .. } => {}
        other => panic!("expected sealed error, got {}", other.debug_repr()),
    }
}
