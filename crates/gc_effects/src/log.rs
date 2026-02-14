use std::collections::BTreeMap;

use gc_coreform::{Term, TermOrdKey, print_term};
use num_traits::ToPrimitive;

use crate::error::EffectsError;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Decision {
    Allow,
    Deny,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LoggedResp {
    Ok(Term),
    Error(Term),
    OkArtifact { artifact: String },
    ErrorArtifact { artifact: String },
    OkBytesArtifact { artifact: String },
    ErrorBytesArtifact { artifact: String },
}

#[derive(Debug, Clone)]
pub struct EffectLogEntry {
    pub i: u64,
    pub op: String,
    pub payload_h: [u8; 32],
    pub cont_h: [u8; 32],
    pub req_h: [u8; 32],
    pub decision: Decision,
    pub cap: Term,
    pub resp: LoggedResp,
    pub resp_h: [u8; 32],
}

#[derive(Debug, Clone)]
pub struct EffectLog {
    pub version: u64,
    pub program_hash: [u8; 32],
    pub toolchain: String,
    pub entries: Vec<EffectLogEntry>,
}

impl EffectLog {
    pub fn to_term(&self) -> Term {
        let mut m = BTreeMap::new();
        m.insert(
            TermOrdKey(Term::Symbol(":version".to_string())),
            Term::Int((self.version as i64).into()),
        );
        m.insert(
            TermOrdKey(Term::Symbol(":program-hash".to_string())),
            Term::Bytes(self.program_hash.to_vec()),
        );
        m.insert(
            TermOrdKey(Term::Symbol(":toolchain".to_string())),
            Term::Str(self.toolchain.clone()),
        );
        let entries: Vec<Term> = self.entries.iter().map(|e| e.to_term()).collect();
        m.insert(
            TermOrdKey(Term::Symbol(":entries".to_string())),
            Term::Vector(entries),
        );
        Term::Map(m)
    }

    pub fn from_term(t: &Term) -> Result<Self, EffectsError> {
        let Term::Map(m) = t else {
            return Err(EffectsError::Log("gclog must be a map".to_string()));
        };
        let version = get_int(m, ":version")?.unwrap_or(2);
        if version != 2 {
            return Err(EffectsError::Log(format!(
                "unsupported gclog :version {version} (expected 2)"
            )));
        }
        let program_hash = get_bytes32(m, ":program-hash")?;
        let toolchain = get_str(m, ":toolchain")?.unwrap_or_else(|| "unknown".to_string());
        let entries_t = map_get(m, ":entries")
            .ok_or_else(|| EffectsError::Log("missing :entries".to_string()))?;
        let Term::Vector(xs) = entries_t else {
            return Err(EffectsError::Log(":entries must be a vector".to_string()));
        };
        let mut entries = Vec::with_capacity(xs.len());
        for x in xs {
            entries.push(EffectLogEntry::from_term(x)?);
        }
        Ok(Self {
            version,
            program_hash,
            toolchain,
            entries,
        })
    }

    pub fn to_string_canonical(&self) -> String {
        print_term(&self.to_term())
    }
}

impl EffectLogEntry {
    pub fn to_term(&self) -> Term {
        let mut m = BTreeMap::new();
        m.insert(
            TermOrdKey(Term::Symbol(":i".to_string())),
            Term::Int((self.i as i64).into()),
        );
        m.insert(
            TermOrdKey(Term::Symbol(":op".to_string())),
            Term::Symbol(self.op.clone()),
        );
        m.insert(
            TermOrdKey(Term::Symbol(":payload-h".to_string())),
            Term::Bytes(self.payload_h.to_vec()),
        );
        m.insert(
            TermOrdKey(Term::Symbol(":cont-h".to_string())),
            Term::Bytes(self.cont_h.to_vec()),
        );
        m.insert(
            TermOrdKey(Term::Symbol(":req-h".to_string())),
            Term::Bytes(self.req_h.to_vec()),
        );
        m.insert(
            TermOrdKey(Term::Symbol(":decision".to_string())),
            Term::Symbol(match self.decision {
                Decision::Allow => ":allow".to_string(),
                Decision::Deny => ":deny".to_string(),
            }),
        );
        m.insert(
            TermOrdKey(Term::Symbol(":cap".to_string())),
            self.cap.clone(),
        );
        m.insert(
            TermOrdKey(Term::Symbol(":resp".to_string())),
            match &self.resp {
                LoggedResp::Ok(v) => resp_tag("ok", v),
                LoggedResp::Error(v) => resp_tag("error", v),
                LoggedResp::OkArtifact { artifact } => resp_artifact_tag("ok-artifact", artifact),
                LoggedResp::ErrorArtifact { artifact } => {
                    resp_artifact_tag("error-artifact", artifact)
                }
                LoggedResp::OkBytesArtifact { artifact } => {
                    resp_artifact_tag("ok-bytes-artifact", artifact)
                }
                LoggedResp::ErrorBytesArtifact { artifact } => {
                    resp_artifact_tag("error-bytes-artifact", artifact)
                }
            },
        );
        m.insert(
            TermOrdKey(Term::Symbol(":resp-h".to_string())),
            Term::Bytes(self.resp_h.to_vec()),
        );
        Term::Map(m)
    }

    pub fn from_term(t: &Term) -> Result<Self, EffectsError> {
        let Term::Map(m) = t else {
            return Err(EffectsError::Log("entry must be a map".to_string()));
        };
        let i =
            get_int(m, ":i")?.ok_or_else(|| EffectsError::Log("entry missing :i".to_string()))?;
        let op = match map_get(m, ":op") {
            Some(Term::Symbol(s)) => s.clone(),
            Some(x) => {
                return Err(EffectsError::Log(format!(
                    ":op must be symbol, got {}",
                    print_term(x)
                )));
            }
            None => return Err(EffectsError::Log("entry missing :op".to_string())),
        };
        let payload_h = get_bytes32(m, ":payload-h")?;
        let cont_h = get_bytes32(m, ":cont-h")?;
        let req_h = get_bytes32(m, ":req-h")?;
        let decision = match map_get(m, ":decision") {
            Some(Term::Symbol(s)) if s == ":allow" => Decision::Allow,
            Some(Term::Symbol(s)) if s == ":deny" => Decision::Deny,
            Some(x) => {
                return Err(EffectsError::Log(format!(
                    ":decision must be :allow or :deny, got {}",
                    print_term(x)
                )));
            }
            None => return Err(EffectsError::Log("entry missing :decision".to_string())),
        };
        let cap = map_get(m, ":cap").cloned().unwrap_or(Term::Nil);
        let resp_t = map_get(m, ":resp")
            .ok_or_else(|| EffectsError::Log("entry missing :resp".to_string()))?;
        let resp = parse_resp(resp_t)?;
        let resp_h = get_bytes32(m, ":resp-h")?;
        Ok(Self {
            i,
            op,
            payload_h,
            cont_h,
            req_h,
            decision,
            cap,
            resp,
            resp_h,
        })
    }
}

fn resp_tag(kind: &str, value: &Term) -> Term {
    let mut m = BTreeMap::new();
    m.insert(
        TermOrdKey(Term::Symbol(":kind".to_string())),
        Term::Symbol(format!(":{kind}")),
    );
    m.insert(
        TermOrdKey(Term::Symbol(":value".to_string())),
        value.clone(),
    );
    Term::Map(m)
}

fn resp_artifact_tag(kind: &str, artifact: &str) -> Term {
    let mut m = BTreeMap::new();
    m.insert(
        TermOrdKey(Term::Symbol(":kind".to_string())),
        Term::Symbol(format!(":{kind}")),
    );
    m.insert(
        TermOrdKey(Term::Symbol(":artifact".to_string())),
        Term::Str(artifact.to_string()),
    );
    Term::Map(m)
}

fn parse_resp(t: &Term) -> Result<LoggedResp, EffectsError> {
    let Term::Map(m) = t else {
        return Err(EffectsError::Log(":resp must be a map".to_string()));
    };
    let kind = match map_get(m, ":kind") {
        Some(Term::Symbol(s)) => s.as_str(),
        Some(x) => {
            return Err(EffectsError::Log(format!(
                ":resp :kind must be symbol, got {}",
                print_term(x)
            )));
        }
        None => return Err(EffectsError::Log(":resp missing :kind".to_string())),
    };
    match kind {
        ":ok" | ":error" => {
            let value = map_get(m, ":value")
                .ok_or_else(|| EffectsError::Log(":resp missing :value".to_string()))?
                .clone();
            match kind {
                ":ok" => Ok(LoggedResp::Ok(value)),
                ":error" => Ok(LoggedResp::Error(value)),
                _ => unreachable!(),
            }
        }
        ":ok-artifact"
        | ":error-artifact"
        | ":ok-bytes-artifact"
        | ":error-bytes-artifact" => {
            let artifact = map_get(m, ":artifact")
                .ok_or_else(|| EffectsError::Log(":resp missing :artifact".to_string()))?;
            let Term::Str(hex) = artifact else {
                return Err(EffectsError::Log(format!(
                    ":resp :artifact must be string, got {}",
                    print_term(artifact)
                )));
            };
            match kind {
                ":ok-artifact" => Ok(LoggedResp::OkArtifact {
                    artifact: hex.clone(),
                }),
                ":error-artifact" => Ok(LoggedResp::ErrorArtifact {
                    artifact: hex.clone(),
                }),
                ":ok-bytes-artifact" => Ok(LoggedResp::OkBytesArtifact {
                    artifact: hex.clone(),
                }),
                ":error-bytes-artifact" => Ok(LoggedResp::ErrorBytesArtifact {
                    artifact: hex.clone(),
                }),
                _ => unreachable!(),
            }
        }
        _ => Err(EffectsError::Log(format!("unknown resp kind {kind}"))),
    }
}

fn map_get<'a>(m: &'a BTreeMap<TermOrdKey, Term>, k: &str) -> Option<&'a Term> {
    m.get(&TermOrdKey(Term::Symbol(k.to_string())))
}

