use std::collections::{BTreeMap, BTreeSet};

use base64ct::{Base64, Encoding};
use ed25519_dalek::VerifyingKey;
use gc_coreform::{Term, TermOrdKey, print_term};
use globset::{Glob, GlobSet, GlobSetBuilder};
use thiserror::Error;

use crate::schema::SchemaError;

#[derive(Debug, Error)]
pub enum PolicyError {
    #[error("policy schema error: {0}")]
    Schema(String),
    #[error("policy invalid: {0}")]
    Invalid(String),
}

#[derive(Debug, Clone)]
pub struct PolicyClass {
    pub name: String,
    patterns: GlobSet,
    excludes: GlobSet,
    pub required_obligations: Vec<String>,
    pub required_evidence_kinds: Vec<String>,
    pub obligation_evidence_kinds: BTreeMap<String, Vec<String>>,
    pub require_signatures: bool,
    pub min_signatures: u64,
    pub allowed_public_keys: Vec<VerifyingKey>,
}

impl PolicyClass {
    pub fn matches(&self, refname: &str) -> bool {
        self.patterns.is_match(refname) && !self.excludes.is_match(refname)
    }

    pub fn normalize_evidence_kind(kind: &str) -> String {
        let trimmed = kind.trim();
        if trimmed.starts_with(':') {
            trimmed.to_string()
        } else {
            format!(":{trimmed}")
        }
    }

    pub fn required_evidence_kind_set(&self, obligations: &[String]) -> BTreeSet<String> {
        let mut out: BTreeSet<String> = self
            .required_evidence_kinds
            .iter()
            .map(|k| Self::normalize_evidence_kind(k))
            .collect();
        let obligation_set: BTreeSet<&str> = obligations.iter().map(String::as_str).collect();
        for (ob, kinds) in &self.obligation_evidence_kinds {
            if !obligation_set.contains(ob.as_str()) {
                continue;
            }
            for k in kinds {
                out.insert(Self::normalize_evidence_kind(k));
            }
        }
        out
    }

    pub fn missing_required_evidence_kinds(
        &self,
        obligations: &[String],
        observed_kinds: &BTreeSet<String>,
    ) -> Vec<String> {
        let mut missing: Vec<String> = Vec::new();
        let required = self.required_evidence_kind_set(obligations);
        for k in required {
            if !observed_kinds.contains(&k) {
                missing.push(k);
            }
        }
        missing
    }
}

#[derive(Debug, Clone)]
pub struct Policy {
    pub name: Option<String>,
    pub frozen_prefixes: Vec<String>,
    pub dev: Option<PolicyClass>,
    pub main: Option<PolicyClass>,
    pub tags: Option<PolicyClass>,
}

impl Policy {
    pub fn from_term(t: &Term) -> Result<Self, PolicyError> {
        let Term::Map(m) = t else {
            return Err(PolicyError::Schema("policy must be a map".to_string()));
        };
        let ty = req_sym(m, ":type", "policy")?;
        if ty != ":vcs/policy" {
            return Err(PolicyError::Schema(format!("wrong :type {ty}")));
        }
        let v = req_i64(m, ":v", "policy")?;
        if v != 1 {
            return Err(PolicyError::Schema(format!("unsupported :v {v}")));
        }
        let name = match m.get(&TermOrdKey(Term::symbol(":name"))) {
            Some(Term::Str(s)) => Some(s.clone()),
            Some(Term::Nil) | None => None,
            Some(other) => {
                return Err(PolicyError::Schema(format!(
                    ":name must be string or nil, got {}",
                    print_term(other)
                )));
            }
        };

        let frozen_prefixes = parse_frozen_prefixes(m)?;

        let classes = match m.get(&TermOrdKey(Term::symbol(":classes"))) {
            Some(Term::Map(c)) => c,
            Some(Term::Nil) | None => {
                return Err(PolicyError::Schema(
                    "policy missing :classes map".to_string(),
                ));
            }
            Some(other) => {
                return Err(PolicyError::Schema(format!(
                    ":classes must be a map, got {}",
                    print_term(other)
                )));
            }
        };

        let dev = parse_class(classes, ":dev")?;
        let main = parse_class(classes, ":main")?;
        let tags = parse_class(classes, ":tags")?;

        Ok(Self {
            name,
            frozen_prefixes,
            dev,
            main,
            tags,
        })
    }

