use std::collections::BTreeMap;

use blake3::Hasher;
use gc_coreform::{Term, TermOrdKey, print_term};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum SchemaError {
    #[error("schema error: {0}")]
    Bad(String),
}

fn req_map<'a>(t: &'a Term, what: &str) -> Result<&'a BTreeMap<TermOrdKey, Term>, SchemaError> {
    match t {
        Term::Map(m) => Ok(m),
        _ => Err(SchemaError::Bad(format!(
            "{what}: expected map, got {}",
            print_term(t)
        ))),
    }
}

fn get<'a>(m: &'a BTreeMap<TermOrdKey, Term>, k: &str) -> Option<&'a Term> {
    m.get(&TermOrdKey(Term::symbol(k)))
}

fn req_sym(m: &BTreeMap<TermOrdKey, Term>, k: &str, what: &str) -> Result<String, SchemaError> {
    match get(m, k) {
        Some(Term::Symbol(s)) => Ok(s.clone()),
        Some(other) => Err(SchemaError::Bad(format!(
            "{what}: {k} must be symbol, got {}",
            print_term(other)
        ))),
        None => Err(SchemaError::Bad(format!("{what}: missing {k}"))),
    }
}

fn req_int(m: &BTreeMap<TermOrdKey, Term>, k: &str, what: &str) -> Result<i64, SchemaError> {
    match get(m, k) {
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

fn opt_vec_str(
    m: &BTreeMap<TermOrdKey, Term>,
    k: &str,
    what: &str,
) -> Result<Vec<String>, SchemaError> {
    let Some(t) = get(m, k) else {
        return Ok(Vec::new());
    };
    let Term::Vector(xs) = t else {
        return Err(SchemaError::Bad(format!(
            "{what}: {k} must be vector, got {}",
            print_term(t)
        )));
    };
    let mut out = Vec::new();
    for x in xs {
        match x {
            Term::Str(s) => out.push(s.clone()),
            Term::Symbol(s) => out.push(s.clone()),
            _ => {
                return Err(SchemaError::Bad(format!(
                    "{what}: {k} entries must be str/sym, got {}",
                    print_term(x)
                )));
            }
        }
    }
    Ok(out)
}

fn opt_vec_hex(
    m: &BTreeMap<TermOrdKey, Term>,
    k: &str,
    what: &str,
) -> Result<Vec<String>, SchemaError> {
    let Some(t) = get(m, k) else {
        return Ok(Vec::new());
    };
    let Term::Vector(xs) = t else {
        return Err(SchemaError::Bad(format!(
            "{what}: {k} must be vector, got {}",
            print_term(t)
        )));
    };
    let mut out = Vec::new();
    for x in xs {
        let Term::Str(s) = x else {
            return Err(SchemaError::Bad(format!(
                "{what}: {k} entries must be hex strings, got {}",
                print_term(x)
            )));
        };
        validate_hex_hash(s).map_err(|e| SchemaError::Bad(format!("{what}: {k}: {e}")))?;
        out.push(s.clone());
    }
    Ok(out)
}

pub fn validate_hex_hash(s: &str) -> Result<(), String> {
    let t = s.trim();
    if t.len() != 64 || !t.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err(format!("invalid hash: {t}"));
    }
    Ok(())
}

pub fn hex_to_bytes32(s: &str) -> Result<[u8; 32], String> {
    let t = s.trim();
    validate_hex_hash(t)?;
    let mut out = [0u8; 32];
    for (i, b) in out.iter_mut().enumerate() {
        let hi = hex_val(t.as_bytes()[2 * i]).ok_or_else(|| "invalid hex".to_string())?;
        let lo = hex_val(t.as_bytes()[2 * i + 1]).ok_or_else(|| "invalid hex".to_string())?;
        *b = (hi << 4) | lo;
    }
    Ok(out)
}

pub fn bytes32_to_hex(b: &[u8; 32]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(64);
    for x in b {
        out.push(HEX[(x >> 4) as usize] as char);
        out.push(HEX[(x & 0xF) as usize] as char);
    }
    out
}

fn hex_val(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(10 + (b - b'a')),
        b'A'..=b'F' => Some(10 + (b - b'A')),
        _ => None,
    }
}

#[derive(Debug, Clone)]
pub struct Commit {
    pub parents: Vec<String>,
    pub base: Option<String>,
    pub patch: String,
    pub result: String,
    pub obligations: Vec<String>,
    pub evidence: Vec<String>,
    pub attestations: Vec<String>,
    pub message: String,
}

