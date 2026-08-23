use std::collections::{BTreeMap, BTreeSet};
use std::io::Read;
use std::path::Path;

use gc_coreform::{Term, TermOrdKey, hash_term};
use gc_kernel::{Apply, Env, EvalCtx, Value};

const AUTHORITY_BINDING: &str = "core/pkg::lock-model-authority";
const REQUEST_KIND: &str = "genesis/pkg-lock-model-authority-request-v0.1";
const RESULT_KIND: &str = "genesis/pkg-lock-model-authority-result-v0.1";
const SOURCE_LIMIT: usize = 4 * 1024 * 1024;
const COLLECTION_LIMIT: usize = 65_536;
const VALUE_LIMIT: usize = 4 * 1024 * 1024;

pub(crate) fn read_bounded(path: &Path) -> Result<Vec<u8>, String> {
    let file = std::fs::File::open(path)
        .map_err(|error| format!("read lock model {}: {error}", path.display()))?;
    let mut bytes = Vec::new();
    file.take((SOURCE_LIMIT as u64) + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| format!("read lock model {}: {error}", path.display()))?;
    if bytes.len() > SOURCE_LIMIT {
        return Err(format!(
            "lock file exceeds {SOURCE_LIMIT}-byte transport limit"
        ));
    }
    Ok(bytes)
}

pub(crate) fn authorize_bytes(
    context: &mut EvalCtx,
    environment: &Env,
    bytes: &[u8],
) -> Result<Term, String> {
    if bytes.len() > SOURCE_LIMIT {
        return Err(format!(
            "lock file exceeds {SOURCE_LIMIT}-byte transport limit"
        ));
    }
    let source =
        std::str::from_utf8(bytes).map_err(|_| "lock file is not valid UTF-8".to_string())?;
    let document = toml::from_str::<toml::Value>(source)
        .map_err(|_| "lock file is not valid TOML".to_string())?;
    let request = map([
        (":document", toml_to_term(document)),
        (":kind", Term::Str(REQUEST_KIND.to_string())),
        (":op", Term::symbol(":read-model")),
        (":v", Term::Int(1.into())),
    ]);
    let request_hash = hex32(hash_term(&request));
    let authority = environment
        .get(AUTHORITY_BINDING)
        .ok_or_else(|| format!("missing binding {AUTHORITY_BINDING}"))?;
    let value = authority
        .apply(context, Value::data(request))
        .map_err(|error| format!("{AUTHORITY_BINDING} failed: {error}"))?;
    if let Some((code, message, _)) = crate::extract_protocol_error(context, &value) {
        return Err(format!(
            "{AUTHORITY_BINDING} returned sealed error: {code}: {message}"
        ));
    }
    decode(value, &request_hash)
}

fn decode(value: Value, request_hash: &str) -> Result<Term, String> {
    let Some(Term::Map(envelope)) = value.to_plain_term() else {
        return Err("lock-model authority returned non-map".to_string());
    };
    require_exact_fields(
        &envelope,
        &[
            ":code",
            ":kind",
            ":message",
            ":model",
            ":ok",
            ":request-h",
            ":v",
        ],
        "lock-model envelope",
    )?;
    require_string(&envelope, ":kind", RESULT_KIND)?;
    require_string(&envelope, ":request-h", request_hash)?;
    require_int(&envelope, ":v", 1)?;
    match field(&envelope, ":ok")? {
        Term::Bool(false) => {
            require_nil(&envelope, ":model")?;
            let code = required_string(&envelope, ":code")?;
            if !matches!(code, "core/pkg/bad-lock" | "core/pkg/bad-authority-request") {
                return Err("lock-model authority returned an unknown error code".to_string());
            }
            Err(format!(
                "{code}: {}",
                required_string(&envelope, ":message")?
            ))
        }
        Term::Bool(true) => {
            require_nil(&envelope, ":code")?;
            require_nil(&envelope, ":message")?;
            let model = field(&envelope, ":model")?.clone();
            validate_model(&model)?;
            Ok(model)
        }
        _ => Err("lock-model envelope :ok must be bool".to_string()),
    }
}

