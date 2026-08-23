use std::collections::{BTreeMap, BTreeSet};

use gc_coreform::{Term, TermOrdKey};

pub(super) fn body<'a>(
    bodies: &'a BTreeMap<String, Vec<u8>>,
    name: &str,
) -> Result<&'a [u8], String> {
    bodies
        .get(name)
        .map(Vec::as_slice)
        .ok_or_else(|| format!("workspace-env missing body {name}"))
}

pub(super) fn nested_map<'a>(
    term: &'a Term,
    key: &str,
) -> Result<&'a BTreeMap<TermOrdKey, Term>, String> {
    let fields = as_map(term, "workspace-env outer map")?;
    as_map(field(fields, key)?, "workspace-env nested map")
}

pub(super) fn nested_string<'a>(
    term: &'a Term,
    map_key: &str,
    key: &str,
) -> Result<&'a str, String> {
    required_string(nested_map(term, map_key)?, key)
}

pub(super) fn as_map<'a>(
    term: &'a Term,
    label: &str,
) -> Result<&'a BTreeMap<TermOrdKey, Term>, String> {
    match term {
        Term::Map(fields) => Ok(fields),
        _ => Err(format!("{label} must be map")),
    }
}

pub(super) fn map(entries: impl IntoIterator<Item = (&'static str, Term)>) -> Term {
    Term::Map(
        entries
            .into_iter()
            .map(|(name, value)| (TermOrdKey(Term::symbol(name)), value))
            .collect(),
    )
}

pub(super) fn optional_term(value: Option<&str>) -> Term {
    value
        .map(|value| Term::Str(value.to_string()))
        .unwrap_or(Term::Nil)
}

pub(super) fn require_exact_fields(
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

pub(super) fn field<'a>(
    fields: &'a BTreeMap<TermOrdKey, Term>,
    name: &str,
) -> Result<&'a Term, String> {
    fields
        .get(&TermOrdKey(Term::symbol(name)))
        .ok_or_else(|| format!("workspace-env result missing {name}"))
}

pub(super) fn required_string<'a>(
    fields: &'a BTreeMap<TermOrdKey, Term>,
    name: &str,
) -> Result<&'a str, String> {
    match field(fields, name)? {
        Term::Str(value) => Ok(value),
        _ => Err(format!("workspace-env {name} must be string")),
    }
}

pub(super) fn optional_string(
    fields: &BTreeMap<TermOrdKey, Term>,
    name: &str,
) -> Result<Option<String>, String> {
    match field(fields, name)? {
        Term::Nil => Ok(None),
        Term::Str(value) => Ok(Some(value.clone())),
        _ => Err(format!("workspace-env {name} must be string or nil")),
    }
}

pub(super) fn required_bytes<'a>(
    fields: &'a BTreeMap<TermOrdKey, Term>,
    name: &str,
) -> Result<&'a [u8], String> {
    match field(fields, name)? {
        Term::Bytes(value) => Ok(value.as_ref()),
        _ => Err(format!("workspace-env {name} must be bytes")),
    }
}

pub(super) fn required_bool(
    fields: &BTreeMap<TermOrdKey, Term>,
    name: &str,
) -> Result<bool, String> {
    match field(fields, name)? {
        Term::Bool(value) => Ok(*value),
        _ => Err(format!("workspace-env {name} must be bool")),
    }
}

pub(super) fn require_string(
    fields: &BTreeMap<TermOrdKey, Term>,
    name: &str,
    expected: &str,
) -> Result<(), String> {
    if required_string(fields, name)? == expected {
        Ok(())
    } else {
        Err(format!("workspace-env {name} mismatch"))
    }
}

pub(super) fn require_optional_string(
    fields: &BTreeMap<TermOrdKey, Term>,
    name: &str,
    expected: Option<&str>,
) -> Result<(), String> {
    match (field(fields, name)?, expected) {
        (Term::Symbol(value), None) if value == ":none" => Ok(()),
        (Term::Str(value), Some(expected)) if value == expected => Ok(()),
        _ => Err(format!("workspace-env {name} mismatch")),
    }
}

pub(super) fn require_symbol(
    fields: &BTreeMap<TermOrdKey, Term>,
    name: &str,
    expected: &str,
) -> Result<(), String> {
    match field(fields, name)? {
        Term::Symbol(value) if value == expected => Ok(()),
        _ => Err(format!("workspace-env {name} symbol mismatch")),
    }
}

pub(super) fn require_bool(
    fields: &BTreeMap<TermOrdKey, Term>,
    name: &str,
    expected: bool,
) -> Result<(), String> {
    if required_bool(fields, name)? == expected {
        Ok(())
    } else {
        Err(format!("workspace-env {name} mismatch"))
    }
}

pub(super) fn require_int(
    fields: &BTreeMap<TermOrdKey, Term>,
    name: &str,
    expected: i64,
) -> Result<(), String> {
    match field(fields, name)? {
        Term::Int(value) if value.to_string() == expected.to_string() => Ok(()),
        _ => Err(format!("workspace-env {name} mismatch")),
    }
}

pub(super) fn require_nil(fields: &BTreeMap<TermOrdKey, Term>, name: &str) -> Result<(), String> {
    match field(fields, name)? {
        Term::Nil => Ok(()),
        _ => Err(format!("workspace-env {name} must be nil")),
    }
}

pub(super) fn require_lower_hex64(value: &str, label: &str) -> Result<(), String> {
    if value.len() == 64
        && value
            .as_bytes()
            .iter()
            .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
    {
        Ok(())
    } else {
        Err(format!("workspace-env {label} must be lowercase hex64"))
    }
}

pub(super) fn blake3_hex(bytes: &[u8]) -> String {
    blake3::hash(bytes).to_hex().to_string()
}

pub(super) fn hex32(bytes: [u8; 32]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}
