use std::collections::BTreeMap;

use gc_coreform::{Term, TermOrdKey, print_term};
use num_traits::ToPrimitive;
use thiserror::Error;

use crate::schema::{SchemaError, validate_hex_hash};

pub const VCS_PATCH_PROFILE_ID: &str = "genesis/vcs-patch/v1";
pub const VCS_PATCH_VERSION: i64 = 1;

#[derive(Debug, Error)]
pub enum PatchError {
    #[error("{0}")]
    Schema(#[from] SchemaError),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PathStep {
    Form(usize),
    PairCar,
    PairCdr,
    Vec(usize),
    Map(Term),
}

pub fn path_from_term(t: &Term) -> Result<Vec<PathStep>, SchemaError> {
    let Term::Vector(steps) = t else {
        return Err(SchemaError::Bad(format!(
            "patch: :path must be a vector, got {}",
            print_term(t)
        )));
    };
    let mut out = Vec::new();
    for s in steps {
        let Term::Vector(items) = s else {
            return Err(SchemaError::Bad(format!(
                "patch: path step must be a vector, got {}",
                print_term(s)
            )));
        };
        if items.is_empty() {
            return Err(SchemaError::Bad("patch: empty path step".to_string()));
        }
        let tag = match &items[0] {
            Term::Symbol(x) => x.as_str(),
            other => {
                return Err(SchemaError::Bad(format!(
                    "patch: bad path tag {}",
                    print_term(other)
                )));
            }
        };
        match tag {
            ":form" => {
                if items.len() != 2 {
                    return Err(SchemaError::Bad("patch: :form expects 1 arg".to_string()));
                }
                out.push(PathStep::Form(term_to_usize(&items[1])?));
            }
            ":pair-car" => out.push(PathStep::PairCar),
            ":pair-cdr" => out.push(PathStep::PairCdr),
            ":vec" => {
                if items.len() != 2 {
                    return Err(SchemaError::Bad("patch: :vec expects 1 arg".to_string()));
                }
                out.push(PathStep::Vec(term_to_usize(&items[1])?));
            }
            ":map" => {
                if items.len() != 2 {
                    return Err(SchemaError::Bad("patch: :map expects 1 arg".to_string()));
                }
                out.push(PathStep::Map(items[1].clone()));
            }
            other => {
                return Err(SchemaError::Bad(format!(
                    "patch: unknown path step {other}"
                )));
            }
        }
    }
    Ok(out)
}

pub fn path_to_term(path: &[PathStep]) -> Term {
    Term::Vector(
        path.iter()
            .map(|s| match s {
                PathStep::Form(i) => {
                    Term::Vector(vec![Term::symbol(":form"), Term::Int((*i as i64).into())])
                }
                PathStep::PairCar => Term::Vector(vec![Term::symbol(":pair-car")]),
                PathStep::PairCdr => Term::Vector(vec![Term::symbol(":pair-cdr")]),
                PathStep::Vec(i) => {
                    Term::Vector(vec![Term::symbol(":vec"), Term::Int((*i as i64).into())])
                }
                PathStep::Map(k) => Term::Vector(vec![Term::symbol(":map"), k.clone()]),
            })
            .collect(),
    )
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PatchOp {
    Replace { path: Vec<PathStep>, value: String },
    Insert { path: Vec<PathStep>, value: String },
    Delete { path: Vec<PathStep> },
    Rename { from: String, to: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Patch {
    pub ops: Vec<PatchOp>,
}

impl Patch {
    pub fn from_term(t: &Term) -> Result<Self, SchemaError> {
        let Term::Map(m) = t else {
            return Err(SchemaError::Bad("patch must be a map".to_string()));
        };
        let ty = req_sym_or_str(m, ":type", "patch")?;
        if ty != ":vcs/patch" {
            return Err(SchemaError::Bad(format!("patch: wrong :type {ty}")));
        }
        let v = req_i64(m, ":v", "patch")?;
        if v != VCS_PATCH_VERSION {
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
            let op = req_sym_or_str(mm, ":op", "patch op")?;
            match op.as_str() {
                ":replace" | ":insert" => {
                    let path_t = mm.get(&TermOrdKey(Term::symbol(":path"))).ok_or_else(|| {
                        SchemaError::Bad(format!("patch: missing :path for {op}"))
                    })?;
                    let path = path_from_term(path_t)?;
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
                            return Err(SchemaError::Bad(format!(
                                "patch: missing :value for {op}"
                            )));
                        }
                    };
                    if op == ":replace" {
                        ops.push(PatchOp::Replace { path, value: v });
                    } else {
                        ops.push(PatchOp::Insert { path, value: v });
                    }
                }
                ":delete" => {
                    let path_t = mm.get(&TermOrdKey(Term::symbol(":path"))).ok_or_else(|| {
                        SchemaError::Bad("patch: missing :path for :delete".to_string())
                    })?;
                    let path = path_from_term(path_t)?;
                    ops.push(PatchOp::Delete { path });
                }
                ":rename" => {
                    let from = req_sym_or_str(mm, ":from", "patch op")?;
                    let to = req_sym_or_str(mm, ":to", "patch op")?;
                    ops.push(PatchOp::Rename { from, to });
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
                PatchOp::Replace { value, .. } | PatchOp::Insert { value, .. } => {
                    out.push(value.clone())
                }
                PatchOp::Delete { .. } | PatchOp::Rename { .. } => {}
            }
        }
        out
    }
}

fn term_to_usize(t: &Term) -> Result<usize, SchemaError> {
    match t {
        Term::Int(i) => i
            .to_usize()
            .ok_or_else(|| SchemaError::Bad("index out of range".to_string())),
        _ => Err(SchemaError::Bad(format!(
            "index must be int, got {}",
            print_term(t)
        ))),
    }
}

fn req_sym_or_str(
    m: &BTreeMap<TermOrdKey, Term>,
    k: &str,
    what: &str,
) -> Result<String, SchemaError> {
    match m.get(&TermOrdKey(Term::symbol(k))) {
        Some(Term::Symbol(s)) => Ok(s.clone()),
        Some(Term::Str(s)) => Ok(s.clone()),
        Some(other) => Err(SchemaError::Bad(format!(
            "{what}: {k} must be symbol/string, got {}",
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
