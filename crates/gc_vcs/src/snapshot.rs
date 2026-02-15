use std::collections::BTreeMap;

use gc_coreform::{Term, TermOrdKey, print_term};
use thiserror::Error;

use crate::schema::{SchemaError, validate_hex_hash};

#[derive(Debug, Error)]
pub enum SnapshotError {
    #[error("{0}")]
    Schema(#[from] SchemaError),
}

#[derive(Debug, Clone)]
pub struct SnapshotModule {
    pub path: String,
    pub hash_hex: String,
    pub module_h: Option<[u8; 32]>,
}

#[derive(Debug, Clone)]
pub struct Snapshot {
    pub kind: String,
    pub pkg_name: Option<String>,
    pub pkg_version: Option<String>,
    pub modules: Vec<SnapshotModule>,
    pub obligations: Vec<String>,
}

impl Snapshot {
    pub fn from_term(t: &Term) -> Result<Self, SchemaError> {
        let Term::Map(m) = t else {
            return Err(SchemaError::Bad("snapshot must be a map".to_string()));
        };
        let ty = req_sym(m, ":type", "snapshot")?;
        if ty != ":vcs/snapshot" {
            return Err(SchemaError::Bad(format!("snapshot: wrong :type {ty}")));
        }
        let v = req_i64(m, ":v", "snapshot")?;
        if v != 1 {
            return Err(SchemaError::Bad(format!("snapshot: unsupported :v {v}")));
        }
        let kind = req_sym(m, ":kind", "snapshot")?;

        let pkg_name = match m.get(&TermOrdKey(Term::symbol(":pkg/name"))) {
            Some(Term::Str(s)) => Some(s.clone()),
            Some(Term::Nil) | None => None,
            Some(other) => {
                return Err(SchemaError::Bad(format!(
                    "snapshot: :pkg/name must be string or nil, got {}",
                    print_term(other)
                )));
            }
        };
        let pkg_version = match m.get(&TermOrdKey(Term::symbol(":pkg/version"))) {
            Some(Term::Str(s)) => Some(s.clone()),
            Some(Term::Nil) | None => None,
            Some(other) => {
                return Err(SchemaError::Bad(format!(
                    "snapshot: :pkg/version must be string or nil, got {}",
                    print_term(other)
                )));
            }
        };

        let obligations = opt_vec_sym_or_str(m, ":obligations")?;

        let modules = match m.get(&TermOrdKey(Term::symbol(":modules"))) {
            Some(Term::Vector(xs)) => {
                let mut out = Vec::new();
                for x in xs {
                    let Term::Map(mm) = x else {
                        return Err(SchemaError::Bad(format!(
                            "snapshot: module entry must be a map, got {}",
                            print_term(x)
                        )));
                    };
                    let path = match mm.get(&TermOrdKey(Term::symbol(":path"))) {
                        Some(Term::Str(s)) => s.clone(),
                        _ => {
                            return Err(SchemaError::Bad(
                                "snapshot: module entry missing :path string".to_string(),
                            ));
                        }
                    };
                    let hash_hex = match mm.get(&TermOrdKey(Term::symbol(":hash"))) {
                        Some(Term::Str(s)) => {
                            validate_hex_hash(s).map_err(|e| {
                                SchemaError::Bad(format!("snapshot: module :hash: {e}"))
                            })?;
                            s.clone()
                        }
                        _ => {
                            return Err(SchemaError::Bad(
                                "snapshot: module entry missing :hash string".to_string(),
                            ));
                        }
                    };
                    let module_h = match mm.get(&TermOrdKey(Term::symbol(":module-h"))) {
                        Some(Term::Bytes(b)) if b.len() == 32 => {
                            let mut outb = [0u8; 32];
                            outb.copy_from_slice(b);
                            Some(outb)
                        }
                        Some(Term::Nil) | None => None,
                        Some(other) => {
                            return Err(SchemaError::Bad(format!(
                                "snapshot: module :module-h must be 32-byte bytes or nil, got {}",
                                print_term(other)
                            )));
                        }
                    };
                    out.push(SnapshotModule {
                        path,
                        hash_hex,
                        module_h,
                    });
                }
                out
            }
            Some(Term::Nil) | None => Vec::new(),
            Some(other) => {
                return Err(SchemaError::Bad(format!(
                    "snapshot: :modules must be vector, got {}",
                    print_term(other)
                )));
            }
        };

        Ok(Self {
            kind,
            pkg_name,
            pkg_version,
            modules,
            obligations,
        })
    }

    pub fn shallow_refs(&self) -> Vec<String> {
        let mut out = Vec::new();
        for m in &self.modules {
            out.push(m.hash_hex.clone());
        }
        out
    }
}

fn req_sym(m: &BTreeMap<TermOrdKey, Term>, k: &str, what: &str) -> Result<String, SchemaError> {
    match m.get(&TermOrdKey(Term::symbol(k))) {
        Some(Term::Symbol(s)) => Ok(s.clone()),
        Some(other) => Err(SchemaError::Bad(format!(
            "{what}: {k} must be symbol, got {}",
            print_term(other)
        ))),
        None => Err(SchemaError::Bad(format!("{what}: missing {k}"))),
    }
}

fn req_i64(m: &BTreeMap<TermOrdKey, Term>, k: &str, what: &str) -> Result<i64, SchemaError> {
    match m.get(&TermOrdKey(Term::symbol(k))) {
        Some(Term::Int(i)) => {
            use num_traits::ToPrimitive;
            i.to_i64()
                .ok_or_else(|| SchemaError::Bad(format!("{what}: {k} out of range")))
        }
        Some(other) => Err(SchemaError::Bad(format!(
            "{what}: {k} must be int, got {}",
            print_term(other)
        ))),
        None => Err(SchemaError::Bad(format!("{what}: missing {k}"))),
    }
}

fn opt_vec_sym_or_str(m: &BTreeMap<TermOrdKey, Term>, k: &str) -> Result<Vec<String>, SchemaError> {
    let Some(t) = m.get(&TermOrdKey(Term::symbol(k))) else {
        return Ok(Vec::new());
    };
    let Term::Vector(xs) = t else {
        return Err(SchemaError::Bad(format!(
            "{k} must be vector, got {}",
            print_term(t)
        )));
    };
    let mut out = Vec::new();
    for x in xs {
        match x {
            Term::Symbol(s) => out.push(s.clone()),
            Term::Str(s) => out.push(s.clone()),
            _ => {
                return Err(SchemaError::Bad(format!(
                    "{k} entries must be strings/symbols, got {}",
                    print_term(x)
                )));
            }
        }
    }
    Ok(out)
}
