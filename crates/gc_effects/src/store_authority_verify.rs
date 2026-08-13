use super::*;

const VERIFY_REQUEST_KIND: &str = "genesis/store-verify-authority-request-v0.1";
const VERIFY_RESULT_KIND: &str = "genesis/store-verify-authority-result-v0.1";

pub(crate) enum StoreVerifyDecision {
    ObserveInventory,
    ObserveHashes {
        hashes: Vec<String>,
    },
    Return {
        checked: usize,
        hash: Option<String>,
    },
    Error {
        code: String,
        message: String,
        hash: Option<String>,
        checked: usize,
    },
}

impl StoreAuthority {
    #[expect(
        clippy::too_many_arguments,
        reason = "closed verify request transports bounded raw inventory and observation state"
    )]
    pub(crate) fn verify(
        &mut self,
        payload: &Term,
        status: &str,
        entries: Option<Term>,
        hashes: Option<&[String]>,
        observations: Option<Term>,
        max_entries: usize,
        max_bytes: usize,
        max_total_bytes: usize,
    ) -> Result<StoreVerifyDecision, EffectsError> {
        let request = map([
            (":entries", entries.unwrap_or(Term::Nil)),
            (
                ":hashes",
                hashes
                    .map(|values| Term::Vector(values.iter().cloned().map(Term::Str).collect()))
                    .unwrap_or(Term::Nil),
            ),
            (":kind", Term::Str(VERIFY_REQUEST_KIND.to_string())),
            (":max-bytes", Term::Int(max_bytes.into())),
            (":max-entries", Term::Int(max_entries.into())),
            (":max-total-bytes", Term::Int(max_total_bytes.into())),
            (":observations", observations.unwrap_or(Term::Nil)),
            (":payload", payload.clone()),
            (":phase", Term::symbol(":verify")),
            (":status", Term::symbol(status)),
            (":v", Term::Int(1.into())),
        ]);
        let (term, request_hash) = self.evaluate_verify(request)?;
        decode_verify_result(term, request_hash)
    }
}

fn decode_verify_result(
    term: Term,
    request_hash: [u8; 32],
) -> Result<StoreVerifyDecision, EffectsError> {
    let fields = exact_map(
        &term,
        &[
            ":action",
            ":checked",
            ":code",
            ":hash",
            ":hashes",
            ":kind",
            ":message",
            ":ok",
            ":request-h",
            ":v",
        ],
    )?;
    require_string(fields, ":kind", VERIFY_RESULT_KIND)?;
    require_int(fields, ":v", 1)?;
    require_string(fields, ":request-h", &hex32(request_hash))?;
    if !required_bool(fields, ":ok")? {
        let code = optional_string(fields, ":code")?.unwrap_or("store/verify-authority-request");
        let message = optional_string(fields, ":message")?.unwrap_or("authority rejected request");
        return Err(authority_error(format!("{code}: {message}")));
    }
    let action = required_symbol(fields, ":action")?;
    match action.as_str() {
        ":observe-inventory" => {
            require_verify_empty(fields)?;
            Ok(StoreVerifyDecision::ObserveInventory)
        }
        ":observe-hashes" => {
            require_nil(fields, ":checked")?;
            require_nil(fields, ":code")?;
            require_nil(fields, ":hash")?;
            require_nil(fields, ":message")?;
            Ok(StoreVerifyDecision::ObserveHashes {
                hashes: required_hashes(fields, ":hashes")?,
            })
        }
        ":return" => {
            require_nil(fields, ":code")?;
            require_nil(fields, ":hashes")?;
            require_nil(fields, ":message")?;
            Ok(StoreVerifyDecision::Return {
                checked: required_usize(fields, ":checked")?,
                hash: optional_checked_hash(fields, ":hash")?,
            })
        }
        ":error" => {
            require_nil(fields, ":hashes")?;
            Ok(StoreVerifyDecision::Error {
                code: required_string(fields, ":code")?.to_string(),
                message: required_string(fields, ":message")?.to_string(),
                hash: optional_checked_hash(fields, ":hash")?,
                checked: required_usize(fields, ":checked")?,
            })
        }
        _ => Err(authority_error(format!(
            "unsupported verify action {action}"
        ))),
    }
}

fn require_verify_empty(fields: &BTreeMap<TermOrdKey, Term>) -> Result<(), EffectsError> {
    require_nil(fields, ":checked")?;
    require_nil(fields, ":code")?;
    require_nil(fields, ":hash")?;
    require_nil(fields, ":hashes")?;
    require_nil(fields, ":message")
}

fn required_hashes(
    fields: &BTreeMap<TermOrdKey, Term>,
    name: &str,
) -> Result<Vec<String>, EffectsError> {
    let Term::Vector(values) = field(fields, name)? else {
        return Err(authority_error(format!("result {name} must be vector")));
    };
    let mut out = Vec::with_capacity(values.len());
    for value in values {
        let Term::Str(hash) = value else {
            return Err(authority_error(format!(
                "result {name} entries must be strings, got {}",
                print_term(value)
            )));
        };
        if !lower_hex64(hash) {
            return Err(authority_error(format!(
                "result {name} entries must be lowercase hex64"
            )));
        }
        if out.last().is_some_and(|previous| previous >= hash) {
            return Err(authority_error(format!(
                "result {name} must be strictly sorted and unique"
            )));
        }
        out.push(hash.clone());
    }
    Ok(out)
}

fn optional_checked_hash(
    fields: &BTreeMap<TermOrdKey, Term>,
    name: &str,
) -> Result<Option<String>, EffectsError> {
    match optional_string(fields, name)? {
        None => Ok(None),
        Some(hash) if lower_hex64(hash) => Ok(Some(hash.to_string())),
        Some(_) => Err(authority_error(format!(
            "result {name} must be lowercase hex64 or nil"
        ))),
    }
}

fn lower_hex64(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}
