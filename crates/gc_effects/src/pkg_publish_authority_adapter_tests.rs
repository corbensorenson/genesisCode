use super::*;

fn hash(byte: u8) -> String {
    format!("{byte:02x}").repeat(32)
}

fn put(term: Term, name: &str, value: Term) -> Term {
    let Term::Map(mut fields) = term else {
        panic!("test term must be a map");
    };
    fields.insert(TermOrdKey(Term::symbol(name)), value);
    Term::Map(fields)
}

fn with_embedded_hash(term: Term, name: &str) -> Term {
    let Term::Map(mut fields) = term else {
        panic!("test term must be a map");
    };
    let digest = hex32(hash_term(&Term::Map(fields.clone())));
    fields.insert(TermOrdKey(Term::symbol(name)), Term::Str(digest));
    Term::Map(fields)
}

fn commit() -> Term {
    gc_coreform::parse_term(&format!(
        r#"{{
          :type :vcs/commit
          :v 1
          :parents ["{}"]
          :target {{ :kind :package :name "adapter-test" }}
          :base nil
          :patch "{}"
          :result "{}"
          :obligations [core/obligation::unit-tests]
          :evidence ["{}"]
          :attestations ["{}"]
          :message "adapter poison control"
        }}"#,
        hash(1),
        hash(2),
        hash(3),
        hash(4),
        hash(5),
    ))
    .expect("commit term")
}

fn facts(commit: Term) -> Term {
    map([
        (":commit", commit),
        (":commit-h", Term::Str(hash(6))),
        (":depth", Term::Int(3.into())),
        (":expected-old", Term::Str(hash(7))),
        (":policy", map([(":type", Term::symbol(":vcs/policy"))])),
        (":policy-h", Term::Str(hash(8))),
        (":ref", Term::Str("refs/heads/main".to_string())),
        (":remote", Term::Str("https://registry.invalid".to_string())),
    ])
}

#[test]
fn adapter_rejects_open_wrong_hash_and_undeclared_phase_results() {
    let request_hash = [9_u8; 32];
    let valid = map([
        (":code", Term::Nil),
        (":kind", Term::Str(RESULT_KIND.to_string())),
        (":message", Term::Nil),
        (":ok", Term::Bool(true)),
        (":request-h", Term::Str(hex32(request_hash))),
        (":v", Term::Int(1.into())),
        (":value", map([(":accepted", Term::Bool(true))])),
    ]);
    assert!(matches!(
        decode_phase_result(valid.clone(), request_hash),
        Ok(PhaseResult::Accept(_))
    ));

    let open = put(valid.clone(), ":extra", Term::Bool(true));
    assert!(decode_phase_result(open, request_hash).is_err());
    let wrong_hash = put(valid, ":request-h", Term::Str(hash(10)));
    assert!(decode_phase_result(wrong_hash, request_hash).is_err());

    let rejected = map([
        (":code", Term::Str("core/pkg/undeclared".to_string())),
        (":kind", Term::Str(RESULT_KIND.to_string())),
        (":message", Term::Str("poison".to_string())),
        (":ok", Term::Bool(false)),
        (":request-h", Term::Str(hex32(request_hash))),
        (":v", Term::Int(1.into())),
        (":value", Term::Nil),
    ]);
    assert!(decode_phase_result(rejected, request_hash).is_err());
}

#[test]
fn adapter_rejects_contradictory_finalize_provenance_and_sync() {
    let facts = facts(commit());
    let value = map([
        (":commit", Term::Str(hash(6))),
        (
            ":provenance",
            expected_provenance(facts_field(&facts, ":commit").unwrap()).unwrap(),
        ),
        (":ref", Term::Str("refs/heads/main".to_string())),
        (":sync", expected_sync(&facts).unwrap()),
    ]);
    assert!(matches!(
        decode_finalize_value(value.clone(), &facts),
        Ok(PkgPublishDecision::Accept { .. })
    ));

    let bad_provenance = put(value.clone(), ":provenance", Term::Nil);
    assert!(decode_finalize_value(bad_provenance, &facts).is_err());
    let bad_sync = put(
        value,
        ":sync",
        map([(":remote", Term::Str("poison".into()))]),
    );
    assert!(decode_finalize_value(bad_sync, &facts).is_err());
}

#[test]
fn adapter_reports_invalid_crypto_as_false_but_rejects_protocol_poisoning() {
    let expected_signing_hash = [11_u8; 32];
    let request = with_embedded_hash(
        map([
            (":alg", Term::Str("unsupported".to_string())),
            (":allowed-public-keys", Term::Vector(Vec::new())),
            (":attestation-h", Term::Str(hash(12))),
            (":pk", Term::Bytes(vec![0_u8; 31].into())),
            (":sig", Term::Bytes(vec![0_u8; 63].into())),
            (":sign-message", Term::Bytes(Vec::new().into())),
            (
                ":signing-h",
                Term::Bytes(expected_signing_hash.to_vec().into()),
            ),
        ]),
        ":request-h",
    );
    let (_, valid) = verify_crypto_request(&request, &expected_signing_hash).unwrap();
    assert!(!valid);

    let poisoned = put(request.clone(), ":request-h", Term::Str(hash(13)));
    assert!(verify_crypto_request(&poisoned, &expected_signing_hash).is_err());
    let open = put(request, ":extra", Term::Bool(true));
    assert!(verify_crypto_request(&open, &expected_signing_hash).is_err());
}

#[test]
fn mechanical_signing_hash_matches_native_assurance_oracle() {
    let commit = commit();
    assert_eq!(
        mechanical_signing_hash(&commit).unwrap(),
        gc_vcs::commit_signing_hash(&commit).unwrap()
    );
}