fn validate_model(model: &Term) -> Result<(), String> {
    let fields = required_map(model, "lock model")?;
    require_exact_fields(
        fields,
        &[
            ":artifacts",
            ":locked",
            ":policy",
            ":registries",
            ":requirements",
            ":version",
            ":workspace",
        ],
        "lock model",
    )?;
    match field(fields, ":version")? {
        Term::Int(value) if value == &1.into() || value == &2.into() => {}
        _ => return Err("lock model :version must be 1 or 2".to_string()),
    }
    bounded_string(fields, ":workspace")?;
    bounded_string(fields, ":policy")?;
    validate_string_map(field(fields, ":registries")?, "lock registries")?;
    validate_string_map(field(fields, ":artifacts")?, "lock artifacts")?;
    validate_requirements(field(fields, ":requirements")?)?;
    validate_locked(field(fields, ":locked")?)
}

fn validate_string_map(term: &Term, label: &str) -> Result<(), String> {
    for (key, value) in bounded_map(term, label)? {
        let (Term::Str(key), Term::Str(value)) = (&key.0, value) else {
            return Err(format!("{label} entries must be string/string"));
        };
        bounded(key, label)?;
        bounded(value, label)?;
    }
    Ok(())
}

fn validate_requirements(term: &Term) -> Result<(), String> {
    for (key, value) in bounded_map(term, "lock requirements")? {
        let Term::Str(name) = &key.0 else {
            return Err("lock requirement names must be strings".to_string());
        };
        bounded(name, "lock requirement name")?;
        let fields = required_map(value, "lock requirement")?;
        require_exact_fields(
            fields,
            &[
                ":registry",
                ":selector",
                ":strategy",
                ":tag-policy",
                ":update-policy",
            ],
            "lock requirement",
        )?;
        optional_string(fields, ":registry")?;
        bounded_string(fields, ":selector")?;
        optional_string(fields, ":tag-policy")?;
        require_symbol(
            fields,
            ":strategy",
            &[":pinned", ":track-ref", ":tag-policy"],
        )?;
        require_symbol(fields, ":update-policy", &[":manual", ":auto"])?;
    }
    Ok(())
}

fn validate_locked(term: &Term) -> Result<(), String> {
    for (key, value) in bounded_map(term, "locked entries")? {
        let Term::Str(name) = &key.0 else {
            return Err("locked entry names must be strings".to_string());
        };
        bounded(name, "locked entry name")?;
        let fields = required_map(value, "locked entry")?;
        require_exact_fields(
            fields,
            &[
                ":commit",
                ":environment-fingerprint",
                ":exports-hash",
                ":registry",
                ":resolved-ref",
                ":snapshot",
                ":source-selector",
            ],
            "locked entry",
        )?;
        optional_string(fields, ":commit")?;
        optional_string(fields, ":environment-fingerprint")?;
        optional_string(fields, ":exports-hash")?;
        optional_string(fields, ":registry")?;
        optional_string(fields, ":resolved-ref")?;
        bounded_string(fields, ":snapshot")?;
        bounded_string(fields, ":source-selector")?;
    }
    Ok(())
}

fn toml_to_term(value: toml::Value) -> Term {
    match value {
        toml::Value::String(value) => Term::Str(value),
        toml::Value::Integer(value) => Term::Int(value.into()),
        toml::Value::Boolean(value) => Term::Bool(value),
        toml::Value::Float(value) => map([(":toml-float", Term::Str(format!("{value:e}")))]),
        toml::Value::Datetime(value) => map([(":toml-datetime", Term::Str(value.to_string()))]),
        toml::Value::Array(values) => Term::Vector(values.into_iter().map(toml_to_term).collect()),
        toml::Value::Table(values) => Term::Map(
            values
                .into_iter()
                .map(|(key, value)| (TermOrdKey(Term::Str(key)), toml_to_term(value)))
                .collect(),
        ),
    }
}

