fn decode_result(term: Term, request_hash: [u8; 32]) -> Result<Term, SigningError> {
    let fields = exact_map(
        &term,
        "authority result",
        &[
            ":code",
            ":data",
            ":kind",
            ":message",
            ":ok",
            ":request-h",
            ":v",
        ],
    )?;
    require_string(fields, ":kind", "authority result", RESULT_KIND)?;
    require_int_one(fields, ":v", "authority result")?;
    require_string(
        fields,
        ":request-h",
        "authority result",
        &hash_hex(request_hash),
    )?;
    match fields.get(&key(":ok")) {
        Some(Term::Bool(true)) => {
            if fields.get(&key(":code")) != Some(&Term::Nil)
                || fields.get(&key(":message")) != Some(&Term::Nil)
            {
                return Err(authority_error(
                    "accepted result must carry nil :code and :message",
                ));
            }
            fields
                .get(&key(":data"))
                .cloned()
                .ok_or_else(|| authority_error("accepted result missing :data"))
        }
        Some(Term::Bool(false)) => {
            if fields.get(&key(":data")) != Some(&Term::Nil) {
                return Err(authority_error("rejected result must carry nil :data"));
            }
            let code = required_nonempty_string(fields, ":code", "rejected result")?;
            let message = required_nonempty_string(fields, ":message", "rejected result")?;
            Err(authority_error(format!("rejected [{code}]: {message}")))
        }
        _ => Err(authority_error("result :ok must be a bool")),
    }
}

fn exact_map<'a>(
    term: &'a Term,
    context: &str,
    names: &[&str],
) -> Result<&'a std::collections::BTreeMap<TermOrdKey, Term>, SigningError> {
    let Term::Map(fields) = term else {
        return Err(authority_error(format!("{context} must be a data map")));
    };
    let expected: BTreeSet<_> = names.iter().map(|name| key(name)).collect();
    if fields.keys().cloned().collect::<BTreeSet<_>>() != expected {
        return Err(authority_error(format!("{context} field set mismatch")));
    }
    Ok(fields)
}

fn required_nonempty_string(
    fields: &std::collections::BTreeMap<TermOrdKey, Term>,
    name: &str,
    context: &str,
) -> Result<String, SigningError> {
    match fields.get(&key(name)) {
        Some(Term::Str(value)) if !value.is_empty() => Ok(value.clone()),
        _ => Err(authority_error(format!(
            "{context} {name} must be a nonempty string"
        ))),
    }
}

fn require_string(
    fields: &std::collections::BTreeMap<TermOrdKey, Term>,
    name: &str,
    context: &str,
    expected: &str,
) -> Result<(), SigningError> {
    if fields.get(&key(name)) == Some(&Term::Str(expected.to_string())) {
        Ok(())
    } else {
        Err(authority_error(format!("{context} {name} mismatch")))
    }
}

fn require_int_one(
    fields: &std::collections::BTreeMap<TermOrdKey, Term>,
    name: &str,
    context: &str,
) -> Result<(), SigningError> {
    if fields.get(&key(name)) == Some(&Term::Int(1.into())) {
        Ok(())
    } else {
        Err(authority_error(format!("{context} {name} mismatch")))
    }
}

fn required_bytes(
    fields: &std::collections::BTreeMap<TermOrdKey, Term>,
    name: &str,
    context: &str,
) -> Result<Vec<u8>, SigningError> {
    match fields.get(&key(name)) {
        Some(Term::Bytes(value)) => Ok(value.to_vec()),
        _ => Err(authority_error(format!("{context} {name} must be bytes"))),
    }
}

fn require_bytes32(
    fields: &std::collections::BTreeMap<TermOrdKey, Term>,
    name: &str,
    context: &str,
    expected: [u8; 32],
) -> Result<(), SigningError> {
    match fields.get(&key(name)) {
        Some(Term::Bytes(value)) if value.as_ref() == expected => Ok(()),
        _ => Err(authority_error(format!("{context} {name} mismatch"))),
    }
}

