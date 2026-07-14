use std::cmp::Ordering;
use std::collections::BTreeMap;

use blake3::Hasher;
use bytes::Bytes;
use num_bigint::BigInt;

pub const LANGUAGE_PROFILE_ID: &str = "genesis/language-profile/v0.2";
pub const COREFORM_PROFILE_ID: &str = "genesis/coreform/v0.2";
pub const HASH_PROFILE_ID: &str = "genesis/hash-profile/gcv0.2-blake3";
pub const HASH_DOMAIN_PREFIX: &[u8] = b"GCv0.2\0";

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Term {
    Nil,
    Bool(bool),
    Int(BigInt),
    Str(String),
    Bytes(Bytes),
    Symbol(String),
    Pair(Box<Term>, Box<Term>),
    Vector(Vec<Term>),
    Map(BTreeMap<TermOrdKey, Term>),
}

/// A stable total ordering wrapper for `Term` keys.
///
/// We don't implement `Ord` on `Term` directly because the full `Term` is used
/// as both code and data; for maps we only need stable ordering for keys.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TermOrdKey(pub Term);

impl PartialOrd for TermOrdKey {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for TermOrdKey {
    fn cmp(&self, other: &Self) -> Ordering {
        cmp_term(&self.0, &other.0)
    }
}

fn tag(t: &Term) -> u8 {
    match t {
        Term::Nil => 0,
        Term::Bool(_) => 1,
        Term::Int(_) => 2,
        Term::Str(_) => 3,
        Term::Bytes(_) => 4,
        Term::Symbol(_) => 5,
        Term::Pair(_, _) => 6,
        Term::Vector(_) => 7,
        Term::Map(_) => 8,
    }
}

fn cmp_term(a: &Term, b: &Term) -> Ordering {
    // Term ordering is structurally recursive; grow stack as needed to avoid overflow on deep terms.
    stacker::maybe_grow(32 * 1024, 8 * 1024 * 1024, || cmp_term_impl(a, b))
}

fn cmp_term_impl(a: &Term, b: &Term) -> Ordering {
    let ta = tag(a);
    let tb = tag(b);
    match ta.cmp(&tb) {
        Ordering::Equal => {}
        ord => return ord,
    }

    match (a, b) {
        (Term::Nil, Term::Nil) => Ordering::Equal,
        (Term::Bool(x), Term::Bool(y)) => x.cmp(y),
        (Term::Int(x), Term::Int(y)) => x.cmp(y),
        (Term::Str(x), Term::Str(y)) => x.cmp(y),
        (Term::Bytes(x), Term::Bytes(y)) => x.as_ref().cmp(y.as_ref()),
        (Term::Symbol(x), Term::Symbol(y)) => x.cmp(y),
        (Term::Pair(ax, ad), Term::Pair(bx, bd)) => {
            let c1 = cmp_term(ax, bx);
            if c1 != Ordering::Equal {
                return c1;
            }
            cmp_term(ad, bd)
        }
        (Term::Vector(x), Term::Vector(y)) => x
            .iter()
            .zip(y.iter())
            .map(|(a, b)| cmp_term(a, b))
            .find(|o| *o != Ordering::Equal)
            .unwrap_or_else(|| x.len().cmp(&y.len())),
        (Term::Map(x), Term::Map(y)) => x
            .iter()
            .zip(y.iter())
            .map(|((ak, av), (bk, bv))| {
                let c1 = ak.cmp(bk);
                if c1 != Ordering::Equal {
                    return c1;
                }
                cmp_term(av, bv)
            })
            .find(|o| *o != Ordering::Equal)
            .unwrap_or_else(|| x.len().cmp(&y.len())),
        _ => Ordering::Equal,
    }
}

impl Term {
    pub fn symbol(s: impl Into<String>) -> Self {
        Term::Symbol(s.into())
    }

    pub fn list(items: Vec<Term>) -> Self {
        let mut cur = Term::Nil;
        for item in items.into_iter().rev() {
            cur = Term::Pair(Box::new(item), Box::new(cur));
        }
        cur
    }

    pub fn as_proper_list(&self) -> Option<Vec<&Term>> {
        let mut out = Vec::new();
        let mut cur = self;
        loop {
            match cur {
                Term::Nil => return Some(out),
                Term::Pair(a, d) => {
                    out.push(a.as_ref());
                    cur = d.as_ref();
                }
                _ => return None,
            }
        }
    }
}

pub fn hash_term(term: &Term) -> [u8; 32] {
    let s = crate::print::print_term(term);
    let mut h = Hasher::new();
    h.update(HASH_DOMAIN_PREFIX);
    h.update(s.as_bytes());
    *h.finalize().as_bytes()
}

pub fn hash_module(forms: &[Term]) -> [u8; 32] {
    let s = crate::print::print_module(forms);
    let mut h = Hasher::new();
    h.update(HASH_DOMAIN_PREFIX);
    h.update(b"module\0");
    h.update(s.as_bytes());
    *h.finalize().as_bytes()
}
