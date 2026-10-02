use super::*;
use std::collections::BTreeSet;

use crate::runner_io_ops::{canonical_path_material, validate_portable_effect_path};

fn fs_entry_kind(file_type: &crate::rooted_fs::FileType) -> &'static str {
    if file_type.is_file() {
        "file"
    } else if file_type.is_dir() {
        "dir"
    } else if file_type.is_symlink() {
        "symlink"
    } else {
        "other"
    }
}

fn fs_rel_display_path(base_dir: &std::path::Path, path: &std::path::Path) -> Option<String> {
    canonical_path_material(path.strip_prefix(base_dir).ok()?)
}

fn path_encoding_error(error_tok: SealId, op: &str) -> Value {
    mk_error(
        error_tok,
        "core/path-encoding-error",
        "filesystem path is outside the capability base, is not valid UTF-8/NFC, or contains a non-portable separator".to_string(),
        Some(op),
    )
}

pub(super) fn capability_io_fs_stat(
    op: &str,
    payload: &Term,
    pol: Option<&OpPolicy>,
    error_tok: SealId,
) -> Result<Value, EffectsError> {
    let path_s = payload_path(payload)?;
    let base_dir = effective_base_dir(pol)?;
    let path = base_dir.join(&path_s);
    let Some(rel_path) = fs_rel_display_path(&base_dir, &path) else {
        return Ok(path_encoding_error(error_tok, op));
    };
    let md = match crate::rooted_fs::FsRoot::open(&base_dir).and_then(|root| root.stat(&path_s)) {
        Ok(md) => md,
        Err(e) => {
            return Ok(Value::Sealed {
                token: error_tok,
                payload: Box::new(Value::data(io_error_payload(op, &base_dir, &path, &e))),
            });
        }
    };

    let mut out = BTreeMap::new();
    out.insert(TermOrdKey(Term::symbol(":path")), Term::Str(rel_path));
    out.insert(
        TermOrdKey(Term::symbol(":exists")),
        Term::Bool(md.is_some()),
    );
    match md {
        Some(md) => {
            out.insert(
                TermOrdKey(Term::symbol(":kind")),
                Term::Symbol(fs_entry_kind(&md.file_type()).to_string()),
            );
            out.insert(
                TermOrdKey(Term::symbol(":len-bytes")),
                Term::Int((md.len() as i64).into()),
            );
            out.insert(
                TermOrdKey(Term::symbol(":readonly")),
                Term::Bool(md.permissions().readonly()),
            );
        }
        None => {
            out.insert(
                TermOrdKey(Term::symbol(":kind")),
                Term::Symbol("missing".to_string()),
            );
            out.insert(
                TermOrdKey(Term::symbol(":len-bytes")),
                Term::Int(0_i64.into()),
            );
            out.insert(TermOrdKey(Term::symbol(":readonly")), Term::Bool(false));
        }
    }
    Ok(Value::data(Term::Map(out)))
}

pub(super) fn capability_io_fs_list(
    op: &str,
    payload: &Term,
    pol: Option<&OpPolicy>,
    error_tok: SealId,
) -> Result<Value, EffectsError> {
    let path_s = payload_path(payload)?;
    let base_dir = effective_base_dir(pol)?;
    let requested_path = base_dir.join(&path_s);
    let (relative_path, read_dir) =
        match crate::rooted_fs::FsRoot::open(&base_dir).and_then(|root| root.list(&path_s)) {
            Ok(result) => result,
            Err(e) => {
                return Ok(Value::Sealed {
                    token: error_tok,
                    payload: Box::new(Value::data(io_error_payload(
                        op,
                        &base_dir,
                        &requested_path,
                        &e,
                    ))),
                });
            }
        };
    let path = base_dir.join(relative_path);

    let mut entries = Vec::new();
    let mut canonical_paths = BTreeSet::new();
    for entry in read_dir {
        let entry = match entry {
            Ok(entry) => entry,
            Err(e) => {
                return Ok(Value::Sealed {
                    token: error_tok,
                    payload: Box::new(Value::data(io_error_payload(op, &base_dir, &path, &e))),
                });
            }
        };
        let entry_path = path.join(entry.file_name());
        let entry_md = match entry.metadata() {
            Ok(md) => md,
            Err(e) => {
                return Ok(Value::Sealed {
                    token: error_tok,
                    payload: Box::new(Value::data(io_error_payload(
                        op,
                        &base_dir,
                        &entry_path,
                        &e,
                    ))),
                });
            }
        };
        let Some(name) = canonical_path_material(std::path::Path::new(&entry.file_name())) else {
            return Ok(path_encoding_error(error_tok, op));
        };
        let Some(relative_path) = fs_rel_display_path(&base_dir, &entry_path) else {
            return Ok(path_encoding_error(error_tok, op));
        };
        if !canonical_paths.insert(relative_path.clone()) {
            return Ok(mk_error(
                error_tok,
                "core/path-collision-error",
                "multiple filesystem entries have the same canonical Unicode 17 NFC path"
                    .to_string(),
                Some(op),
            ));
        }
        let mut row = BTreeMap::new();
        row.insert(TermOrdKey(Term::symbol(":name")), Term::Str(name));
        row.insert(TermOrdKey(Term::symbol(":path")), Term::Str(relative_path));
        row.insert(
            TermOrdKey(Term::symbol(":kind")),
            Term::Symbol(fs_entry_kind(&entry_md.file_type()).to_string()),
        );
        row.insert(
            TermOrdKey(Term::symbol(":len-bytes")),
            Term::Int((entry_md.len() as i64).into()),
        );
        entries.push(Term::Map(row));
    }
    entries.sort_by_key(print_term);
    Ok(Value::data(Term::Vector(entries)))
}