fn required_hash_vector(
    fields: &std::collections::BTreeMap<TermOrdKey, Term>,
    name: &str,
    context: &str,
) -> Result<Vec<String>, SigningError> {
    let Some(Term::Vector(values)) = fields.get(&key(name)) else {
        return Err(authority_error(format!(
            "{context} {name} must be a vector, got {}",
            fields
                .get(&key(name))
                .map(print_term)
                .unwrap_or_else(|| "<missing>".to_string())
        )));
    };
    let mut output = Vec::with_capacity(values.len());
    for value in values {
        let Term::Str(value) = value else {
            return Err(authority_error(format!(
                "{context} {name} entries must be strings"
            )));
        };
        crate::signing::parse_hash32(value)?;
        output.push(value.clone());
    }
    if !output.windows(2).all(|window| window[0] < window[1]) {
        return Err(authority_error(format!(
            "{context} {name} must be strictly sorted and unique"
        )));
    }
    Ok(output)
}

fn validate_transparency_entry(
    term: &Term,
    package_artifact: &str,
    acceptance_artifact: &str,
    signature_artifact: &str,
    public_key_base64: &str,
    previous_head: Option<[u8; 32]>,
) -> Result<(), SigningError> {
    let fields = exact_map(
        term,
        "transparency entry",
        &[
            ":acceptance-artifact",
            ":kind",
            ":package-artifact",
            ":prev-h",
            ":signature-artifact",
            ":signer-pk-b64",
        ],
    )?;
    require_string(
        fields,
        ":kind",
        "transparency entry",
        "genesis/transparency-entry-v0.2",
    )?;
    require_string(
        fields,
        ":package-artifact",
        "transparency entry",
        package_artifact,
    )?;
    require_string(
        fields,
        ":acceptance-artifact",
        "transparency entry",
        acceptance_artifact,
    )?;
    require_string(
        fields,
        ":signature-artifact",
        "transparency entry",
        signature_artifact,
    )?;
    require_string(
        fields,
        ":signer-pk-b64",
        "transparency entry",
        public_key_base64,
    )?;
    match (fields.get(&key(":prev-h")), previous_head) {
        (Some(Term::Nil), None) => Ok(()),
        (Some(Term::Bytes(actual)), Some(expected)) if actual.as_ref() == expected => Ok(()),
        _ => Err(authority_error("transparency entry :prev-h mismatch")),
    }
}

