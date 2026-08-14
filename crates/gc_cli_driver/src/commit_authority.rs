use super::*;

const BINDING: &str = "core/commit::authority";
const REQUEST_KIND: &str = "genesis/commit-authority-request-v0.1";
const RESULT_KIND: &str = "genesis/commit-authority-result-v0.1";

pub(super) fn make(cli: &Cli, payload: Term) -> Result<Term, CliError> {
    evaluate(cli, ":make", payload, "commit/new")
}

pub(super) fn validate(cli: &Cli, artifact: Term, command: &str) -> Result<Term, CliError> {
    evaluate(
        cli,
        ":validate",
        Term::Map(
            [(TermOrdKey(Term::symbol(":artifact")), artifact)]
                .into_iter()
                .collect(),
        ),
        command,
    )
}

fn evaluate(cli: &Cli, op: &str, payload: Term, command: &str) -> Result<Term, CliError> {
    let request = Term::Map(
        [
            (
                TermOrdKey(Term::symbol(":kind")),
                Term::Str(REQUEST_KIND.to_string()),
            ),
            (TermOrdKey(Term::symbol(":op")), Term::symbol(op)),
            (TermOrdKey(Term::symbol(":payload")), payload),
            (TermOrdKey(Term::symbol(":v")), Term::Int(1.into())),
        ]
        .into_iter()
        .collect(),
    );
    let request_hash = hex32(gc_coreform::hash_term(&request));
    let mut context = mk_ctx(cli);
    let prelude = build_prelude(&mut context);
    let mut environment = prelude.env;
    load_selfhost_toolchain(cli, &mut context, &mut environment)?;
    let authority = environment.get(BINDING).ok_or_else(|| {
        cli_err(
            EX_INTERNAL,
            "selfhost/missing",
            format!("missing binding {BINDING}"),
        )
    })?;
    let value = authority
        .apply(&mut context, Value::data(request))
        .map_err(|error| {
            cli_err_with_context(
                EX_EVAL,
                "eval/error",
                format!("{BINDING} failed for {command}: {error}"),
                structured_failures::evaluator_context("commit/authority", &error),
            )
        })?;
    if let Some((code, message, _payload)) = extract_protocol_error(&context, &value) {
        return Err(authority_error(format!(
            "{BINDING} returned sealed error for {command}: {code}: {message}"
        )));
    }
    decode_result(value, &request_hash, command)
}

fn decode_result(value: Value, request_hash: &str, command: &str) -> Result<Term, CliError> {
    let Some(Term::Map(fields)) = value.to_plain_term() else {
        return Err(authority_error(format!(
            "{BINDING} returned non-map for {command}: {}",
            value.debug_repr()
        )));
    };
    let expected = [
        ":artifact",
        ":code",
        ":kind",
        ":message",
        ":ok",
        ":request-h",
        ":v",
    ]
    .into_iter()
    .map(|name| TermOrdKey(Term::symbol(name)))
    .collect::<BTreeSet<_>>();
    if fields.keys().cloned().collect::<BTreeSet<_>>() != expected {
        return Err(authority_error("result field set mismatch"));
    }
    require_string(&fields, ":kind", RESULT_KIND)?;
    require_int(&fields, ":v", 1)?;
    require_string(&fields, ":request-h", request_hash)?;
    let ok = required_bool(&fields, ":ok")?;
    if !ok {
        require_nil(&fields, ":artifact")?;
        let code = required_string(&fields, ":code")?;
        let message = required_string(&fields, ":message")?;
        return Err(cli_err(
            EX_PARSE,
            "selfhost/error",
            format!("{command}: {code}: {message}"),
        ));
    }
    require_nil(&fields, ":code")?;
    require_nil(&fields, ":message")?;
    match field(&fields, ":artifact")? {
        artifact @ Term::Map(_) => Ok(artifact.clone()),
        _ => Err(authority_error("successful result artifact must be a map")),
    }
}

fn authority_error(message: impl Into<String>) -> CliError {
    cli_err(EX_INTERNAL, "selfhost/bad-return", message)
}

fn field<'a>(fields: &'a BTreeMap<TermOrdKey, Term>, name: &str) -> Result<&'a Term, CliError> {
    fields
        .get(&TermOrdKey(Term::symbol(name)))
        .ok_or_else(|| authority_error(format!("result missing {name}")))
}

