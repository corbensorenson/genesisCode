use super::semantic_workspace_types::PathStep;
use super::*;

pub(super) fn map_patch_error(err: gc_patches::PatchError) -> CliError {
    match err {
        gc_patches::PatchError::Parse(_) | gc_patches::PatchError::Validate(_) => {
            cli_err(EX_PARSE, "semantic-edit/invalid", format!("{err}"))
        }
        gc_patches::PatchError::Io(_) => cli_err(EX_IO, "io/error", format!("{err}")),
        gc_patches::PatchError::Obligations(inner) => obligation_err(inner),
    }
}

pub(super) fn path_to_term(path: &[PathStep]) -> Result<Term, CliError> {
    let mut steps = Vec::with_capacity(path.len());
    for step in path {
        let term = match step {
            PathStep::Form(i) => Term::Vector(vec![
                Term::symbol(":form"),
                Term::Int(
                    i64::try_from(*i)
                        .map_err(|_| {
                            cli_err(
                                EX_PARSE,
                                "semantic-edit/path",
                                "path index out of range".to_string(),
                            )
                        })?
                        .into(),
                ),
            ]),
            PathStep::PairCar => Term::Vector(vec![Term::symbol(":pair-car")]),
            PathStep::PairCdr => Term::Vector(vec![Term::symbol(":pair-cdr")]),
        };
        steps.push(term);
    }
    Ok(Term::Vector(steps))
}
