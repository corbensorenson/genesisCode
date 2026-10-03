use super::{invalid, push};
use crate::error::KernelError;
use gc_coreform::Term;

pub(super) fn list(term: &Term) -> Result<Vec<&Term>, KernelError> {
    let mut items = Vec::new();
    let mut current = term;
    loop {
        match current {
            Term::Nil => return Ok(items),
            Term::Pair(car, cdr) => {
                push(&mut items, car.as_ref())?;
                current = cdr;
            }
            _ => return Err(invalid()),
        }
    }
}

pub(super) fn equal(left: &Term, right: &Term) -> Result<bool, KernelError> {
    let mut work = Vec::new();
    push(&mut work, (left, right))?;
    while let Some((left, right)) = work.pop() {
        match (left, right) {
            (Term::Nil, Term::Nil) => {}
            (Term::Bool(a), Term::Bool(b)) if a == b => {}
            (Term::Int(a), Term::Int(b)) if a == b => {}
            (Term::Str(a), Term::Str(b)) | (Term::Symbol(a), Term::Symbol(b)) if a == b => {}
            (Term::Bytes(a), Term::Bytes(b)) if a == b => {}
            (Term::Pair(ac, ad), Term::Pair(bc, bd)) => {
                push(&mut work, (ad.as_ref(), bd.as_ref()))?;
                push(&mut work, (ac.as_ref(), bc.as_ref()))?;
            }
            (Term::Vector(a), Term::Vector(b)) if a.len() == b.len() => {
                for pair in a.iter().zip(b).rev() {
                    push(&mut work, pair)?;
                }
            }
            (Term::Map(a), Term::Map(b)) if a.len() == b.len() => {
                for ((ak, av), (bk, bv)) in a.iter().zip(b).rev() {
                    push(&mut work, (av, bv))?;
                    push(&mut work, (&ak.0, &bk.0))?;
                }
            }
            _ => return Ok(false),
        }
    }
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn semantic_term_equality_is_iterative_exact_and_differential() {
        let sources = [
            "nil",
            "true",
            "false",
            "0",
            "-1",
            "4294967296",
            "\"x\"",
            "b\"00ff\"",
            "symbol",
            "(1 2)",
            "[1 2]",
            "{:a 1 :b 2}",
            "{:a [1 (2)] :b {:c 3}}",
        ];
        let terms = sources
            .iter()
            .map(|text| gc_coreform::parse_term(text).unwrap())
            .collect::<Vec<_>>();
        for left in &terms {
            for right in &terms {
                assert_eq!(equal(left, right).unwrap(), left == right);
            }
        }
        std::thread::Builder::new()
            .stack_size(96 * 1024)
            .spawn(|| {
                let mut left = Term::Int(1.into());
                let mut right = Term::Int(1.into());
                for _ in 0..20000 {
                    left = Term::Pair(Box::new(Term::Nil), Box::new(left));
                    right = Term::Pair(Box::new(Term::Nil), Box::new(right));
                }
                assert!(equal(&left, &right).unwrap());
                assert!(!equal(&left, &Term::Int(1.into())).unwrap());
                // Retire this observer's deliberately deep fixture without testing
                // the inherited recursive Term destructor as part of this checker.
                std::mem::forget(left);
                std::mem::forget(right);
            })
            .unwrap()
            .join()
            .unwrap();
    }
}
