use std::collections::BTreeMap;
use std::path::Path;

use gc_coreform::{Term, TermOrdKey, canonicalize_module, parse_module, print_term};
use gc_kernel::{MemLimits, StepLimit};
use gc_obligations::{
    CoreformFrontend, coreform_frontend_is_rust, parse_canonicalize_module_source_with_frontend,
};
use num_traits::ToPrimitive;

use crate::PatchError;

pub(super) fn parse_canonicalize_module_src(
    src: &str,
    frontend: &CoreformFrontend,
    step_limit: StepLimit,
    mem_limits: MemLimits,
) -> Result<Vec<Term>, PatchError> {
    if coreform_frontend_is_rust(frontend) {
        let forms = parse_module(src).map_err(|e| PatchError::Parse(e.to_string()))?;
        canonicalize_module(forms).map_err(|e| PatchError::Validate(e.to_string()))
    } else {
        parse_canonicalize_module_source_with_frontend(src, frontend, step_limit, mem_limits)
            .map_err(|e| PatchError::Parse(e.to_string()))
    }
}

pub(super) fn patch_string_vec_field(
    v: &mut toml::Value,
    field: &str,
    add: &[String],
    remove: &[String],
) -> Result<(), PatchError> {
    let tbl = v
        .as_table_mut()
        .ok_or_else(|| PatchError::Validate("manifest must be a table".to_string()))?;
    let arr = tbl
        .entry(field.to_string())
        .or_insert_with(|| toml::Value::Array(Vec::new()))
        .as_array_mut()
        .ok_or_else(|| PatchError::Validate(format!("manifest field {field} must be array")))?;
    let mut set: std::collections::BTreeSet<String> = arr
        .iter()
        .filter_map(|x| x.as_str().map(|s| s.to_string()))
        .collect();
    for r in remove {
        set.remove(r);
    }
    for a in add {
        set.insert(a.clone());
    }
    *arr = set.into_iter().map(toml::Value::String).collect();
    Ok(())
}

pub(super) fn patch_manifest_add_module_path(
    pkg_toml: &Path,
    module_path: &str,
) -> Result<(), PatchError> {
    let mut s = std::fs::read_to_string(pkg_toml)?;
    let mut v: toml::Value = toml::from_str(&s).map_err(|e| PatchError::Parse(e.to_string()))?;
    let mods = v
        .get_mut("modules")
        .and_then(|x| x.as_array_mut())
        .ok_or_else(|| PatchError::Validate("manifest missing modules array".to_string()))?;
    let exists = mods
        .iter()
        .any(|m| m.get("path").and_then(|p| p.as_str()) == Some(module_path));
    if exists {
        return Err(PatchError::Validate(format!(
            "manifest already contains module path `{module_path}`"
        )));
    }
    mods.push(toml::Value::Table(
        [
            (
                "path".to_string(),
                toml::Value::String(module_path.to_string()),
            ),
            ("hash".to_string(), toml::Value::String("".to_string())),
        ]
        .into_iter()
        .collect(),
    ));
    s = toml::to_string_pretty(&v).map_err(|e| PatchError::Parse(e.to_string()))?;
    std::fs::write(pkg_toml, s)?;
    Ok(())
}

pub(super) fn patch_manifest_move_module_path(
    pkg_toml: &Path,
    from_module_path: &str,
    to_module_path: &str,
) -> Result<(), PatchError> {
    let mut s = std::fs::read_to_string(pkg_toml)?;
    let mut v: toml::Value = toml::from_str(&s).map_err(|e| PatchError::Parse(e.to_string()))?;
    let mods = v
        .get_mut("modules")
        .and_then(|x| x.as_array_mut())
        .ok_or_else(|| PatchError::Validate("manifest missing modules array".to_string()))?;
    let mut moved = false;
    for m in mods.iter_mut() {
        if m.get("path").and_then(|p| p.as_str()) == Some(from_module_path) {
            m.as_table_mut()
                .ok_or_else(|| {
                    PatchError::Validate("manifest module entry must be table".to_string())
                })?
                .insert(
                    "path".to_string(),
                    toml::Value::String(to_module_path.to_string()),
                );
            moved = true;
            break;
        }
    }
    if !moved {
        return Err(PatchError::Validate(format!(
            "manifest missing module path `{from_module_path}`"
        )));
    }
    s = toml::to_string_pretty(&v).map_err(|e| PatchError::Parse(e.to_string()))?;
    std::fs::write(pkg_toml, s)?;
    Ok(())
}

