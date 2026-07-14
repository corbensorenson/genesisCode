use std::collections::BTreeMap;

use gc_coreform::{Term, TermOrdKey, print_term};
use thiserror::Error;

use crate::schema::{SchemaError, validate_hex_hash};

pub const VCS_SNAPSHOT_PROFILE_ID: &str = "genesis/vcs-snapshot/v1";
pub const VCS_SNAPSHOT_VERSION: i64 = 1;

#[derive(Debug, Error)]
pub enum SnapshotError {
    #[error("{0}")]
    Schema(#[from] SchemaError),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SnapshotModule {
    pub path: String,
    pub hash_hex: String,
    pub module_h: Option<[u8; 32]>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackageSnapshot {
    pub pkg_name: String,
    pub pkg_version: String,
    pub modules: Vec<SnapshotModule>,
    pub obligations: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModuleSnapshot {
    /// Optional human-readable module id.
    pub name: Option<String>,

    /// Symbol/string -> artifact hash mapping (usually definition AST hashes).
    pub defs: BTreeMap<String, String>,

    pub exports: Vec<String>,
    pub obligations: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContractSnapshot {
    pub proto: Option<String>,

    /// Op symbol -> handler artifact hash.
    pub overrides: BTreeMap<String, String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceSnapshot {
    pub workspace: Option<String>,

    /// Module name -> module snapshot hash.
    pub modules: BTreeMap<String, String>,

    /// Optional lock snapshot hash (or similar workspace root pointer).
    pub lock: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SnapshotKind {
    Package(PackageSnapshot),
    Module(ModuleSnapshot),
    Contract(ContractSnapshot),
    Workspace(WorkspaceSnapshot),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Snapshot {
    pub kind: SnapshotKind,
}

impl Snapshot {
    pub fn kind_symbol(&self) -> &'static str {
        match &self.kind {
            SnapshotKind::Package(_) => ":package",
            SnapshotKind::Module(_) => ":module",
            SnapshotKind::Contract(_) => ":contract",
            SnapshotKind::Workspace(_) => ":workspace",
        }
    }

    pub fn from_term(t: &Term) -> Result<Self, SchemaError> {
        let Term::Map(m) = t else {
            return Err(SchemaError::Bad("snapshot must be a map".to_string()));
        };
        let ty = req_sym(m, ":type", "snapshot")?;
        if ty != ":vcs/snapshot" {
            return Err(SchemaError::Bad(format!("snapshot: wrong :type {ty}")));
        }
        let v = req_i64(m, ":v", "snapshot")?;
        if v != VCS_SNAPSHOT_VERSION {
            return Err(SchemaError::Bad(format!("snapshot: unsupported :v {v}")));
        }
        let kind = req_sym(m, ":kind", "snapshot")?;

        let kind = match kind.as_str() {
            ":package" => SnapshotKind::Package(parse_package_snapshot(m)?),
            ":module" => SnapshotKind::Module(parse_module_snapshot(m)?),
            ":contract" => SnapshotKind::Contract(parse_contract_snapshot(m)?),
            ":workspace" => SnapshotKind::Workspace(parse_workspace_snapshot(m)?),
            other => return Err(SchemaError::Bad(format!("snapshot: unknown :kind {other}"))),
        };
        Ok(Self { kind })
    }

    pub fn shallow_refs(&self) -> Vec<String> {
        let mut out = Vec::new();
        match &self.kind {
            SnapshotKind::Package(p) => {
                for m in &p.modules {
                    out.push(m.hash_hex.clone());
                }
            }
            SnapshotKind::Module(m) => {
                out.extend(m.defs.values().cloned());
            }
            SnapshotKind::Contract(c) => {
                if let Some(p) = &c.proto {
                    out.push(p.clone());
                }
                out.extend(c.overrides.values().cloned());
            }
            SnapshotKind::Workspace(w) => {
                if let Some(lk) = &w.lock {
                    out.push(lk.clone());
                }
                out.extend(w.modules.values().cloned());
            }
        }
        out.sort();
        out.dedup();
        out
    }
}

impl ContractSnapshot {
    pub fn to_term(&self) -> Term {
        let mut ov: BTreeMap<TermOrdKey, Term> = BTreeMap::new();
        for (k, v) in &self.overrides {
            ov.insert(TermOrdKey(Term::symbol(k)), Term::Str(v.clone()));
        }

        Term::Map(
            [
                (
                    TermOrdKey(Term::symbol(":type")),
                    Term::symbol(":vcs/snapshot"),
                ),
                (
                    TermOrdKey(Term::symbol(":v")),
                    Term::Int(VCS_SNAPSHOT_VERSION.into()),
                ),
                (TermOrdKey(Term::symbol(":kind")), Term::symbol(":contract")),
                (
                    TermOrdKey(Term::symbol(":proto")),
                    self.proto.clone().map(Term::Str).unwrap_or(Term::Nil),
                ),
                (TermOrdKey(Term::symbol(":overrides")), Term::Map(ov)),
            ]
            .into_iter()
            .collect(),
        )
    }
}

fn parse_package_snapshot(m: &BTreeMap<TermOrdKey, Term>) -> Result<PackageSnapshot, SchemaError> {
    let pkg_name = match m.get(&TermOrdKey(Term::symbol(":pkg/name"))) {
        Some(Term::Str(s)) => s.clone(),
        Some(other) => {
            return Err(SchemaError::Bad(format!(
                "snapshot(:package): :pkg/name must be string, got {}",
                print_term(other)
            )));
        }
        None => {
            return Err(SchemaError::Bad(
                "snapshot(:package): missing :pkg/name".to_string(),
            ));
        }
    };
    let pkg_version = match m.get(&TermOrdKey(Term::symbol(":pkg/version"))) {
        Some(Term::Str(s)) => s.clone(),
        Some(other) => {
            return Err(SchemaError::Bad(format!(
                "snapshot(:package): :pkg/version must be string, got {}",
                print_term(other)
            )));
        }
        None => {
            return Err(SchemaError::Bad(
                "snapshot(:package): missing :pkg/version".to_string(),
            ));
        }
    };

    let obligations = opt_vec_sym_or_str(m, ":obligations")?;

    let modules = match m.get(&TermOrdKey(Term::symbol(":modules"))) {
        Some(Term::Vector(xs)) => parse_package_modules(xs)?,
        Some(Term::Nil) | None => Vec::new(),
        Some(other) => {
            return Err(SchemaError::Bad(format!(
                "snapshot(:package): :modules must be vector, got {}",
                print_term(other)
            )));
        }
    };

    Ok(PackageSnapshot {
        pkg_name,
        pkg_version,
        modules,
        obligations,
    })
}

fn parse_package_modules(xs: &[Term]) -> Result<Vec<SnapshotModule>, SchemaError> {
    let mut out = Vec::new();
    for x in xs {
        let Term::Map(mm) = x else {
            return Err(SchemaError::Bad(format!(
                "snapshot(:package): module entry must be a map, got {}",
                print_term(x)
            )));
        };
        let path = match mm.get(&TermOrdKey(Term::symbol(":path"))) {
            Some(Term::Str(s)) => s.clone(),
            _ => {
                return Err(SchemaError::Bad(
                    "snapshot(:package): module entry missing :path string".to_string(),
                ));
            }
        };
        let hash_hex = match mm.get(&TermOrdKey(Term::symbol(":hash"))) {
            Some(Term::Str(s)) => {
                validate_hex_hash(s).map_err(|e| {
                    SchemaError::Bad(format!("snapshot(:package): module :hash: {e}"))
                })?;
                s.clone()
            }
            _ => {
                return Err(SchemaError::Bad(
                    "snapshot(:package): module entry missing :hash string".to_string(),
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
                    "snapshot(:package): module :module-h must be 32-byte bytes or nil, got {}",
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
    Ok(out)
}

fn parse_module_snapshot(m: &BTreeMap<TermOrdKey, Term>) -> Result<ModuleSnapshot, SchemaError> {
    let name = match m.get(&TermOrdKey(Term::symbol(":module/name"))) {
        Some(Term::Str(s)) => Some(s.clone()),
        Some(Term::Nil) | None => None,
        Some(other) => {
            return Err(SchemaError::Bad(format!(
                "snapshot(:module): :module/name must be string or nil, got {}",
                print_term(other)
            )));
        }
    };

    let exports = opt_vec_sym_or_str(m, ":exports")?;
    let obligations = opt_vec_sym_or_str(m, ":obligations")?;

    let defs_t = m
        .get(&TermOrdKey(Term::symbol(":defs")))
        .ok_or_else(|| SchemaError::Bad("snapshot(:module): missing :defs".to_string()))?;
    let Term::Map(defs_m) = defs_t else {
        return Err(SchemaError::Bad(format!(
            "snapshot(:module): :defs must be map, got {}",
            print_term(defs_t)
        )));
    };
    let mut defs: BTreeMap<String, String> = BTreeMap::new();
    for (k, v) in defs_m {
        let key = match &k.0 {
            Term::Symbol(s) => s.clone(),
            Term::Str(s) => s.clone(),
            other => {
                return Err(SchemaError::Bad(format!(
                    "snapshot(:module): :defs keys must be symbol/string, got {}",
                    print_term(other)
                )));
            }
        };
        let hv = match v {
            Term::Str(s) => {
                validate_hex_hash(s).map_err(|e| {
                    SchemaError::Bad(format!("snapshot(:module): :defs/{key}: {e}"))
                })?;
                s.clone()
            }
            other => {
                return Err(SchemaError::Bad(format!(
                    "snapshot(:module): :defs values must be hash strings, got {}",
                    print_term(other)
                )));
            }
        };
        defs.insert(key, hv);
    }

    Ok(ModuleSnapshot {
        name,
        defs,
        exports,
        obligations,
    })
}

fn parse_contract_snapshot(
    m: &BTreeMap<TermOrdKey, Term>,
) -> Result<ContractSnapshot, SchemaError> {
    let proto = match m.get(&TermOrdKey(Term::symbol(":proto"))) {
        None | Some(Term::Nil) => None,
        Some(Term::Str(s)) => {
            validate_hex_hash(s)
                .map_err(|e| SchemaError::Bad(format!("snapshot(:contract): :proto: {e}")))?;
            Some(s.clone())
        }
        Some(other) => {
            return Err(SchemaError::Bad(format!(
                "snapshot(:contract): :proto must be hex string or nil, got {}",
                print_term(other)
            )));
        }
    };

    let overrides_t = m
        .get(&TermOrdKey(Term::symbol(":overrides")))
        .ok_or_else(|| SchemaError::Bad("snapshot(:contract): missing :overrides".to_string()))?;
    let Term::Map(ovm) = overrides_t else {
        return Err(SchemaError::Bad(format!(
            "snapshot(:contract): :overrides must be map, got {}",
            print_term(overrides_t)
        )));
    };
    let mut overrides: BTreeMap<String, String> = BTreeMap::new();
    for (k, v) in ovm {
        let op_sym = match &k.0 {
            Term::Symbol(s) => s.clone(),
            Term::Str(s) => s.clone(),
            other => {
                return Err(SchemaError::Bad(format!(
                    "snapshot(:contract): :overrides keys must be symbol/string, got {}",
                    print_term(other)
                )));
            }
        };
        let hv = match v {
            Term::Str(s) => {
                validate_hex_hash(s).map_err(|e| {
                    SchemaError::Bad(format!("snapshot(:contract): :overrides/{op_sym}: {e}"))
                })?;
                s.clone()
            }
            other => {
                return Err(SchemaError::Bad(format!(
                    "snapshot(:contract): :overrides values must be hash strings, got {}",
                    print_term(other)
                )));
            }
        };
        overrides.insert(op_sym, hv);
    }

    Ok(ContractSnapshot { proto, overrides })
}

fn parse_workspace_snapshot(
    m: &BTreeMap<TermOrdKey, Term>,
) -> Result<WorkspaceSnapshot, SchemaError> {
    let workspace = match m.get(&TermOrdKey(Term::symbol(":workspace"))) {
        Some(Term::Str(s)) => Some(s.clone()),
        Some(Term::Nil) | None => None,
        Some(other) => {
            return Err(SchemaError::Bad(format!(
                "snapshot(:workspace): :workspace must be string or nil, got {}",
                print_term(other)
            )));
        }
    };
    let lock = match m.get(&TermOrdKey(Term::symbol(":lock"))) {
        Some(Term::Str(s)) => {
            validate_hex_hash(s)
                .map_err(|e| SchemaError::Bad(format!("snapshot(:workspace): :lock: {e}")))?;
            Some(s.clone())
        }
        Some(Term::Nil) | None => None,
        Some(other) => {
            return Err(SchemaError::Bad(format!(
                "snapshot(:workspace): :lock must be hex string or nil, got {}",
                print_term(other)
            )));
        }
    };

    let modules_t = m
        .get(&TermOrdKey(Term::symbol(":modules")))
        .ok_or_else(|| SchemaError::Bad("snapshot(:workspace): missing :modules".to_string()))?;
    let Term::Map(mm) = modules_t else {
        return Err(SchemaError::Bad(format!(
            "snapshot(:workspace): :modules must be map, got {}",
            print_term(modules_t)
        )));
    };
    let mut modules: BTreeMap<String, String> = BTreeMap::new();
    for (k, v) in mm {
        let name = match &k.0 {
            Term::Symbol(s) => s.clone(),
            Term::Str(s) => s.clone(),
            other => {
                return Err(SchemaError::Bad(format!(
                    "snapshot(:workspace): :modules keys must be symbol/string, got {}",
                    print_term(other)
                )));
            }
        };
        let hv = match v {
            Term::Str(s) => {
                validate_hex_hash(s).map_err(|e| {
                    SchemaError::Bad(format!("snapshot(:workspace): :modules/{name}: {e}"))
                })?;
                s.clone()
            }
            other => {
                return Err(SchemaError::Bad(format!(
                    "snapshot(:workspace): :modules values must be hash strings, got {}",
                    print_term(other)
                )));
            }
        };
        modules.insert(name, hv);
    }

    Ok(WorkspaceSnapshot {
        workspace,
        modules,
        lock,
    })
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