    pub fn class_for_ref(&self, refname: &str) -> Option<&PolicyClass> {
        // Deterministic precedence.
        if let Some(c) = &self.tags
            && c.matches(refname)
        {
            return Some(c);
        }
        if let Some(c) = &self.main
            && c.matches(refname)
        {
            return Some(c);
        }
        if let Some(c) = &self.dev
            && c.matches(refname)
        {
            return Some(c);
        }
        None
    }

    pub fn is_frozen_ref(&self, refname: &str) -> bool {
        self.frozen_prefixes.iter().any(|p| refname.starts_with(p))
    }
}

fn parse_frozen_prefixes(m: &BTreeMap<TermOrdKey, Term>) -> Result<Vec<String>, PolicyError> {
    let Some(Term::Map(refs)) = m.get(&TermOrdKey(Term::symbol(":refs"))) else {
        return Ok(Vec::new());
    };
    let Some(Term::Vector(xs)) = refs.get(&TermOrdKey(Term::symbol(":frozen-prefixes"))) else {
        return Ok(Vec::new());
    };
    let mut out = Vec::new();
    for x in xs {
        match x {
            Term::Str(s) => out.push(s.clone()),
            _ => {
                return Err(PolicyError::Schema(
                    ":refs.:frozen-prefixes entries must be strings".to_string(),
                ));
            }
        }
    }
    Ok(out)
}

fn parse_class(
    classes: &BTreeMap<TermOrdKey, Term>,
    key: &str,
) -> Result<Option<PolicyClass>, PolicyError> {
    let Some(t) = classes.get(&TermOrdKey(Term::symbol(key))) else {
        return Ok(None);
    };
    if matches!(t, Term::Nil) {
        return Ok(None);
    }
    let Term::Map(m) = t else {
        return Err(PolicyError::Schema(format!(
            "class {key} must be a map, got {}",
            print_term(t)
        )));
    };

    let patterns = req_vec_str(m, ":patterns", "policy class")?;
    if patterns.is_empty() {
        return Err(PolicyError::Invalid(format!("{key}: empty :patterns")));
    }
    let excludes = opt_vec_str(m, ":exclude")?;
    let required_obligations = opt_vec_str_or_sym(m, ":required-obligations")?;
    let required_evidence_kinds =
        normalize_kind_vec(opt_vec_str_or_sym(m, ":required-evidence-kinds")?);
    let obligation_evidence_kinds =
        parse_obligation_evidence_kind_map(m, key, ":obligation-evidence-kinds")?;

    let require_signatures = match m.get(&TermOrdKey(Term::symbol(":require-signatures"))) {
        Some(Term::Bool(b)) => *b,
        Some(Term::Nil) | None => false,
        Some(other) => {
            return Err(PolicyError::Schema(format!(
                "{key}: :require-signatures must be bool, got {}",
                print_term(other)
            )));
        }
    };

    let min_signatures = match m.get(&TermOrdKey(Term::symbol(":min-signatures"))) {
        Some(Term::Int(i)) => {
            use num_traits::ToPrimitive;
            i.to_u64().ok_or_else(|| {
                PolicyError::Schema(format!("{key}: :min-signatures out of range"))
            })?
        }
        Some(Term::Nil) | None => {
            if require_signatures {
                1
            } else {
                0
            }
        }
        Some(other) => {
            return Err(PolicyError::Schema(format!(
                "{key}: :min-signatures must be int, got {}",
                print_term(other)
            )));
        }
    };

    let allowed_public_keys = match m.get(&TermOrdKey(Term::symbol(":allowed-public-keys"))) {
        Some(Term::Vector(xs)) => {
            let mut out = Vec::new();
            for x in xs {
                let Term::Str(s) = x else {
                    return Err(PolicyError::Schema(format!(
                        "{key}: :allowed-public-keys entries must be base64 strings"
                    )));
                };
                let pk = decode_pk_b64(s).map_err(PolicyError::Invalid)?;
                let vk = VerifyingKey::from_bytes(&pk)
                    .map_err(|e| PolicyError::Invalid(format!("{key}: bad pk: {e}")))?;
                out.push(vk);
            }
            out
        }
        Some(Term::Nil) | None => Vec::new(),
        Some(other) => {
            return Err(PolicyError::Schema(format!(
                "{key}: :allowed-public-keys must be vector, got {}",
                print_term(other)
            )));
        }
    };

    if min_signatures > 0 && allowed_public_keys.is_empty() {
        return Err(PolicyError::Invalid(format!(
            "{key}: min_signatures > 0 but allowed_public_keys is empty"
        )));
    }

    let patterns = compile_globs(&patterns).map_err(PolicyError::Invalid)?;
    let excludes = compile_globs(&excludes).map_err(PolicyError::Invalid)?;

    Ok(Some(PolicyClass {
        name: key.trim_start_matches(':').to_string(),
        patterns,
        excludes,
        required_obligations,
        required_evidence_kinds,
        obligation_evidence_kinds,
        require_signatures,
        min_signatures,
        allowed_public_keys,
    }))
}