pub(super) fn apply_manifest_set(v: &mut toml::Value, set: &Term) -> Result<(), PatchError> {
    let Term::Map(m) = set else {
        return Err(PatchError::Validate(":set must be a map".to_string()));
    };
    let tbl = v
        .as_table_mut()
        .ok_or_else(|| PatchError::Validate("manifest must be a table".to_string()))?;
    for (k, vv) in m {
        let Term::Symbol(key) = &k.0 else {
            return Err(PatchError::Validate(
                "manifest :set keys must be symbols".to_string(),
            ));
        };
        let key = key.strip_prefix(':').unwrap_or(key.as_str()).to_string();
        tbl.insert(key, coreform_to_toml(vv)?);
    }
    Ok(())
}

pub(super) fn coreform_to_toml(t: &Term) -> Result<toml::Value, PatchError> {
    use base64::Engine as _;
    match t {
        Term::Nil => Ok(toml::Value::String("nil".to_string())),
        Term::Bool(b) => Ok(toml::Value::Boolean(*b)),
        Term::Int(i) => {
            Ok(toml::Value::Integer(i.to_i64().ok_or_else(|| {
                PatchError::Validate("int out of range".to_string())
            })?))
        }
        Term::Str(s) => Ok(toml::Value::String(s.clone())),
        Term::Bytes(b) => Ok(toml::Value::String(
            base64::engine::general_purpose::STANDARD.encode(b),
        )),
        Term::Symbol(s) => Ok(toml::Value::String(s.clone())),
        Term::Vector(xs) => Ok(toml::Value::Array(
            xs.iter().map(coreform_to_toml).collect::<Result<_, _>>()?,
        )),
        Term::Map(m) => Ok(toml::Value::Table(
            m.iter()
                .map(|(k, v)| {
                    let kk = match &k.0 {
                        Term::Symbol(s) => s.clone(),
                        Term::Str(s) => s.clone(),
                        other => print_term(other),
                    };
                    Ok((kk, coreform_to_toml(v)?))
                })
                .collect::<Result<_, PatchError>>()?,
        )),
        Term::Pair(_, _) => Err(PatchError::Validate(
            "cannot convert list to TOML in :set".to_string(),
        )),
    }
}

pub(super) fn toml_to_coreform(v: &toml::Value) -> Result<Term, PatchError> {
    match v {
        toml::Value::String(s) => Ok(Term::Str(s.clone())),
        toml::Value::Integer(i) => Ok(Term::Int((*i).into())),
        toml::Value::Boolean(b) => Ok(Term::Bool(*b)),
        toml::Value::Float(f) => Ok(Term::Str(f.to_string())),
        toml::Value::Datetime(dt) => Ok(Term::Str(dt.to_string())),
        toml::Value::Array(xs) => Ok(Term::Vector(
            xs.iter().map(toml_to_coreform).collect::<Result<_, _>>()?,
        )),
        toml::Value::Table(m) => Ok(Term::Map(
            m.iter()
                .map(|(k, v)| Ok((TermOrdKey(Term::Str(k.clone())), toml_to_coreform(v)?)))
                .collect::<Result<_, PatchError>>()?,
        )),
    }
}

pub(super) fn update_manifest_op_to_term(
    set: Option<&Term>,
    obligations_add: &[String],
    obligations_remove: &[String],
    tests_add: &[String],
    tests_remove: &[String],
    caps_policy: Option<&str>,
) -> Result<Term, PatchError> {
    let mut m = BTreeMap::new();
    if let Some(set) = set {
        m.insert(TermOrdKey(Term::symbol(":set")), set.clone());
    }
    if !obligations_add.is_empty() {
        m.insert(
            TermOrdKey(Term::symbol(":obligations-add")),
            Term::Vector(
                obligations_add
                    .iter()
                    .map(|s| Term::Str(s.clone()))
                    .collect(),
            ),
        );
    }
    if !obligations_remove.is_empty() {
        m.insert(
            TermOrdKey(Term::symbol(":obligations-remove")),
            Term::Vector(
                obligations_remove
                    .iter()
                    .map(|s| Term::Str(s.clone()))
                    .collect(),
            ),
        );
    }
    if !tests_add.is_empty() {
        m.insert(
            TermOrdKey(Term::symbol(":tests-add")),
            Term::Vector(tests_add.iter().map(|s| Term::Str(s.clone())).collect()),
        );
    }
    if !tests_remove.is_empty() {
        m.insert(
            TermOrdKey(Term::symbol(":tests-remove")),
            Term::Vector(tests_remove.iter().map(|s| Term::Str(s.clone())).collect()),
        );
    }
    if let Some(p) = caps_policy {
        m.insert(
            TermOrdKey(Term::symbol(":caps-policy")),
            Term::Str(p.to_string()),
        );
    }
    Ok(Term::Map(m))
}