impl Commit {
    pub fn from_term(t: &Term) -> Result<Self, SchemaError> {
        let m = req_map(t, "commit")?;
        let ty = req_sym(m, ":type", "commit")?;
        if ty != ":vcs/commit" {
            return Err(SchemaError::Bad(format!("commit: wrong :type {ty}")));
        }
        let v = req_int(m, ":v", "commit")?;
        if v != 1 {
            return Err(SchemaError::Bad(format!("commit: unsupported :v {v}")));
        }

        let parents = opt_vec_hex(m, ":parents", "commit")?;
        let base = match get(m, ":base") {
            None => None,
            Some(Term::Str(s)) => {
                validate_hex_hash(s)
                    .map_err(|e| SchemaError::Bad(format!("commit: :base: {e}")))?;
                Some(s.clone())
            }
            Some(Term::Nil) => None,
            Some(other) => {
                return Err(SchemaError::Bad(format!(
                    "commit: :base must be hex string or nil, got {}",
                    print_term(other)
                )));
            }
        };
        let patch = match get(m, ":patch") {
            Some(Term::Str(s)) => {
                validate_hex_hash(s)
                    .map_err(|e| SchemaError::Bad(format!("commit: :patch: {e}")))?;
                s.clone()
            }
            Some(other) => {
                return Err(SchemaError::Bad(format!(
                    "commit: :patch must be hex string, got {}",
                    print_term(other)
                )));
            }
            None => return Err(SchemaError::Bad("commit: missing :patch".to_string())),
        };
        let result = match get(m, ":result") {
            Some(Term::Str(s)) => {
                validate_hex_hash(s)
                    .map_err(|e| SchemaError::Bad(format!("commit: :result: {e}")))?;
                s.clone()
            }
            Some(other) => {
                return Err(SchemaError::Bad(format!(
                    "commit: :result must be hex string, got {}",
                    print_term(other)
                )));
            }
            None => return Err(SchemaError::Bad("commit: missing :result".to_string())),
        };

        let obligations = opt_vec_str(m, ":obligations", "commit")?;
        let evidence = opt_vec_hex(m, ":evidence", "commit")?;
        let attestations = opt_vec_hex(m, ":attestations", "commit")?;

        let message = match get(m, ":message") {
            Some(Term::Str(s)) => s.clone(),
            Some(other) => {
                return Err(SchemaError::Bad(format!(
                    "commit: :message must be string, got {}",
                    print_term(other)
                )));
            }
            None => return Err(SchemaError::Bad("commit: missing :message".to_string())),
        };

        Ok(Self {
            parents,
            base,
            patch,
            result,
            obligations,
            evidence,
            attestations,
            message,
        })
    }
}

/// Compute the stable "signing hash" for a `:vcs/commit` artifact.
///
/// This avoids self-referential cycles by hashing the canonical commit term with `:attestations`
/// forced to `[]` (regardless of what the stored commit contains).
pub fn commit_signing_hash(commit_term: &Term) -> Result<[u8; 32], SchemaError> {
    let Term::Map(m) = commit_term else {
        return Err(SchemaError::Bad(
            "commit_signing_hash: expected map".to_string(),
        ));
    };
    let ty = req_sym(m, ":type", "commit")?;
    if ty != ":vcs/commit" {
        return Err(SchemaError::Bad(
            "commit_signing_hash: not a :vcs/commit".to_string(),
        ));
    }
    let mut mm = m.clone();
    mm.insert(
        TermOrdKey(Term::symbol(":attestations")),
        Term::Vector(Vec::new()),
    );
    let canonical = print_term(&Term::Map(mm));
    let mut h = Hasher::new();
    h.update(b"GCv0.2\0vcs\0commit-signing-hash\0");
    h.update(canonical.as_bytes());
    Ok(*h.finalize().as_bytes())
}

#[derive(Debug, Clone)]
pub struct Evidence {
    pub kind: String,
    pub inputs: Vec<String>,
    pub outputs: Vec<String>,
    pub data: Option<String>,
}

