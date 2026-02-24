use super::Policy;
use base64ct::{Base64, Encoding};
use ed25519_dalek::SigningKey;
use gc_coreform::parse_term;
use std::collections::BTreeSet;

#[test]
fn policy_parses_required_evidence_kinds_and_obligation_mapping() {
    let t = parse_term(
        r#"
        {
          :type :vcs/policy
          :v 1
          :classes {
            :main {
              :patterns ["refs/**/heads/main"]
              :required-obligations [core/obligation::unit-tests]
              :required-evidence-kinds [:unit-tests]
              :obligation-evidence-kinds {
                core/obligation::unit-tests [:effect-log]
              }
            }
          }
        }
        "#,
    )
    .expect("policy term");

    let pol = Policy::from_term(&t).expect("policy parse");
    let class = pol.class_for_ref("refs/heads/main").expect("class");
    let required = class.required_evidence_kind_set(&[
        "core/obligation::unit-tests".to_string(),
        "core/obligation::other".to_string(),
    ]);
    assert!(required.contains(":unit-tests"));
    assert!(required.contains(":effect-log"));

    let observed: BTreeSet<String> = [":unit-tests".to_string()].into_iter().collect();
    let missing = class
        .missing_required_evidence_kinds(&["core/obligation::unit-tests".to_string()], &observed);
    assert_eq!(missing, vec![":effect-log".to_string()]);
}

#[test]
fn policy_parses_role_constraints_for_signature_classes() {
    let sk = SigningKey::from_bytes(&[7u8; 32]);
    let pk_b64 = Base64::encode_string(&sk.verifying_key().to_bytes());
    let policy_src = r#"
        {
          :type :vcs/policy
          :v 1
          :classes {
            :tags {
              :patterns ["refs/**/tags/*"]
              :required-obligations [core/obligation::unit-tests]
              :require-signatures true
              :min-signatures 2
              :allowed-public-keys ["__PK_B64__"]
              :required-attestation-roles [:reviewer :verifier]
              :role-min-signatures {:reviewer 1 :verifier 1}
              :independent-role-pairs [{:left :reviewer :right :verifier}]
            }
          }
        }
        "#
    .replace("__PK_B64__", &pk_b64);
    let t = parse_term(&policy_src).expect("policy term");

    let pol = Policy::from_term(&t).expect("policy parse");
    let class = pol.class_for_ref("refs/tags/v1.2.3").expect("class");
    assert!(
        class
            .required_attestation_roles
            .contains(&":reviewer".to_string())
    );
    assert!(
        class
            .required_attestation_roles
            .contains(&":verifier".to_string())
    );
    assert_eq!(class.role_min_signatures.get(":reviewer"), Some(&1));
    assert_eq!(class.role_min_signatures.get(":verifier"), Some(&1));
    assert_eq!(
        class.independent_role_pairs,
        vec![(":reviewer".to_string(), ":verifier".to_string())]
    );
}
