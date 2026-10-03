use crate::error::{KernelError, KernelErrorKind};
use crate::fallible_alloc::vec_with_capacity;

pub(super) fn validate_names(names: &[String]) -> Result<(), KernelError> {
    let mut sorted_names = vec_with_capacity(names.len(), "compiled module slot inventory")?;
    for name in names {
        gc_coreform::validate_symbol_name(name).map_err(|_| {
            KernelError::new(
                KernelErrorKind::Internal,
                "compiled module slot name is not canonical",
            )
        })?;
        sorted_names.push(name.as_str());
    }
    sorted_names.sort_unstable();
    if sorted_names.windows(2).any(|pair| pair[0] == pair[1]) {
        return Err(KernelError::new(
            KernelErrorKind::Internal,
            "compiled module slot names are duplicated",
        ));
    }
    Ok(())
}

pub(super) fn render_term(t: &gc_coreform::Term) -> Result<String, KernelError> {
    let rendered = gc_coreform::print_term_checked(t).map_err(|error| {
        let kind = if matches!(error, gc_coreform::CanonicalDomainError::WorkAllocation) {
            KernelErrorKind::MemoryLimit
        } else {
            KernelErrorKind::BadForm
        };
        KernelError::new(
            kind,
            format!("compiled module term is outside canonical serialization domain: {error}"),
        )
    })?;
    Ok(rendered)
}
