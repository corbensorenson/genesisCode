use super::*;

const HAS_REQUEST_KIND: &str = "genesis/store-has-authority-request-v0.1";
const HAS_RESULT_KIND: &str = "genesis/store-has-authority-result-v0.1";
const GET_REQUEST_KIND: &str = "genesis/store-get-authority-request-v0.1";
const GET_RESULT_KIND: &str = "genesis/store-get-authority-result-v0.1";

pub(crate) enum StoreHasDecision {
    ObserveLocal { hash: String },
    FetchRemote { hash: String },
    Return { present: bool },
    Error { code: String, message: String },
}

pub(crate) enum StoreGetDecision {
    ObserveLocal {
        hash: String,
    },
    FetchRemote {
        hash: String,
    },
    Return {
        artifact: Term,
        hash: String,
    },
    CacheReturn {
        artifact: Term,
        bytes: Vec<u8>,
        hash: String,
        written_bytes: usize,
    },
    Error {
        code: String,
        message: String,
    },
}

impl StoreAuthority {
    pub(crate) fn has(
        &mut self,
        payload: &Term,
        status: &str,
        remote_enabled: bool,
        local_hash: Option<&str>,
        remote_present: Option<bool>,
        mechanism_message: Option<&str>,
    ) -> Result<StoreHasDecision, EffectsError> {
        let request = map([
            (":kind", Term::Str(HAS_REQUEST_KIND.to_string())),
            (":local-hash", local_hash.map(str_term).unwrap_or(Term::Nil)),
            (
                ":mechanism-message",
                mechanism_message.map(str_term).unwrap_or(Term::Nil),
            ),
            (":payload", payload.clone()),
            (":phase", Term::symbol(":has")),
            (":remote-enabled", Term::Bool(remote_enabled)),
            (
                ":remote-present",
                remote_present.map(Term::Bool).unwrap_or(Term::Nil),
            ),
            (":status", Term::symbol(status)),
            (":v", Term::Int(1.into())),
        ]);
        let (term, request_hash) = self.evaluate(request)?;
        decode_has_result(term, request_hash)
    }

    #[expect(
        clippy::too_many_arguments,
        reason = "closed get request carries all bounded raw observations explicitly"
    )]
    pub(crate) fn get(
        &mut self,
        payload: &Term,
        status: &str,
        bytes: Option<&[u8]>,
        remote_enabled: bool,
        mechanism_message: Option<&str>,
        max_bytes: usize,
        budget_used: usize,
        budget_limit: Option<usize>,
    ) -> Result<StoreGetDecision, EffectsError> {
        let request = map([
            (
                ":budget-limit",
                budget_limit
                    .map(|value| Term::Int(value.into()))
                    .unwrap_or(Term::Nil),
            ),
            (":budget-used", Term::Int(budget_used.into())),
            (
                ":bytes",
                bytes
                    .map(|value| Term::Bytes(value.to_vec().into()))
                    .unwrap_or(Term::Nil),
            ),
            (":kind", Term::Str(GET_REQUEST_KIND.to_string())),
            (":max-bytes", Term::Int(max_bytes.into())),
            (
                ":mechanism-message",
                mechanism_message.map(str_term).unwrap_or(Term::Nil),
            ),
            (":payload", payload.clone()),
            (":phase", Term::symbol(":get")),
            (":remote-enabled", Term::Bool(remote_enabled)),
            (":status", Term::symbol(status)),
            (":v", Term::Int(1.into())),
        ]);
        let (term, request_hash) = self.evaluate(request)?;
        decode_get_result(term, request_hash)
    }
}

fn str_term(value: &str) -> Term {
    Term::Str(value.to_string())
}

fn decode_has_result(term: Term, request_hash: [u8; 32]) -> Result<StoreHasDecision, EffectsError> {
    let fields = exact_map(
        &term,
        &[
            ":action",
            ":code",
            ":hash",
            ":kind",
            ":message",
            ":ok",
            ":present",
            ":request-h",
            ":v",
        ],
    )?;
    check_envelope(fields, HAS_RESULT_KIND, request_hash)?;
    let action = required_symbol(fields, ":action")?;
    match action.as_str() {
        ":observe-local" => {
            require_nil(fields, ":code")?;
            require_nil(fields, ":message")?;
            require_nil(fields, ":present")?;
            Ok(StoreHasDecision::ObserveLocal {
                hash: checked_hash(fields)?,
            })
        }
        ":fetch-remote" => {
            require_nil(fields, ":code")?;
            require_nil(fields, ":message")?;
            require_nil(fields, ":present")?;
            Ok(StoreHasDecision::FetchRemote {
                hash: checked_hash(fields)?,
            })
        }
        ":return" => {
            require_nil(fields, ":code")?;
            require_nil(fields, ":hash")?;
            require_nil(fields, ":message")?;
            Ok(StoreHasDecision::Return {
                present: required_bool(fields, ":present")?,
            })
        }
        ":error" => {
            require_nil(fields, ":hash")?;
            require_nil(fields, ":present")?;
            Ok(StoreHasDecision::Error {
                code: required_string(fields, ":code")?.to_string(),
                message: required_string(fields, ":message")?.to_string(),
            })
        }
        _ => Err(authority_error(format!("unsupported has action {action}"))),
    }
}