pub(super) fn capability_io_fs_mkdir(
    op: &str,
    payload: &Term,
    pol: Option<&OpPolicy>,
    error_tok: SealId,
) -> Result<Value, EffectsError> {
    let path_s = payload_path(payload)?;
    let base_dir = effective_base_dir(pol)?;
    let create_parents = payload_optional_bool_field(payload, op, ":parents", true)?;
    let path = base_dir.join(&path_s);
    let result = crate::rooted_fs::FsRoot::open(&base_dir)
        .and_then(|root| root.mkdir(&path_s, create_parents));
    match result {
        Ok(()) => Ok(Value::data(Term::Nil)),
        Err(e) => Ok(Value::Sealed {
            token: error_tok,
            payload: Box::new(Value::data(io_error_payload(op, &base_dir, &path, &e))),
        }),
    }
}

pub(super) fn capability_io_fs_remove(
    op: &str,
    payload: &Term,
    pol: Option<&OpPolicy>,
    error_tok: SealId,
) -> Result<Value, EffectsError> {
    let path_s = payload_path(payload)?;
    let base_dir = effective_base_dir(pol)?;
    let recursive = payload_optional_bool_field(payload, op, ":recursive", false)?;
    let path = base_dir.join(&path_s);
    let result =
        crate::rooted_fs::FsRoot::open(&base_dir).and_then(|root| root.remove(&path_s, recursive));
    match result {
        Ok(()) => Ok(Value::data(Term::Nil)),
        Err(e) => Ok(Value::Sealed {
            token: error_tok,
            payload: Box::new(Value::data(io_error_payload(op, &base_dir, &path, &e))),
        }),
    }
}

pub(super) fn capability_io_fs_rename(
    op: &str,
    payload: &Term,
    pol: Option<&OpPolicy>,
    error_tok: SealId,
) -> Result<Value, EffectsError> {
    let from_path = payload_required_string_field(payload, op, ":from")?;
    let to_path = payload_required_string_field(payload, op, ":to")?;
    validate_portable_effect_path(&from_path)?;
    validate_portable_effect_path(&to_path)?;
    let overwrite = payload_optional_bool_field(payload, op, ":overwrite", false)?;
    let base_dir = effective_base_dir(pol)?;
    let create_dirs = pol.is_some_and(|p| p.create_dirs);
    let from = base_dir.join(&from_path);
    let result = crate::rooted_fs::FsRoot::open(&base_dir)
        .and_then(|root| root.rename(&from_path, &to_path, overwrite, create_dirs));
    match result {
        Ok(()) => Ok(Value::data(Term::Nil)),
        Err(e) if !overwrite && e.kind() == std::io::ErrorKind::AlreadyExists => Ok(mk_error(
            error_tok,
            "core/caps/policy-error",
            format!(
                "{op} target `{to_path}` already exists; set :overwrite true to allow replacing it"
            ),
            Some(op),
        )),
        Err(e) => Ok(Value::Sealed {
            token: error_tok,
            payload: Box::new(Value::data(io_error_payload(op, &base_dir, &from, &e))),
        }),
    }
}
