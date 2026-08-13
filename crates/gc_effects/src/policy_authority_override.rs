use super::*;

fn optional_bool(value: Option<&toml::Value>) -> Term {
    value
        .and_then(toml::Value::as_bool)
        .map(Term::Bool)
        .unwrap_or(Term::Nil)
}

fn optional_int(value: Option<&toml::Value>) -> Term {
    value
        .and_then(toml::Value::as_integer)
        .map(|number| Term::Int(number.into()))
        .unwrap_or(Term::Nil)
}

fn optional_str(value: Option<&toml::Value>) -> Term {
    value
        .and_then(toml::Value::as_str)
        .map(|text| Term::Str(text.to_string()))
        .unwrap_or(Term::Nil)
}

pub(super) fn term(op: &str, value: Option<&toml::Value>) -> Result<Term, EffectsError> {
    let Some(value) = value else {
        return Ok(Term::Nil);
    };
    let table = value
        .as_table()
        .ok_or_else(|| authority_error("operation override must be a table"))?;
    Ok(Term::Map(
        [
            (
                TermOrdKey(Term::symbol(":allow")),
                optional_bool(table.get("allow")),
            ),
            (
                TermOrdKey(Term::symbol(":base-dir")),
                optional_str(table.get("base_dir")),
            ),
            (
                TermOrdKey(Term::symbol(":create-dirs")),
                optional_bool(table.get("create_dirs")),
            ),
            (
                TermOrdKey(Term::symbol(":bridge-identity-policy")),
                bridge::input(table),
            ),
            (
                TermOrdKey(Term::symbol(":credential-policy")),
                if store_credentials::operation_applies(op) {
                    store_credentials::input(Some(table))
                } else {
                    Term::Nil
                },
            ),
            (
                TermOrdKey(Term::symbol(":crypto-policy")),
                crypto::input(table),
            ),
            (
                TermOrdKey(Term::symbol(":database-policy")),
                database::input(table),
            ),
            (TermOrdKey(Term::symbol(":ffi-policy")), ffi::input(table)),
            (
                TermOrdKey(Term::symbol(":log-inline-max-bytes")),
                optional_int(table.get("log_inline_max_bytes")),
            ),
            (
                TermOrdKey(Term::symbol(":max-bytes")),
                max_bytes_input(table.get("max_bytes")),
            ),
            (
                TermOrdKey(Term::symbol(":network-policy")),
                network::input(table),
            ),
            (
                TermOrdKey(Term::symbol(":plugin-policy")),
                plugin::input(table),
            ),
            (
                TermOrdKey(Term::symbol(":process-programs")),
                process::input(table.get("allow_programs")),
            ),
            (
                TermOrdKey(Term::symbol(":timeout-ms")),
                optional_int(table.get("timeout_ms")),
            ),
        ]
        .into_iter()
        .collect(),
    ))
}