fn decode_get_result(term: Term, request_hash: [u8; 32]) -> Result<StoreGetDecision, EffectsError> {
    let fields = exact_map(
        &term,
        &[
            ":action",
            ":artifact",
            ":bytes",
            ":code",
            ":hash",
            ":kind",
            ":message",
            ":ok",
            ":request-h",
            ":v",
            ":written-bytes",
        ],
    )?;
    check_envelope(fields, GET_RESULT_KIND, request_hash)?;
    let action = required_symbol(fields, ":action")?;
    match action.as_str() {
        ":observe-local" => {
            require_get_empty(fields)?;
            Ok(StoreGetDecision::ObserveLocal {
                hash: checked_hash(fields)?,
            })
        }
        ":fetch-remote" => {
            require_get_empty(fields)?;
            Ok(StoreGetDecision::FetchRemote {
                hash: checked_hash(fields)?,
            })
        }
        ":return" => {
            require_nil(fields, ":bytes")?;
            require_nil(fields, ":code")?;
            require_nil(fields, ":message")?;
            require_nil(fields, ":written-bytes")?;
            Ok(StoreGetDecision::Return {
                artifact: field(fields, ":artifact")?.clone(),
                hash: checked_hash(fields)?,
            })
        }
        ":cache-return" => {
            require_nil(fields, ":code")?;
            require_nil(fields, ":message")?;
            let bytes = required_bytes(fields, ":bytes")?;
            let hash = checked_hash(fields)?;
            let written_bytes = required_usize(fields, ":written-bytes")?;
            if written_bytes != bytes.len() {
                return Err(authority_error("cache byte count contradiction"));
            }
            if blake3::hash(&bytes).to_hex().as_str() != hash {
                return Err(authority_error("cache hash/bytes contradiction"));
            }
            Ok(StoreGetDecision::CacheReturn {
                artifact: field(fields, ":artifact")?.clone(),
                bytes,
                hash,
                written_bytes,
            })
        }
        ":error" => {
            require_nil(fields, ":artifact")?;
            require_nil(fields, ":bytes")?;
            require_nil(fields, ":hash")?;
            require_nil(fields, ":written-bytes")?;
            Ok(StoreGetDecision::Error {
                code: required_string(fields, ":code")?.to_string(),
                message: required_string(fields, ":message")?.to_string(),
            })
        }
        _ => Err(authority_error(format!("unsupported get action {action}"))),
    }
}

fn check_envelope(
    fields: &BTreeMap<TermOrdKey, Term>,
    kind: &str,
    request_hash: [u8; 32],
) -> Result<(), EffectsError> {
    require_string(fields, ":kind", kind)?;
    require_int(fields, ":v", 1)?;
    require_string(fields, ":request-h", &hex32(request_hash))?;
    if required_bool(fields, ":ok")? {
        return Ok(());
    }
    let code = optional_string(fields, ":code")?.unwrap_or("store/authority-request");
    let message = optional_string(fields, ":message")?.unwrap_or("authority rejected request");
    Err(authority_error(format!("{code}: {message}")))
}

fn checked_hash(fields: &BTreeMap<TermOrdKey, Term>) -> Result<String, EffectsError> {
    let hash = required_string(fields, ":hash")?;
    if hash.len() == 64
        && hash
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        Ok(hash.to_string())
    } else {
        Err(authority_error("result hash must be lowercase hex64"))
    }
}

fn require_get_empty(fields: &BTreeMap<TermOrdKey, Term>) -> Result<(), EffectsError> {
    require_nil(fields, ":artifact")?;
    require_nil(fields, ":bytes")?;
    require_nil(fields, ":code")?;
    require_nil(fields, ":message")?;
    require_nil(fields, ":written-bytes")
}
