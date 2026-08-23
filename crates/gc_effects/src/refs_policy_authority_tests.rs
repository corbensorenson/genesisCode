use super::*;

fn accepted_result(request_hash: [u8; 32], value: Term) -> Term {
    map([
        (":code", Term::Nil),
        (":kind", Term::Str(RESULT_KIND.to_string())),
        (":message", Term::Nil),
        (":ok", Term::Bool(true)),
        (":request-h", Term::Str(hex32(request_hash))),
        (":v", Term::Int(1.into())),
        (":value", value),
    ])
}

#[test]
fn result_decoder_rejects_request_substitution_and_open_fields() {
    let request_hash = [7; 32];
    let value = map([(":admit", Term::Bool(true))]);
    assert!(matches!(
        decode_phase_result(accepted_result(request_hash, value.clone()), request_hash),
        Ok(PhaseResult::Accept(actual)) if actual == value
    ));
    assert!(decode_phase_result(accepted_result([8; 32], value.clone()), request_hash).is_err());

    let open_result = accepted_result(request_hash, value);
    assert!(matches!(open_result, Term::Map(_)));
    let Term::Map(mut open) = open_result else {
        return;
    };
    open.insert(TermOrdKey(Term::symbol(":extra")), Term::Bool(true));
    assert!(decode_phase_result(Term::Map(open), request_hash).is_err());
}

#[test]
fn admission_decoder_rejects_identity_substitution_and_open_fields() {
    let name = "refs/heads/main";
    let commit_hash = "a".repeat(64);
    let policy_hash = "b".repeat(64);
    let admission = map([
        (":admit", Term::Bool(true)),
        (":commit-h", Term::Str(commit_hash.clone())),
        (":policy-h", Term::Str(policy_hash.clone())),
        (":ref", Term::Str(name.to_string())),
    ]);
    assert!(decode_admission(admission.clone(), name, Some(&commit_hash), &policy_hash).is_ok());
    assert!(
        decode_admission(
            admission.clone(),
            "refs/heads/other",
            Some(&commit_hash),
            &policy_hash
        )
        .is_err()
    );

    assert!(matches!(admission, Term::Map(_)));
    let Term::Map(mut open) = admission else {
        return;
    };
    open.insert(TermOrdKey(Term::symbol(":extra")), Term::Bool(true));
    assert!(decode_admission(Term::Map(open), name, Some(&commit_hash), &policy_hash).is_err());
}

#[test]
fn hash_inventory_decoder_enforces_shape_and_object_bound() {
    let valid = Term::Vector(vec![Term::Str("c".repeat(64))]);
    assert_eq!(hash_vector(&valid).unwrap(), vec!["c".repeat(64)]);
    assert!(hash_vector(&Term::Vector(vec![Term::Str("C".repeat(64))])).is_err());
    assert!(
        hash_vector(&Term::Vector(vec![
            Term::Str("d".repeat(64));
            MAX_OBJECTS + 1
        ]))
        .is_err()
    );
}

#[test]
fn artifact_loader_rejects_content_hash_substitution() {
    let temp = tempfile::tempdir().unwrap();
    let store = ArtifactStore::open(temp.path()).unwrap();
    let hash = store.put_bytes(b"{:v 1}").unwrap();
    std::fs::write(store.path_for(&hash), b"{:v 2}").unwrap();
    let mut observed = 0;
    let decision = load_object(&store, &hash, ObjectRole::Evidence, &mut observed)
        .unwrap()
        .unwrap_err();
    assert!(matches!(
        decision,
        RefsPolicyDecision::Error { code, .. } if code == "core/store/corruption"
    ));
}