fn get_int(m: &BTreeMap<TermOrdKey, Term>, k: &str) -> Result<Option<u64>, EffectsError> {
    match map_get(m, k) {
        None => Ok(None),
        Some(Term::Int(i)) => {
            Ok(Some(i.to_u64().ok_or_else(|| {
                EffectsError::Log(format!("{k} out of range"))
            })?))
        }
        Some(x) => Err(EffectsError::Log(format!(
            "{k} must be int, got {}",
            print_term(x)
        ))),
    }
}

fn get_str(m: &BTreeMap<TermOrdKey, Term>, k: &str) -> Result<Option<String>, EffectsError> {
    match map_get(m, k) {
        None => Ok(None),
        Some(Term::Str(s)) => Ok(Some(s.clone())),
        Some(x) => Err(EffectsError::Log(format!(
            "{k} must be string, got {}",
            print_term(x)
        ))),
    }
}

fn get_bytes32(m: &BTreeMap<TermOrdKey, Term>, k: &str) -> Result<[u8; 32], EffectsError> {
    match map_get(m, k) {
        Some(Term::Bytes(b)) if b.len() == 32 => {
            let mut out = [0u8; 32];
            out.copy_from_slice(b);
            Ok(out)
        }
        Some(Term::Bytes(b)) => Err(EffectsError::Log(format!(
            "{k} must be 32 bytes, got {} bytes",
            b.len()
        ))),
        Some(x) => Err(EffectsError::Log(format!(
            "{k} must be bytes, got {}",
            print_term(x)
        ))),
        None => Err(EffectsError::Log(format!("missing {k}"))),
    }
}
