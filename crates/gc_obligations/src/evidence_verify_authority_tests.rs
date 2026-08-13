use std::{collections::BTreeMap, path::PathBuf};

use gc_coreform::{Term, TermOrdKey};

use crate::{
    DsseVerificationFacts, EvidenceFact, EvidenceVerifyAuthority, PackageVerificationRequest,
    PolicyKeyObservation, RegistryPolicyObservation, SignatureObservation, StoreHashObservation,
    TransparencyEntryObservation,
};

fn fixture_authority() -> EvidenceVerifyAuthority {
    let artifact = std::env::var_os("GENESIS_TEST_SELFHOST_ARTIFACT")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("../..")
                .join("selfhost/toolchain.gc")
        });
    EvidenceVerifyAuthority::load(&artifact).expect("load evidence authority")
}

fn map(entries: impl IntoIterator<Item = (&'static str, Term)>) -> Term {
    Term::Map(
        entries
            .into_iter()
            .map(|(name, value)| (TermOrdKey(Term::symbol(name)), value))
            .collect::<BTreeMap<_, _>>(),
    )
}

fn bytes(value: impl Into<Vec<u8>>) -> Term {
    Term::Bytes(value.into().into())
}

fn package_request(facts: Vec<EvidenceFact>) -> PackageVerificationRequest {
    PackageVerificationRequest {
        facts,
        acceptance_hash: None,
        acceptance: Term::Nil,
        store: Vec::new(),
        policy: None,
        signature_set: Term::Nil,
        signatures: Vec::new(),
    }
}

fn signed_package_request() -> PackageVerificationRequest {
    let acceptance_hash = [1; 32];
    let signature_hash = "03".repeat(32);
    let public_key = [2; 32];
    let acceptance = map([
        (":kind", Term::Str("genesis/acceptance-v0.2".to_string())),
        (":obligations", Term::Vector(Vec::new())),
        (":ok", Term::Bool(true)),
        (
            ":package",
            map([
                (":name", Term::Str("fixture".to_string())),
                (":version", Term::Str("0.1.0".to_string())),
            ]),
        ),
    ]);
    let signature = map([
        (":acceptance-h", bytes(acceptance_hash)),
        (":alg", Term::Str("ed25519".to_string())),
        (
            ":kind",
            Term::Str("genesis/acceptance-signature-v0.2".to_string()),
        ),
        (":pk", bytes(public_key)),
        (":sig", bytes(vec![4; 64])),
    ]);
    PackageVerificationRequest {
        facts: Vec::new(),
        acceptance_hash: Some(acceptance_hash),
        acceptance,
        store: vec![
            StoreHashObservation {
                role: ":acceptance",
                required_hash: "01".repeat(32),
                observed_hash: Some("01".repeat(32)),
                load_error: None,
            },
            StoreHashObservation {
                role: ":signature",
                required_hash: signature_hash.clone(),
                observed_hash: Some(signature_hash.clone()),
                load_error: None,
            },
        ],
        policy: Some(RegistryPolicyObservation {
            version: 1,
            min_signatures: 1,
            allowed_keys: vec![PolicyKeyObservation {
                encoded: "fixture-key".to_string(),
                decoded: Some(public_key),
                decode_error: None,
                key_valid: true,
            }],
        }),
        signature_set: Term::Vector(vec![Term::Str(signature_hash.clone())]),
        signatures: vec![SignatureObservation {
            artifact_hash: signature_hash,
            crypto_valid: true,
            term: signature,
        }],
    }
}

#[test]
fn package_identity_and_mechanism_facts_control_consumed_verdict() {
    let accepted = fixture_authority()
        .package(package_request(vec![EvidenceFact {
            class: ":identity",
            code: "fixture/identity".to_string(),
            mechanism_ok: true,
            observed: Term::Str("same".to_string()),
            required: Term::Str("same".to_string()),
        }]))
        .expect("valid fact request");
    assert!(accepted.verified);

    let rejected = fixture_authority()
        .package(package_request(vec![EvidenceFact {
            class: ":identity",
            code: "fixture/identity".to_string(),
            mechanism_ok: true,
            observed: Term::Str("changed".to_string()),
            required: Term::Str("same".to_string()),
        }]))
        .expect("semantic denial is a valid authority result");
    assert_eq!(rejected.errors, vec!["fixture/identity"]);
    assert!(!rejected.verified);
}

#[test]
fn package_authority_owns_schema_store_key_and_threshold_decisions() {
    let accepted = fixture_authority()
        .package(signed_package_request())
        .expect("signed package request");
    assert!(
        accepted.verified,
        "valid signed package denial: {accepted:?}"
    );
    assert_eq!(accepted.valid_signatures, 1);

    let mut store_substitution = signed_package_request();
    store_substitution.store[0].observed_hash = Some("ff".repeat(32));
    assert!(
        !fixture_authority()
            .package(store_substitution)
            .expect("store substitution decision")
            .verified
    );

    let mut key_substitution = signed_package_request();
    let Term::Map(signature) = &mut key_substitution.signatures[0].term else {
        panic!("fixture signature map")
    };
    signature.insert(TermOrdKey(Term::symbol(":pk")), bytes([9; 32]));
    assert!(
        !fixture_authority()
            .package(key_substitution)
            .expect("key substitution decision")
            .verified
    );

    let mut threshold = signed_package_request();
    threshold
        .policy
        .as_mut()
        .expect("fixture policy")
        .min_signatures = 2;
    assert!(
        !fixture_authority()
            .package(threshold)
            .expect("threshold decision")
            .verified
    );

    let mut open_signature = signed_package_request();
    let Term::Map(signature) = &mut open_signature.signatures[0].term else {
        panic!("fixture signature map")
    };
    signature.insert(TermOrdKey(Term::symbol(":extra")), Term::Bool(true));
    assert!(
        !fixture_authority()
            .package(open_signature)
            .expect("open signature decision")
            .verified
    );

    let mut omitted_reference = signed_package_request();
    let Term::Map(acceptance) = &mut omitted_reference.acceptance else {
        panic!("fixture acceptance map")
    };
    acceptance.insert(
        TermOrdKey(Term::symbol(":obligations")),
        Term::Vector(vec![map([
            (":artifact", Term::Str("05".repeat(32))),
            (":name", Term::symbol("fixture/obligation")),
            (":ok", Term::Bool(true)),
        ])]),
    );
    assert!(
        !fixture_authority()
            .package(omitted_reference)
            .expect("omitted reference decision")
            .verified
    );
}

#[test]
fn transparency_authority_rejects_cycles_and_store_substitution() {
    let hash = [7; 32];
    let term = map([
        (":acceptance-artifact", Term::Str("aa".repeat(32))),
        (
            ":kind",
            Term::Str("genesis/transparency-entry-v0.2".to_string()),
        ),
        (":package-artifact", Term::Str("bb".repeat(32))),
        (":prev-h", bytes(hash)),
        (":signature-artifact", Term::Str("cc".repeat(32))),
        (":signer-pk-b64", Term::Str("fixture-key".to_string())),
    ]);
    let observation = TransparencyEntryObservation {
        hash,
        observed_hash: Some(hash),
        load_error: None,
        term,
    };
    let decision = fixture_authority()
        .transparency(Some(hash), None, vec![observation.clone(), observation])
        .expect("cycle request");
    assert_eq!(decision.errors, vec!["transparency/cycle"]);

    let substituted = TransparencyEntryObservation {
        observed_hash: Some([8; 32]),
        ..decision_fixture(hash)
    };
    assert!(
        !fixture_authority()
            .transparency(Some(hash), None, vec![substituted])
            .expect("store substitution request")
            .verified
    );
}

fn decision_fixture(hash: [u8; 32]) -> TransparencyEntryObservation {
    TransparencyEntryObservation {
        hash,
        observed_hash: Some(hash),
        load_error: None,
        term: map([
            (":acceptance-artifact", Term::Str("aa".repeat(32))),
            (
                ":kind",
                Term::Str("genesis/transparency-entry-v0.2".to_string()),
            ),
            (":package-artifact", Term::Str("bb".repeat(32))),
            (":prev-h", Term::Nil),
            (":signature-artifact", Term::Str("cc".repeat(32))),
            (":signer-pk-b64", Term::Str("fixture-key".to_string())),
        ]),
    }
}

#[test]
fn dsse_authority_requires_closed_field_inventories_and_crypto() {
    let envelope_fields = vec![
        "payload".to_string(),
        "payloadType".to_string(),
        "signatures".to_string(),
    ];
    let signature_fields = vec!["keyid".to_string(), "sig".to_string()];
    let valid = DsseVerificationFacts {
        envelope_fields: &envelope_fields,
        expected_key_id: "sha256:key",
        expected_payload_type: "fixture/type",
        key_id: "sha256:key",
        key_valid: true,
        payload_hash: [9; 32],
        payload_type: "fixture/type",
        signature_count: 1,
        signature_fields: &signature_fields,
        signature_key_id: "sha256:key",
        signature_valid: true,
    };
    assert!(
        fixture_authority()
            .dsse(valid.clone())
            .expect("valid DSSE facts")
            .verified
    );

    let mut invalid_crypto = valid.clone();
    invalid_crypto.signature_valid = false;
    assert!(
        !fixture_authority()
            .dsse(invalid_crypto)
            .expect("invalid DSSE crypto")
            .verified
    );

    let open_envelope = vec![
        "extra".to_string(),
        "payload".to_string(),
        "payloadType".to_string(),
        "signatures".to_string(),
    ];
    let mut invalid_shape = valid;
    invalid_shape.envelope_fields = &open_envelope;
    assert!(
        !fixture_authority()
            .dsse(invalid_shape)
            .expect("open DSSE envelope")
            .verified
    );
}
