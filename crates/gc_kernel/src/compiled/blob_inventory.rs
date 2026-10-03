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