fn normalize_kind_vec(xs: Vec<String>) -> Vec<String> {
    let mut set: BTreeSet<String> = BTreeSet::new();
    for x in xs {
        set.insert(PolicyClass::normalize_evidence_kind(&x));
    }
    set.into_iter().collect()
}

fn parse_obligation_evidence_kind_map(
    m: &BTreeMap<TermOrdKey, Term>,
    class_key: &str,
    field: &str,
) -> Result<BTreeMap<String, Vec<String>>, PolicyError> {
    let Some(t) = m.get(&TermOrdKey(Term::symbol(field))) else {
        return Ok(BTreeMap::new());
    };
    let Term::Map(mm) = t else {
        return Err(PolicyError::Schema(format!(
            "{class_key}: {field} must be map, got {}",
            print_term(t)
        )));
    };
    let mut out: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for (k, v) in mm {
        let ob = match &k.0 {
            Term::Str(s) => s.clone(),
            Term::Symbol(s) => s.clone(),
            other => {
                return Err(PolicyError::Schema(format!(
                    "{class_key}: {field} keys must be strings/symbols, got {}",
                    print_term(other)
                )));
            }
        };
        let Term::Vector(xs) = v else {
            return Err(PolicyError::Schema(format!(
                "{class_key}: {field}[{ob}] must be vector, got {}",
                print_term(v)
            )));
        };
        let mut kinds: Vec<String> = Vec::new();
        for x in xs {
            match x {
                Term::Str(s) => kinds.push(PolicyClass::normalize_evidence_kind(s)),
                Term::Symbol(s) => kinds.push(PolicyClass::normalize_evidence_kind(s)),
                other => {
                    return Err(PolicyError::Schema(format!(
                        "{class_key}: {field}[{ob}] entries must be strings/symbols, got {}",
                        print_term(other)
                    )));
                }
            }
        }
        let mut dedup = BTreeSet::new();
        for k in kinds {
            dedup.insert(k);
        }
        out.insert(ob, dedup.into_iter().collect());
    }
    Ok(out)
}

fn compile_globs(pats: &[String]) -> Result<GlobSet, String> {
    let mut b = GlobSetBuilder::new();
    for p in pats {
        let g = Glob::new(p).map_err(|e| format!("bad glob pattern {p}: {e}"))?;
        b.add(g);
    }
    b.build().map_err(|e| format!("globset build: {e}"))
}

fn decode_pk_b64(s: &str) -> Result<[u8; 32], String> {
    let mut out = [0u8; 32];
    Base64::decode(s, &mut out).map_err(|e| format!("invalid base64 pk: {e}"))?;
    Ok(out)
}

fn req_sym(m: &BTreeMap<TermOrdKey, Term>, k: &str, what: &str) -> Result<String, PolicyError> {
    match m.get(&TermOrdKey(Term::symbol(k))) {
        Some(Term::Symbol(s)) => Ok(s.clone()),
        Some(other) => Err(PolicyError::Schema(format!(
            "{what}: {k} must be symbol, got {}",
            print_term(other)
        ))),
        None => Err(PolicyError::Schema(format!("{what}: missing {k}"))),
    }
}

