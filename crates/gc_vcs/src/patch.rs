use std::collections::BTreeMap;

use gc_coreform::{Term, TermOrdKey, print_term};
use thiserror::Error;

use crate::schema::{SchemaError, validate_hex_hash};

#[derive(Debug, Error)]
pub enum PatchError {
    #[error("{0}")]
    Schema(#[from] SchemaError),
}

#[derive(Debug, Clone)]
pub enum PatchOp {
    Replace { value: String },
    Insert { value: String },
    Delete,
    Rename,
}

#[derive(Debug, Clone)]
pub struct Patch {
    pub ops: Vec<PatchOp>,
}

impl Patch {
    pub fn from_term(t: &Term) -> Result<Self, SchemaError> {
        let Term::Map(m) = t else {
            return Err(SchemaError::Bad("patch must be a map".to_string()));
        };
        let ty = req_sym(m, ":type", "patch")?;
        if ty != ":vcs/patch" {
            return Err(SchemaError::Bad(format!("patch: wrong :type {ty}")));
        }
        let v = req_i64(m, ":v", "patch")?;
        if v != 1 {
            return Err(SchemaError::Bad(format!("patch: unsupported :v {v}")));
        }
        let ops_t = m
            .get(&TermOrdKey(Term::symbol(":ops")))
            .ok_or_else(|| SchemaError::Bad("patch: missing :ops".to_string()))?;
        let Term::Vector(xs) = ops_t else {
            return Err(SchemaError::Bad(format!(
                "patch: :ops must be vector, got {}",
                print_term(ops_t)
            )));
        };
        let mut ops = Vec::new();
        for x in xs {
            let Term::Map(mm) = x else {
                return Err(SchemaError::Bad(format!(
                    "patch: op must be a map, got {}",
                    print_term(x)
                )));
            };
            let op = req_sym(mm, ":op", "patch op")?;
            match op.as_str() {
                ":replace" | ":insert" => {
                    let v = match mm.get(&TermOrdKey(Term::symbol(":value"))) {
                        Some(Term::Str(s)) => {
                            validate_hex_hash(s)
                                .map_err(|e| SchemaError::Bad(format!("patch: :value: {e}")))?;
                            s.clone()
                        }
                        Some(other) => {
                            return Err(SchemaError::Bad(format!(
                                "patch: :value must be hex string, got {}",
                                print_term(other)
                            )));
                        }
                        None => {
                            return Err(SchemaError::Bad(
                                "patch: missing :value for replace/insert".to_string(),
                            ));
                        }
                    };
                    if op == ":replace" {
                        ops.push(PatchOp::Replace { value: v });
                    } else {
                        ops.push(PatchOp::Insert { value: v });
                    }
                }
                ":delete" => {
                    ops.push(PatchOp::Delete);
                }
                ":rename" => {
                    ops.push(PatchOp::Rename);
                }
                other => {
                    return Err(SchemaError::Bad(format!("patch: unknown op {other}")));
                }
            }
        }
        Ok(Self { ops })
    }

    pub fn refs(&self) -> Vec<String> {
        let mut out = Vec::new();
        for op in &self.ops {
            match op {
                PatchOp::Replace { value } | PatchOp::Insert { value } => out.push(value.clone()),
                PatchOp::Delete | PatchOp::Rename => {}
            }
        }
        out
    }
}

fn req_sym(m: &BTreeMap<TermOrdKey, Term>, k: &str, what: &str) -> Result<String, SchemaError> {
    match m.get(&TermOrdKey(Term::symbol(k))) {
        Some(Term::Symbol(s)) => Ok(s.clone()),
        Some(Term::Str(s)) => Ok(s.clone()),
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
