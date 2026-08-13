use super::*;

pub(super) fn decode(
    cap: &Term,
    op: &str,
) -> Result<(bool, Option<u64>, Option<usize>), EffectsError> {
    let Term::Map(map) = cap else {
        return Err(authority_error("result :cap must be a data map"));
    };
    let allowed_keys: BTreeSet<_> = [
        ":create-dirs",
        ":log-inline-max-bytes",
        ":op",
        ":timeout-ms",
    ]
    .into_iter()
    .map(|key| TermOrdKey(Term::symbol(key)))
    .collect();
    if map.keys().any(|key| !allowed_keys.contains(key)) {
        return Err(authority_error("result :cap field set mismatch"));
    }
    if !matches!(map.get(&TermOrdKey(Term::symbol(":op"))), Some(Term::Symbol(actual)) if actual == op)
    {
        return Err(authority_error("result :cap operation mismatch"));
    }
    let create_dirs = match map.get(&TermOrdKey(Term::symbol(":create-dirs"))) {
        None => false,
        Some(Term::Bool(true)) => true,
        _ => {
            return Err(authority_error(
                "result :cap :create-dirs must be omitted or true",
            ));
        }
    };
    let timeout_ms = match map.get(&TermOrdKey(Term::symbol(":timeout-ms"))) {
        None => None,
        Some(Term::Int(value)) => value
            .to_u64()
            .map(Some)
            .ok_or_else(|| authority_error("result :cap :timeout-ms must fit a nonnegative u64"))?,
        _ => {
            return Err(authority_error(
                "result :cap :timeout-ms must be an integer",
            ));
        }
    };
    let log_inline_max_bytes = match map.get(&TermOrdKey(Term::symbol(":log-inline-max-bytes"))) {
        None => None,
        Some(Term::Int(value)) => {
            let limit = value.to_usize().ok_or_else(|| {
                authority_error(
                    "result :cap :log-inline-max-bytes must fit a positive platform usize",
                )
            })?;
            if limit == 0 {
                return Err(authority_error(
                    "result :cap :log-inline-max-bytes must be positive",
                ));
            }
            Some(limit)
        }
        _ => {
            return Err(authority_error(
                "result :cap :log-inline-max-bytes must be an integer",
            ));
        }
    };
    Ok((create_dirs, timeout_ms, log_inline_max_bytes))
}
