use super::*;
use crate::policy::{AuthorizedCryptoPolicy, AuthorizedStringList};

const LIMIT_FIELDS: [(&str, &str); 12] = [
    (":max-aad-bytes", "max_aad_bytes"),
    (":max-ciphertext-bytes", "max_ciphertext_bytes"),
    (":max-context-bytes", "max_context_bytes"),
    (":max-info-bytes", "max_info_bytes"),
    (":max-input-bytes", "max_input_bytes"),
    (":max-message-bytes", "max_message_bytes"),
    (":max-nonce-bytes", "max_nonce_bytes"),
    (":max-output-bytes", "max_output_bytes"),
    (":max-plaintext-bytes", "max_plaintext_bytes"),
    (":max-salt-bytes", "max_salt_bytes"),
    (":max-signature-bytes", "max_signature_bytes"),
    (":max-tag-bytes", "max_tag_bytes"),
];

pub(super) fn input(table: &toml::value::Table) -> Term {
    let mut fields = BTreeMap::from([
        (
            TermOrdKey(Term::symbol(":algorithms")),
            database::string_list_input(table.get("allow_algorithms")),
        ),
        (
            TermOrdKey(Term::symbol(":key-ids")),
            database::string_list_input(table.get("allow_key_ids")),
        ),
    ]);
    for (field, key) in LIMIT_FIELDS {
        fields.insert(
            TermOrdKey(Term::symbol(field)),
            max_bytes_input(table.get(key)),
        );
    }
    Term::Map(fields)
}

fn ascii_lower(value: &str) -> String {
    value.to_ascii_lowercase()
}

pub(super) fn legacy(policy: Option<&OpPolicy>) -> AuthorizedCryptoPolicy {
    let extra = policy.map(|policy| &policy.extra);
    let get = |key| extra.and_then(|extra| extra.get(key));
    let algorithms = match database::legacy_string_list(get("allow_algorithms")) {
        AuthorizedStringList::Valid(values) => {
            AuthorizedStringList::Valid(values.iter().map(|value| ascii_lower(value)).collect())
        }
        state => state,
    };
    AuthorizedCryptoPolicy {
        algorithms,
        key_ids: database::legacy_string_list(get("allow_key_ids")),
        max_aad_bytes: database::legacy_positive(get("max_aad_bytes")),
        max_ciphertext_bytes: database::legacy_positive(get("max_ciphertext_bytes")),
        max_context_bytes: database::legacy_positive(get("max_context_bytes")),
        max_info_bytes: database::legacy_positive(get("max_info_bytes")),
        max_input_bytes: database::legacy_positive(get("max_input_bytes")),
        max_message_bytes: database::legacy_positive(get("max_message_bytes")),
        max_nonce_bytes: database::legacy_positive(get("max_nonce_bytes")),
        max_output_bytes: database::legacy_positive(get("max_output_bytes")),
        max_plaintext_bytes: database::legacy_positive(get("max_plaintext_bytes")),
        max_salt_bytes: database::legacy_positive(get("max_salt_bytes")),
        max_signature_bytes: database::legacy_positive(get("max_signature_bytes")),
        max_tag_bytes: database::legacy_positive(get("max_tag_bytes")),
    }
}

pub(super) fn decode(term: &Term, allowed: bool) -> Result<AuthorizedCryptoPolicy, EffectsError> {
    if !allowed {
        return if term == &Term::Nil {
            Ok(legacy(None))
        } else {
            Err(authority_error("denied result :crypto-policy must be nil"))
        };
    }
    let Term::Map(map) = term else {
        return Err(authority_error(
            "admitted result :crypto-policy must be a data map",
        ));
    };
    let mut expected: BTreeSet<_> = [":algorithms", ":key-ids"]
        .into_iter()
        .map(|key| TermOrdKey(Term::symbol(key)))
        .collect();
    expected.extend(
        LIMIT_FIELDS
            .iter()
            .map(|(field, _)| TermOrdKey(Term::symbol(*field))),
    );
    if map.keys().cloned().collect::<BTreeSet<_>>() != expected {
        return Err(authority_error("result :crypto-policy field set mismatch"));
    }
    let field = |key: &str| {
        map.get(&TermOrdKey(Term::symbol(key)))
            .ok_or_else(|| authority_error(format!("result :crypto-policy is missing {key}")))
    };
    let algorithms = database::decode_string_list(field(":algorithms")?, ":algorithms")?;
    if let AuthorizedStringList::Valid(values) = &algorithms
        && values
            .iter()
            .any(|value| value != &value.to_ascii_lowercase())
    {
        return Err(authority_error(
            "result :algorithms values must be canonical ASCII lowercase",
        ));
    }
    Ok(AuthorizedCryptoPolicy {
        algorithms,
        key_ids: database::decode_string_list(field(":key-ids")?, ":key-ids")?,
        max_aad_bytes: decode_max_bytes_policy(field(":max-aad-bytes")?, true)?,
        max_ciphertext_bytes: decode_max_bytes_policy(field(":max-ciphertext-bytes")?, true)?,
        max_context_bytes: decode_max_bytes_policy(field(":max-context-bytes")?, true)?,
        max_info_bytes: decode_max_bytes_policy(field(":max-info-bytes")?, true)?,
        max_input_bytes: decode_max_bytes_policy(field(":max-input-bytes")?, true)?,
        max_message_bytes: decode_max_bytes_policy(field(":max-message-bytes")?, true)?,
        max_nonce_bytes: decode_max_bytes_policy(field(":max-nonce-bytes")?, true)?,
        max_output_bytes: decode_max_bytes_policy(field(":max-output-bytes")?, true)?,
        max_plaintext_bytes: decode_max_bytes_policy(field(":max-plaintext-bytes")?, true)?,
        max_salt_bytes: decode_max_bytes_policy(field(":max-salt-bytes")?, true)?,
        max_signature_bytes: decode_max_bytes_policy(field(":max-signature-bytes")?, true)?,
        max_tag_bytes: decode_max_bytes_policy(field(":max-tag-bytes")?, true)?,
    })
}