fn bounded_map<'a>(term: &'a Term, label: &str) -> Result<&'a BTreeMap<TermOrdKey, Term>, String> {
    let fields = required_map(term, label)?;
    if fields.len() > COLLECTION_LIMIT {
        Err(format!(
            "{label} exceeds {COLLECTION_LIMIT}-entry result limit"
        ))
    } else {
        Ok(fields)
    }
}

fn required_map<'a>(term: &'a Term, label: &str) -> Result<&'a BTreeMap<TermOrdKey, Term>, String> {
    match term {
        Term::Map(fields) => Ok(fields),
        _ => Err(format!("{label} must be map")),
    }
}

fn bounded_string<'a>(
    fields: &'a BTreeMap<TermOrdKey, Term>,
    name: &str,
) -> Result<&'a str, String> {
    let value = required_string(fields, name)?;
    bounded(value, name)?;
    Ok(value)
}

fn optional_string(fields: &BTreeMap<TermOrdKey, Term>, name: &str) -> Result<(), String> {
    match field(fields, name)? {
        Term::Nil => Ok(()),
        Term::Str(value) => bounded(value, name),
        _ => Err(format!("lock model {name} must be string or nil")),
    }
}

fn require_symbol(
    fields: &BTreeMap<TermOrdKey, Term>,
    name: &str,
    allowed: &[&str],
) -> Result<(), String> {
    match field(fields, name)? {
        Term::Symbol(value) if allowed.contains(&value.as_str()) => Ok(()),
        _ => Err(format!("lock model {name} is invalid")),
    }
}

fn bounded(value: &str, label: &str) -> Result<(), String> {
    if value.len() <= VALUE_LIMIT {
        Ok(())
    } else {
        Err(format!("{label} exceeds {VALUE_LIMIT}-byte result limit"))
    }
}

fn require_exact_fields(
    fields: &BTreeMap<TermOrdKey, Term>,
    names: &[&str],
    label: &str,
) -> Result<(), String> {
    let expected = names
        .iter()
        .map(|name| TermOrdKey(Term::symbol(*name)))
        .collect::<BTreeSet<_>>();
    if fields.keys().cloned().collect::<BTreeSet<_>>() == expected {
        Ok(())
    } else {
        Err(format!("{label} field set mismatch"))
    }
}

fn field<'a>(fields: &'a BTreeMap<TermOrdKey, Term>, name: &str) -> Result<&'a Term, String> {
    fields
        .get(&TermOrdKey(Term::symbol(name)))
        .ok_or_else(|| format!("lock-model result missing {name}"))
}

fn required_string<'a>(
    fields: &'a BTreeMap<TermOrdKey, Term>,
    name: &str,
) -> Result<&'a str, String> {
    match field(fields, name)? {
        Term::Str(value) => Ok(value),
        _ => Err(format!("lock-model result {name} must be string")),
    }
}

fn require_string(
    fields: &BTreeMap<TermOrdKey, Term>,
    name: &str,
    expected: &str,
) -> Result<(), String> {
    if required_string(fields, name)? == expected {
        Ok(())
    } else {
        Err(format!("lock-model result {name} mismatch"))
    }
}

fn require_int(
    fields: &BTreeMap<TermOrdKey, Term>,
    name: &str,
    expected: i64,
) -> Result<(), String> {
    match field(fields, name)? {
        Term::Int(value) if value == &expected.into() => Ok(()),
        _ => Err(format!("lock-model result {name} mismatch")),
    }
}

fn require_nil(fields: &BTreeMap<TermOrdKey, Term>, name: &str) -> Result<(), String> {
    if field(fields, name)? == &Term::Nil {
        Ok(())
    } else {
        Err(format!("lock-model result {name} must be nil"))
    }
}

fn map(entries: impl IntoIterator<Item = (&'static str, Term)>) -> Term {
    Term::Map(
        entries
            .into_iter()
            .map(|(name, value)| (TermOrdKey(Term::symbol(name)), value))
            .collect(),
    )
}

fn hex32(value: [u8; 32]) -> String {
    value.iter().map(|byte| format!("{byte:02x}")).collect()
}
