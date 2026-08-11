use super::*;

pub(super) fn summarize_protocol_error_payload(payload: &Value) -> String {
    let Some(term) = payload.as_data() else {
        return payload.debug_repr();
    };
    match term {
        Term::Map(map) => {
            let code = map
                .get(&TermOrdKey(Term::symbol(":error/code")))
                .and_then(|term| match term {
                    Term::Str(value) => Some(value.as_str()),
                    _ => None,
                })
                .unwrap_or("core/error");
            let message = map
                .get(&TermOrdKey(Term::symbol(":error/message")))
                .and_then(|term| match term {
                    Term::Str(value) => Some(value.as_str()),
                    _ => None,
                })
                .unwrap_or("error");
            format!("{code}: {message}")
        }
        _ => print_term(term),
    }
}

pub(super) fn extract_protocol_error(out: &Value, error_token: SealId) -> Option<String> {
    match out {
        Value::Sealed { token, payload } if *token == error_token => {
            Some(summarize_protocol_error_payload(payload))
        }
        _ => None,
    }
}
