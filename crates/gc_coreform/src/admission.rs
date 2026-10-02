//! Admission to the v0.2 canonical domain, distinct from arbitrary runtime data.

use std::collections::btree_map;
use std::slice;

use thiserror::Error;

use crate::{Term, TermOrdKey};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum SymbolNameError {
    #[error("symbol text is empty")]
    Empty,
    #[error("symbol text is a reserved literal")]
    Literal,
    #[error("symbol text starts an integer token")]
    IntegerPrefix,
    #[error("symbol text contains a delimiter at byte {at}")]
    Delimiter { at: usize },
}

pub(crate) fn is_symbol_delimiter(byte: u8) -> bool {
    matches!(
        byte,
        b' ' | b'\t'
            | b'\n'
            | b'\r'
            | b'('
            | b')'
            | b'['
            | b']'
            | b'{'
            | b'}'
            | b'\''
            | b'"'
            | b';'
    )
}

/// A name must parse as exactly one symbol with identical text, under v0.2.
/// Unicode scalars other than the lexer’s ASCII delimiters remain name bytes;
/// `.` has no dotted-list role in this source profile.
pub fn validate_symbol_name(name: &str) -> Result<(), SymbolNameError> {
    let bytes = name.as_bytes();
    let Some(first) = bytes.first() else {
        return Err(SymbolNameError::Empty);
    };
    if matches!(name, "nil" | "true" | "false") {
        return Err(SymbolNameError::Literal);
    }
    if first.is_ascii_digit() || (*first == b'-' && bytes.get(1).is_some_and(u8::is_ascii_digit)) {
        return Err(SymbolNameError::IntegerPrefix);
    }
    if let Some(at) = bytes.iter().position(|byte| is_symbol_delimiter(*byte)) {
        return Err(SymbolNameError::Delimiter { at });
    }
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum CanonicalDomainError {
    #[error("noncanonical symbol: {0}")]
    Symbol(#[from] SymbolNameError),
    #[error("improper pair has no canonical CoreForm v0.2 serialization")]
    ImproperPair,
    #[error("canonical admission work allocation failed")]
    WorkAllocation,
}

enum Work<'a> {
    Term(&'a Term),
    Vector(slice::Iter<'a, Term>),
    Map(btree_map::Iter<'a, TermOrdKey, Term>),
}

fn push<'a>(stack: &mut Vec<Work<'a>>, item: Work<'a>) -> Result<(), CanonicalDomainError> {
    stack
        .try_reserve(1)
        .map_err(|_| CanonicalDomainError::WorkAllocation)?;
    stack.push(item);
    Ok(())
}

/// Check every key, value and child without cloning data, recursive host calls,
/// or allocating one work item per entry of a wide container.
pub fn validate_canonical_term(term: &Term) -> Result<(), CanonicalDomainError> {
    let mut stack = Vec::new();
    push(&mut stack, Work::Term(term))?;
    while let Some(work) = stack.pop() {
        match work {
            Work::Term(Term::Symbol(name)) => validate_symbol_name(name)?,
            Work::Term(Term::Pair(car, cdr)) => {
                if !matches!(cdr.as_ref(), Term::Nil | Term::Pair(_, _)) {
                    return Err(CanonicalDomainError::ImproperPair);
                }
                push(&mut stack, Work::Term(cdr))?;
                push(&mut stack, Work::Term(car))?;
            }
            Work::Term(Term::Vector(values)) => push(&mut stack, Work::Vector(values.iter()))?,
            Work::Term(Term::Map(entries)) => push(&mut stack, Work::Map(entries.iter()))?,
            Work::Term(_) => {}
            Work::Vector(mut values) => {
                if let Some(value) = values.next() {
                    push(&mut stack, Work::Vector(values))?;
                    push(&mut stack, Work::Term(value))?;
                }
            }
            Work::Map(mut entries) => {
                if let Some((key, value)) = entries.next() {
                    push(&mut stack, Work::Map(entries))?;
                    push(&mut stack, Work::Term(value))?;
                    push(&mut stack, Work::Term(&key.0))?;
                }
            }
        }
    }
    Ok(())
}

pub fn print_term_checked(term: &Term) -> Result<String, CanonicalDomainError> {
    validate_canonical_term(term)?;
    Ok(crate::print_term(term))
}

pub fn print_term_compact_checked(term: &Term) -> Result<String, CanonicalDomainError> {
    validate_canonical_term(term)?;
    Ok(crate::print_term_compact(term))
}

pub fn hash_term_checked(term: &Term) -> Result<[u8; 32], CanonicalDomainError> {
    validate_canonical_term(term)?;
    Ok(crate::hash_term(term))
}

pub fn print_module_checked(forms: &[Term]) -> Result<String, CanonicalDomainError> {
    for form in forms {
        validate_canonical_term(form)?;
    }
    Ok(crate::print_module(forms))
}

pub fn hash_module_checked(forms: &[Term]) -> Result<[u8; 32], CanonicalDomainError> {
    for form in forms {
        validate_canonical_term(form)?;
    }
    Ok(crate::hash_module(forms))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{hash_term, parse_term, print_term};
    use proptest::prelude::*;
    use proptest::test_runner::RngSeed;
    use std::collections::BTreeMap;

    #[test]
    fn admitted_symbol_names_roundtrip_in_atom_and_list_contexts() {
        for name in [
            ".",
            "a|b",
            "+1",
            "-name",
            "λ",
            "١٢",
            "a\u{a0}b",
            "a\0b",
            "b",
            ":key",
            "pkg/name::method",
        ] {
            let term = Term::try_symbol(name).unwrap();
            assert_eq!(
                parse_term(&print_term_checked(&term).unwrap()).unwrap(),
                term
            );
            let list = Term::list(vec![term.clone(), Term::Nil]);
            assert_eq!(
                parse_term(&print_term_checked(&list).unwrap()).unwrap(),
                list
            );
        }
        for name in [
            "", "nil", "true", "false", "0", "12name", "-1", "-1name", "a b", "a\tb", "a\nb",
            "a\rb", "(x)", "[x]", "{x}", "'x", "b\"x\"", "a;b",
        ] {
            assert!(Term::try_symbol(name).is_err(), "{name:?}");
            assert!(
                print_term_checked(&Term::Symbol(name.into())).is_err(),
                "{name:?}"
            );
        }
    }

    #[test]
    fn canonical_admission_rejects_aliases_at_every_container_edge() {
        let improper = Term::Pair(Box::new(Term::Int(1.into())), Box::new(Term::Int(2.into())));
        for invalid in [improper, Term::Symbol("true".into())] {
            let mut bad_key = BTreeMap::new();
            bad_key.insert(TermOrdKey(invalid.clone()), Term::Nil);
            let mut bad_value = BTreeMap::new();
            bad_value.insert(TermOrdKey(Term::symbol(":key")), invalid.clone());
            for term in [
                invalid.clone(),
                Term::list(vec![invalid.clone()]),
                Term::Vector(vec![invalid.clone()]),
                Term::Map(bad_key),
                Term::Map(bad_value),
            ] {
                assert!(validate_canonical_term(&term).is_err());
                assert!(print_term_checked(&term).is_err());
                assert!(print_term_compact_checked(&term).is_err());
                assert!(hash_term_checked(&term).is_err());
                assert!(print_module_checked(std::slice::from_ref(&term)).is_err());
                assert!(hash_module_checked(std::slice::from_ref(&term)).is_err());
            }
        }
    }

    #[test]
    fn admitted_v02_corpus_preserves_bytes_hashes_and_distinct_identity() {
        let sources = [
            "nil",
            "true",
            "false",
            "-123",
            "\"λ\"",
            "b\"\\x00\\xFF\"",
            "name",
            "(pair <improper>)",
            "(a . b)",
            "[nil true name]",
            "{name true :key [1 2]}",
        ];
        let mut hashes = std::collections::BTreeSet::new();
        for source in sources {
            let term = parse_term(source).unwrap();
            let bytes = print_term_checked(&term).unwrap();
            assert_eq!(bytes, print_term(&term));
            assert_eq!(parse_term(&bytes).unwrap(), term);
            let hash = hash_term_checked(&term).unwrap();
            assert_eq!(hash, hash_term(&term));
            assert!(hashes.insert(hash));
        }
    }

    proptest! {
        #![proptest_config(ProptestConfig { cases: 512, rng_seed: RngSeed::Fixed(0xf01), .. ProptestConfig::default() })]
        #[test]
        fn symbol_admission_matches_lexer_observation(name in any::<String>()) {
            let expected = crate::parse::lexer_symbol_observation(&name);
            prop_assert_eq!(validate_symbol_name(&name).is_ok(), expected);
        }
    }
}