impl Evidence {
    pub fn from_term(t: &Term) -> Result<Self, SchemaError> {
        let m = req_map(t, "evidence")?;
        let ty = req_sym(m, ":type", "evidence")?;
        if ty != ":vcs/evidence" {
            return Err(SchemaError::Bad(format!("evidence: wrong :type {ty}")));
        }
        let v = req_int(m, ":v", "evidence")?;
        if v != 1 {
            return Err(SchemaError::Bad(format!("evidence: unsupported :v {v}")));
        }
        let kind = req_sym(m, ":kind", "evidence")?;

        // Optional reachability pointers (all content-addressed).
        let inputs = opt_vec_hex(m, ":inputs", "evidence")?;
        let outputs = opt_vec_hex(m, ":outputs", "evidence")?;
        let data = match get(m, ":data") {
            None | Some(Term::Nil) => None,
            Some(Term::Str(s)) => {
                validate_hex_hash(s)
                    .map_err(|e| SchemaError::Bad(format!("evidence: :data: {e}")))?;
                Some(s.clone())
            }
            // Inline evidence payloads are allowed; only hash strings participate in reachability.
            Some(_) => None,
        };

        Ok(Self {
            kind,
            inputs,
            outputs,
            data,
        })
    }

    pub fn refs(&self) -> Vec<String> {
        let mut out = Vec::new();
        out.extend(self.inputs.iter().cloned());
        out.extend(self.outputs.iter().cloned());
        if let Some(d) = &self.data {
            out.push(d.clone());
        }
        out
    }
}

#[derive(Debug, Clone)]
pub struct Attestation {
    pub alg: String,
    pub signing_hash: [u8; 32],
    pub pk: [u8; 32],
    pub sig: [u8; 64],
    pub role: Option<String>,
}

#[derive(Debug, Clone)]
pub struct ConflictEntry {
    pub op: String,
    pub base: Option<String>,
    pub left: Option<String>,
    pub right: Option<String>,
}

#[derive(Debug, Clone)]
pub struct Conflict {
    pub kind: String,
    pub base: String,
    pub left: String,
    pub right: String,
    pub conflicts: Vec<ConflictEntry>,
}

impl Conflict {
    pub fn from_term(t: &Term) -> Result<Self, SchemaError> {
        let m = req_map(t, "conflict")?;
        let ty = req_sym(m, ":type", "conflict")?;
        if ty != ":vcs/conflict" {
            return Err(SchemaError::Bad(format!("conflict: wrong :type {ty}")));
        }
        let v = req_int(m, ":v", "conflict")?;
        if v != 1 {
            return Err(SchemaError::Bad(format!("conflict: unsupported :v {v}")));
        }
        let kind = req_sym(m, ":kind", "conflict")?;
        let base = match get(m, ":base") {
            Some(Term::Str(s)) => {
                validate_hex_hash(s)
                    .map_err(|e| SchemaError::Bad(format!("conflict: :base: {e}")))?;
                s.clone()
            }
            Some(other) => {
                return Err(SchemaError::Bad(format!(
                    "conflict: :base must be hex string, got {}",
                    print_term(other)
                )));
            }
            None => return Err(SchemaError::Bad("conflict: missing :base".to_string())),
        };
        let left = match get(m, ":left") {
            Some(Term::Str(s)) => {
                validate_hex_hash(s)
                    .map_err(|e| SchemaError::Bad(format!("conflict: :left: {e}")))?;
                s.clone()
            }
            Some(other) => {
                return Err(SchemaError::Bad(format!(
                    "conflict: :left must be hex string, got {}",
                    print_term(other)
                )));
            }
            None => return Err(SchemaError::Bad("conflict: missing :left".to_string())),
        };
        let right = match get(m, ":right") {
            Some(Term::Str(s)) => {
                validate_hex_hash(s)
                    .map_err(|e| SchemaError::Bad(format!("conflict: :right: {e}")))?;
                s.clone()
            }
            Some(other) => {
                return Err(SchemaError::Bad(format!(
                    "conflict: :right must be hex string, got {}",
                    print_term(other)
                )));
            }
            None => return Err(SchemaError::Bad("conflict: missing :right".to_string())),
        };

        let ct = get(m, ":conflicts")
            .ok_or_else(|| SchemaError::Bad("conflict: missing :conflicts".to_string()))?;
        let Term::Vector(xs) = ct else {
            return Err(SchemaError::Bad(format!(
                "conflict: :conflicts must be vector, got {}",
                print_term(ct)
            )));
        };
        let mut conflicts = Vec::new();
        for x in xs {
            let Term::Map(mm) = x else {
                return Err(SchemaError::Bad(format!(
                    "conflict: entry must be map, got {}",
                    print_term(x)
                )));
            };
            let op = req_sym(mm, ":op", "conflict entry")?;
            let basev = opt_hex_or_nil(mm, ":base", "conflict entry")?;
            let leftv = opt_hex_or_nil(mm, ":left", "conflict entry")?;
            let rightv = opt_hex_or_nil(mm, ":right", "conflict entry")?;
            conflicts.push(ConflictEntry {
                op,
                base: basev,
                left: leftv,
                right: rightv,
            });
        }

        Ok(Self {
            kind,
            base,
            left,
            right,
            conflicts,
        })
    }