#[allow(clippy::too_many_arguments)]
fn decode_dsse_artifact(
    term: Term,
    payload_type: &str,
    payload: &[u8],
    payload_hash: [u8; 32],
    public_key: [u8; 32],
    key_hash: [u8; 32],
    signature: [u8; 64],
) -> Result<DsseSigningArtifact, SigningError> {
    let fields = exact_map(
        &term,
        "DSSE artifact",
        &[
            ":key-id",
            ":kind",
            ":payload",
            ":payload-sha256",
            ":payload-type",
            ":public-key",
            ":signature",
            ":version",
        ],
    )?;
    let key_id = format!("sha256:{}", hash_hex(key_hash));
    require_string(fields, ":key-id", "DSSE artifact", &key_id)?;
    require_string(
        fields,
        ":kind",
        "DSSE artifact",
        "genesis/genesisbench-dsse-signature-v0.1",
    )?;
    require_string(fields, ":payload-type", "DSSE artifact", payload_type)?;
    require_string(
        fields,
        ":payload-sha256",
        "DSSE artifact",
        &hash_hex(payload_hash),
    )?;
    require_string(fields, ":version", "DSSE artifact", "0.1.0")?;
    if required_bytes(fields, ":payload", "DSSE artifact")? != payload
        || required_bytes(fields, ":public-key", "DSSE artifact")? != public_key
        || required_bytes(fields, ":signature", "DSSE artifact")? != signature
    {
        return Err(authority_error("DSSE artifact byte facts mismatch"));
    }
    Ok(DsseSigningArtifact {
        key_id,
        kind: "genesis/genesisbench-dsse-signature-v0.1".to_string(),
        payload: payload.to_vec(),
        payload_sha256: hash_hex(payload_hash),
        payload_type: payload_type.to_string(),
        public_key,
        signature,
        version: "0.1.0".to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;
    use std::path::PathBuf;

    fn fixture_authority() -> SigningAuthority {
        let artifact = std::env::var_os("GENESIS_TEST_SELFHOST_ARTIFACT")
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                    .join("../..")
                    .join("selfhost/toolchain.gc")
            });
        SigningAuthority::load(SelfhostBootstrapMode::ArtifactOnly, Some(&artifact))
            .expect("load signing authority")
    }

    fn result(request_hash: [u8; 32], extra: Option<(&'static str, Term)>) -> Term {
        let mut fields: BTreeMap<_, _> = [
            (key(":code"), Term::Nil),
            (key(":data"), Term::Map(BTreeMap::new())),
            (key(":kind"), Term::Str(RESULT_KIND.to_string())),
            (key(":message"), Term::Nil),
            (key(":ok"), Term::Bool(true)),
            (key(":request-h"), Term::Str(hash_hex(request_hash))),
            (key(":v"), Term::Int(1.into())),
        ]
        .into_iter()
        .collect();
        if let Some((name, value)) = extra {
            fields.insert(key(name), value);
        }
        Term::Map(fields)
    }

    #[test]
    fn result_decoder_rejects_open_results() {
        let hash = [7; 32];
        let error = decode_result(result(hash, Some((":invented", Term::Bool(true)))), hash)
            .expect_err("authority result must be closed");
        assert!(error.to_string().contains("field set mismatch"));
    }

    #[test]
    fn result_decoder_rejects_unbound_results() {
        let error = decode_result(result([8; 32], None), [9; 32])
            .expect_err("authority result must bind the exact request");
        assert!(error.to_string().contains(":request-h mismatch"));
    }

    #[test]
    fn authority_owns_acceptance_commit_and_dsse_artifacts() {
        let mut authority = fixture_authority();
        authority.keygen([3; 32], true).expect("valid keypair");

        let acceptance = [5; 32];
        let message = authority
            .acceptance_message(acceptance)
            .expect("acceptance message");
        assert_eq!(
            message,
            [b"GCv0.2\0acceptance\0".as_slice(), acceptance.as_slice()].concat()
        );
        let signature_term = authority
            .acceptance_artifact(acceptance, [3; 32], [7; 64], true)
            .expect("acceptance artifact");
        let signature_fields = exact_map(
            &signature_term,
            "signature artifact",
            &[":acceptance-h", ":alg", ":kind", ":pk", ":sig"],
        )
        .expect("closed signature artifact");
        require_string(
            signature_fields,
            ":kind",
            "signature artifact",
            "genesis/acceptance-signature-v0.2",
        )
        .expect("signature kind");

        let low = "00".repeat(32);
        let high = "ff".repeat(32);
        let middle = "77".repeat(32);
        let commit = authority
            .commit(
                &"11".repeat(32),
                &"22".repeat(32),
                &middle,
                "fixture-public-key",
                &[high.clone(), low.clone(), middle.clone()],
                Some([9; 32]),
            )
            .expect("commit authority");
        assert_eq!(commit.signature_set, vec![low, middle, high]);

        let payload = b"{\"canonical\":true}\n";
        let payload_type = "application/vnd.genesiscode.fixture+json";
        let dsse_message = authority
            .dsse_message(payload_type, payload)
            .expect("DSSE message");
        assert_eq!(
            dsse_message,
            format!(
                "DSSEv1 {} {} {} ",
                payload_type.len(),
                payload_type,
                payload.len()
            )
            .into_bytes()
            .into_iter()
            .chain(payload.iter().copied())
            .collect::<Vec<_>>()
        );
        let artifact = authority
            .dsse_artifact(
                payload_type,
                payload,
                [10; 32],
                [3; 32],
                [11; 32],
                [12; 64],
                true,
            )
            .expect("DSSE artifact");
        assert_eq!(artifact.key_id, format!("sha256:{}", "0b".repeat(32)));
        assert_eq!(artifact.payload, payload);
    }

    #[test]
    fn authority_rejects_failed_cryptographic_mechanism_facts() {
        let mut authority = fixture_authority();
        let key_error = authority
            .keygen([3; 32], false)
            .expect_err("invalid keypair must fail closed");
        assert!(key_error.to_string().contains("signing/keypair"));

        let signature_error = authority
            .acceptance_artifact([5; 32], [3; 32], [7; 64], false)
            .expect_err("invalid signature must fail closed");
        assert!(signature_error.to_string().contains("signing/signature"));
    }
}
