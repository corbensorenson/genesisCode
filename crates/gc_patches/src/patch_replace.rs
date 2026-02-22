use gc_coreform::{Term, TermOrdKey, print_term};

use crate::{PatchError, PathStep};

pub(super) fn apply_replace(
    forms: &mut [Term],
    path: &[PathStep],
    new_term: Term,
) -> Result<(), PatchError> {
    if path.is_empty() {
        return Err(PatchError::Validate("empty path".to_string()));
    }
    let mut cur: ReplaceTarget = ReplaceTarget::Module(forms);
    for (i, step) in path.iter().enumerate() {
        let is_last = i + 1 == path.len();
        cur = cur.step(step, is_last, new_term.clone())?;
    }
    Ok(())
}

enum ReplaceTarget<'a> {
    Module(&'a mut [Term]),
    Term(&'a mut Term),
}

impl<'a> ReplaceTarget<'a> {
    fn step(
        self,
        s: &PathStep,
        is_last: bool,
        new_term: Term,
    ) -> Result<ReplaceTarget<'a>, PatchError> {
        match self {
            ReplaceTarget::Module(forms) => match s {
                PathStep::Form(idx) => {
                    let t = forms.get_mut(*idx).ok_or_else(|| {
                        PatchError::Validate(format!("form index out of range: {idx}"))
                    })?;
                    if is_last {
                        *t = new_term;
                        Ok(ReplaceTarget::Term(t))
                    } else {
                        Ok(ReplaceTarget::Term(t))
                    }
                }
                _ => Err(PatchError::Validate(
                    "path must start with [:form i]".to_string(),
                )),
            },
            ReplaceTarget::Term(t) => match s {
                PathStep::PairCar => {
                    let Term::Pair(a, _) = t else {
                        return Err(PatchError::Validate("expected pair".to_string()));
                    };
                    if is_last {
                        **a = new_term;
                    }
                    Ok(ReplaceTarget::Term(a))
                }
                PathStep::PairCdr => {
                    let Term::Pair(_, d) = t else {
                        return Err(PatchError::Validate("expected pair".to_string()));
                    };
                    if is_last {
                        **d = new_term;
                    }
                    Ok(ReplaceTarget::Term(d))
                }
                PathStep::Vec(idx) => {
                    let Term::Vector(xs) = t else {
                        return Err(PatchError::Validate("expected vector".to_string()));
                    };
                    let elt = xs.get_mut(*idx).ok_or_else(|| {
                        PatchError::Validate(format!("vector index out of range: {idx}"))
                    })?;
                    if is_last {
                        *elt = new_term;
                    }
                    Ok(ReplaceTarget::Term(elt))
                }
                PathStep::Map(key) => {
                    let Term::Map(m) = t else {
                        return Err(PatchError::Validate("expected map".to_string()));
                    };
                    let elt = m.get_mut(&TermOrdKey(key.clone())).ok_or_else(|| {
                        PatchError::Validate(format!("missing map key {}", print_term(key)))
                    })?;
                    if is_last {
                        *elt = new_term;
                    }
                    Ok(ReplaceTarget::Term(elt))
                }
                PathStep::Form(_) => Err(PatchError::Validate("unexpected :form step".to_string())),
            },
        }
    }
}