    pub fn refs(&self) -> Vec<String> {
        let mut out = Vec::new();
        out.push(self.base.clone());
        out.push(self.left.clone());
        out.push(self.right.clone());
        for c in &self.conflicts {
            if let Some(h) = &c.base {
                out.push(h.clone());
            }
            if let Some(h) = &c.left {
                out.push(h.clone());
            }
            if let Some(h) = &c.right {
                out.push(h.clone());
            }
        }
        out
    }
}

fn opt_hex_or_nil(
    m: &BTreeMap<TermOrdKey, Term>,
    k: &str,
    what: &str,
) -> Result<Option<String>, SchemaError> {
    match get(m, k) {
        None | Some(Term::Nil) => Ok(None),
        Some(Term::Str(s)) => {
            validate_hex_hash(s).map_err(|e| SchemaError::Bad(format!("{what}: {k}: {e}")))?;
            Ok(Some(s.clone()))
        }
        Some(other) => Err(SchemaError::Bad(format!(
            "{what}: {k} must be hex string or nil, got {}",
            print_term(other)
        ))),
    }
}

impl Attestation {
    pub fn from_term(t: &Term) -> Result<Self, SchemaError> {
        let m = req_map(t, "attestation")?;
        let ty = req_sym(m, ":type", "attestation")?;
        if ty != ":vcs/attestation" {
            return Err(SchemaError::Bad(format!("attestation: wrong :type {ty}")));
        }
        let v = req_int(m, ":v", "attestation")?;
        if v != 1 {
            return Err(SchemaError::Bad(format!("attestation: unsupported :v {v}")));
        }
        let alg = match get(m, ":alg") {
            Some(Term::Str(s)) => s.clone(),
            Some(other) => {
                return Err(SchemaError::Bad(format!(
                    "attestation: :alg must be string, got {}",
                    print_term(other)
                )));
            }
            None => return Err(SchemaError::Bad("attestation: missing :alg".to_string())),
        };
        let signing_hash = match get(m, ":signing-h") {
            Some(Term::Bytes(b)) if b.len() == 32 => {
                let mut out = [0u8; 32];
                out.copy_from_slice(b);
                out
            }
            Some(other) => {
                return Err(SchemaError::Bad(format!(
                    "attestation: :signing-h must be 32-byte bytes, got {}",
                    print_term(other)
                )));
            }
            None => {
                return Err(SchemaError::Bad(
                    "attestation: missing :signing-h".to_string(),
                ));
            }
        };

        let pk = match get(m, ":pk") {
            Some(Term::Bytes(b)) if b.len() == 32 => {
                let mut out = [0u8; 32];
                out.copy_from_slice(b);
                out
            }
            Some(other) => {
                return Err(SchemaError::Bad(format!(
                    "attestation: :pk must be 32-byte bytes, got {}",
                    print_term(other)
                )));
            }
            None => return Err(SchemaError::Bad("attestation: missing :pk".to_string())),
        };
        let sig = match get(m, ":sig") {
            Some(Term::Bytes(b)) if b.len() == 64 => {
                let mut out = [0u8; 64];
                out.copy_from_slice(b);
                out
            }
            Some(other) => {
                return Err(SchemaError::Bad(format!(
                    "attestation: :sig must be 64-byte bytes, got {}",
                    print_term(other)
                )));
            }
            None => return Err(SchemaError::Bad("attestation: missing :sig".to_string())),
        };
        let role = match get(m, ":role") {
            None | Some(Term::Nil) => None,
            Some(Term::Str(s)) => {
                let trimmed = s.trim();
                if trimmed.is_empty() {
                    return Err(SchemaError::Bad(
                        "attestation: :role cannot be empty".to_string(),
                    ));
                }
                Some(trimmed.to_string())
            }
            Some(Term::Symbol(s)) => {
                let trimmed = s.trim();
                if trimmed.is_empty() {
                    return Err(SchemaError::Bad(
                        "attestation: :role cannot be empty".to_string(),
                    ));
                }
                Some(trimmed.to_string())
            }
            Some(other) => {
                return Err(SchemaError::Bad(format!(
                    "attestation: :role must be string/symbol or nil, got {}",
                    print_term(other)
                )));
            }
        };

        Ok(Self {
            alg,
            signing_hash,
            pk,
            sig,
            role,
        })
    }
}
