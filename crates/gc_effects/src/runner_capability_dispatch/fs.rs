use super::*;
use std::collections::BTreeSet;

use crate::runner_io_ops::{canonical_path_material, validate_portable_effect_path};

fn fs_entry_kind(file_type: &std::fs::FileType) -> &'static str {
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
    let path = sandbox_path_allow_missing(&base_dir, &path_s, false)?;
    let Some(rel_path) = fs_rel_display_path(&base_dir, &path) else {
        return Ok(path_encoding_error(error_tok, op));
    };
    let md = match std::fs::symlink_metadata(&path) {
        Ok(md) => Some(md),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
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
    let path = sandbox_path_read(&base_dir, &path_s)?;
    let read_dir = match std::fs::read_dir(&path) {
        Ok(rd) => rd,
        Err(e) => {
            return Ok(Value::Sealed {
                token: error_tok,
                payload: Box::new(Value::data(io_error_payload(op, &base_dir, &path, &e))),
            });
        }
    };

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
        let entry_path = entry.path();
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
    let path = sandbox_path_allow_missing(&base_dir, &path_s, create_parents)?;
    let result = if create_parents {
        std::fs::create_dir_all(&path)
    } else {
        std::fs::create_dir(&path)
    };
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
    let path = sandbox_path_allow_missing(&base_dir, &path_s, false)?;
    let md = match std::fs::symlink_metadata(&path) {
        Ok(md) => Some(md),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
        Err(e) => {
            return Ok(Value::Sealed {
                token: error_tok,
                payload: Box::new(Value::data(io_error_payload(op, &base_dir, &path, &e))),
            });
        }
    };
    let Some(md) = md else {
        return Ok(Value::data(Term::Nil));
    };
    let file_type = md.file_type();
    let result = if file_type.is_dir() && !file_type.is_symlink() {
        if recursive {
            std::fs::remove_dir_all(&path)
        } else {
            std::fs::remove_dir(&path)
        }
    } else {
        std::fs::remove_file(&path)
    };
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
    let from = sandbox_path_read(&base_dir, &from_path)?;
    let to = sandbox_path_allow_missing(&base_dir, &to_path, create_dirs)?;
    if !overwrite && to.exists() {
        return Ok(mk_error(
            error_tok,
            "core/caps/policy-error",
            format!(
                "{op} target `{}` already exists; set :overwrite true to allow replacing it",
                fs_rel_display_path(&base_dir, &to).unwrap_or_else(|| "<invalid-path>".to_string())
            ),
            Some(op),
        ));
    }
    let result = if overwrite && to.exists() {
        let md = std::fs::symlink_metadata(&to).map_err(|e| Value::Sealed {
            token: error_tok,
            payload: Box::new(Value::data(io_error_payload(op, &base_dir, &to, &e))),
        });
        match md {
            Ok(md) => {
                let remove_result = if md.file_type().is_dir() && !md.file_type().is_symlink() {
                    std::fs::remove_dir_all(&to)
                } else {
                    std::fs::remove_file(&to)
                };
                if let Err(e) = remove_result {
                    return Ok(Value::Sealed {
                        token: error_tok,
                        payload: Box::new(Value::data(io_error_payload(op, &base_dir, &to, &e))),
                    });
                }
                std::fs::rename(&from, &to)
            }
            Err(sealed) => return Ok(sealed),
        }
    } else {
        std::fs::rename(&from, &to)
    };
    match result {
        Ok(()) => Ok(Value::data(Term::Nil)),
        Err(e) => Ok(Value::Sealed {
            token: error_tok,
            payload: Box::new(Value::data(io_error_payload(op, &base_dir, &from, &e))),
        }),
    }
}