fn req_i64(m: &BTreeMap<TermOrdKey, Term>, k: &str, what: &str) -> Result<i64, PolicyError> {
    match m.get(&TermOrdKey(Term::symbol(k))) {
        Some(Term::Int(i)) => {
            use num_traits::ToPrimitive;
            i.to_i64()
                .ok_or_else(|| PolicyError::Schema(format!("{what}: {k} out of range")))
        }
        Some(other) => Err(PolicyError::Schema(format!(
            "{what}: {k} must be int, got {}",
            print_term(other)
        ))),
        None => Err(PolicyError::Schema(format!("{what}: missing {k}"))),
    }
}

fn req_vec_str(
    m: &BTreeMap<TermOrdKey, Term>,
    k: &str,
    what: &str,
) -> Result<Vec<String>, PolicyError> {
    let Some(t) = m.get(&TermOrdKey(Term::symbol(k))) else {
        return Err(PolicyError::Schema(format!("{what}: missing {k}")));
    };
    let Term::Vector(xs) = t else {
        return Err(PolicyError::Schema(format!(
            "{what}: {k} must be vector, got {}",
            print_term(t)
        )));
    };
    let mut out = Vec::new();
    for x in xs {
        match x {
            Term::Str(s) => out.push(s.clone()),
            _ => {
                return Err(PolicyError::Schema(format!(
                    "{what}: {k} entries must be strings"
                )));
            }
        }
    }
    Ok(out)
}

fn opt_vec_str(m: &BTreeMap<TermOrdKey, Term>, k: &str) -> Result<Vec<String>, PolicyError> {
    let Some(t) = m.get(&TermOrdKey(Term::symbol(k))) else {
        return Ok(Vec::new());
    };
    let Term::Vector(xs) = t else {
        return Err(PolicyError::Schema(format!(
            "{k} must be vector, got {}",
            print_term(t)
        )));
    };
    let mut out = Vec::new();
    for x in xs {
        let Term::Str(s) = x else {
            return Err(PolicyError::Schema(format!("{k} entries must be strings")));
        };
        out.push(s.clone());
    }
    Ok(out)
}

fn opt_vec_str_or_sym(m: &BTreeMap<TermOrdKey, Term>, k: &str) -> Result<Vec<String>, PolicyError> {
    let Some(t) = m.get(&TermOrdKey(Term::symbol(k))) else {
        return Ok(Vec::new());
    };
    let Term::Vector(xs) = t else {
        return Err(PolicyError::Schema(format!(
            "{k} must be vector, got {}",
            print_term(t)
        )));
    };
    let mut out = Vec::new();
    for x in xs {
        match x {
            Term::Str(s) => out.push(s.clone()),
            Term::Symbol(s) => out.push(s.clone()),
            _ => {
                return Err(PolicyError::Schema(format!(
                    "{k} entries must be strings/symbols"
                )));
            }
        }
    }
    Ok(out)
}

impl From<SchemaError> for PolicyError {
    fn from(e: SchemaError) -> Self {
        PolicyError::Schema(format!("{e}"))
    }
}

#[cfg(test)]
mod tests {
    use super::Policy;
    use gc_coreform::parse_term;
    use std::collections::BTreeSet;

    #[test]
    fn policy_parses_required_evidence_kinds_and_obligation_mapping() {
        let t = parse_term(
            r#"
            {
              :type :vcs/policy
              :v 1
              :classes {
                :main {
                  :patterns ["refs/**/heads/main"]
                  :required-obligations [core/obligation::unit-tests]
                  :required-evidence-kinds [:unit-tests]
                  :obligation-evidence-kinds {
                    core/obligation::unit-tests [:effect-log]
                  }
                }
              }
            }
            "#,
        )
        .expect("policy term");

        let pol = Policy::from_term(&t).expect("policy parse");
        let class = pol.class_for_ref("refs/heads/main").expect("class");
        let required = class.required_evidence_kind_set(&[
            "core/obligation::unit-tests".to_string(),
            "core/obligation::other".to_string(),
        ]);
        assert!(required.contains(":unit-tests"));
        assert!(required.contains(":effect-log"));

        let observed: BTreeSet<String> = [":unit-tests".to_string()].into_iter().collect();
        let missing = class.missing_required_evidence_kinds(
            &["core/obligation::unit-tests".to_string()],
            &observed,
        );
        assert_eq!(missing, vec![":effect-log".to_string()]);
    }
}