fn require_string(
    fields: &BTreeMap<TermOrdKey, Term>,
    name: &str,
    expected: &str,
) -> Result<(), CliError> {
    match field(fields, name)? {
        Term::Str(value) if value == expected => Ok(()),
        _ => Err(authority_error(format!("result {name} mismatch"))),
    }
}

fn required_string<'a>(
    fields: &'a BTreeMap<TermOrdKey, Term>,
    name: &str,
) -> Result<&'a str, CliError> {
    match field(fields, name)? {
        Term::Str(value) => Ok(value),
        _ => Err(authority_error(format!("result {name} must be string"))),
    }
}

fn require_int(
    fields: &BTreeMap<TermOrdKey, Term>,
    name: &str,
    expected: i64,
) -> Result<(), CliError> {
    match field(fields, name)? {
        Term::Int(value) if value.to_string() == expected.to_string() => Ok(()),
        _ => Err(authority_error(format!("result {name} mismatch"))),
    }
}

fn required_bool(fields: &BTreeMap<TermOrdKey, Term>, name: &str) -> Result<bool, CliError> {
    match field(fields, name)? {
        Term::Bool(value) => Ok(*value),
        _ => Err(authority_error(format!("result {name} must be bool"))),
    }
}

fn require_nil(fields: &BTreeMap<TermOrdKey, Term>, name: &str) -> Result<(), CliError> {
    match field(fields, name)? {
        Term::Nil => Ok(()),
        _ => Err(authority_error(format!("result {name} must be nil"))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gc_kernel::ValueMap;

    fn result(
        fields: impl IntoIterator<Item = (&'static str, Term)>,
    ) -> BTreeMap<TermOrdKey, Term> {
        fields
            .into_iter()
            .map(|(name, value)| (TermOrdKey(Term::symbol(name)), value))
            .collect()
    }

    #[test]
    fn strict_decoder_rejects_open_and_unbound_results() {
        let open = result([
            (":artifact", Term::Map(BTreeMap::new())),
            (":code", Term::Nil),
            (":kind", Term::Str(RESULT_KIND.to_string())),
            (":message", Term::Nil),
            (":ok", Term::Bool(true)),
            (":request-h", Term::Str("0".repeat(64))),
            (":v", Term::Int(1.into())),
            (":extra", Term::Nil),
        ]);
        assert!(decode_result(Value::data(Term::Map(open)), &"0".repeat(64), "test").is_err());

        let unbound = result([
            (":artifact", Term::Map(BTreeMap::new())),
            (":code", Term::Nil),
            (":kind", Term::Str(RESULT_KIND.to_string())),
            (":message", Term::Nil),
            (":ok", Term::Bool(true)),
            (":request-h", Term::Str("0".repeat(64))),
            (":v", Term::Int(1.into())),
        ]);
        assert!(decode_result(Value::data(Term::Map(unbound)), &"1".repeat(64), "test").is_err());
    }

    #[test]
    fn strict_decoder_accepts_runtime_map_results() {
        let mut result = ValueMap::new();
        result.insert_mut(
            TermOrdKey(Term::symbol(":artifact")),
            Value::data(Term::Map(BTreeMap::new())),
        );
        result.insert_mut(TermOrdKey(Term::symbol(":code")), Value::data(Term::Nil));
        result.insert_mut(
            TermOrdKey(Term::symbol(":kind")),
            Value::data(Term::Str(RESULT_KIND.to_string())),
        );
        result.insert_mut(TermOrdKey(Term::symbol(":message")), Value::data(Term::Nil));
        result.insert_mut(
            TermOrdKey(Term::symbol(":ok")),
            Value::data(Term::Bool(true)),
        );
        result.insert_mut(
            TermOrdKey(Term::symbol(":request-h")),
            Value::data(Term::Str("0".repeat(64))),
        );
        result.insert_mut(TermOrdKey(Term::symbol(":v")), Value::int(1));

        assert!(matches!(
            decode_result(Value::map(result), &"0".repeat(64), "test"),
            Ok(Term::Map(_))
        ));
    }
}
