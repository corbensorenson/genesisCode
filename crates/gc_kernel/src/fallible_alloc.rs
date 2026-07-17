use crate::error::{KernelError, KernelErrorKind};

fn allocation_error(operation: &'static str, requested: usize) -> KernelError {
    KernelError::new(
        KernelErrorKind::MemoryLimit,
        format!("host allocation failed while reserving {requested} units for {operation}"),
    )
}

pub(crate) fn vec_with_capacity<T>(
    capacity: usize,
    operation: &'static str,
) -> Result<Vec<T>, KernelError> {
    let mut out = Vec::new();
    out.try_reserve_exact(capacity)
        .map_err(|_| allocation_error(operation, capacity))?;
    Ok(out)
}

pub(crate) fn string_with_capacity(
    capacity: usize,
    operation: &'static str,
) -> Result<String, KernelError> {
    let mut out = String::new();
    out.try_reserve_exact(capacity)
        .map_err(|_| allocation_error(operation, capacity))?;
    Ok(out)
}

pub(crate) fn clone_str(value: &str, operation: &'static str) -> Result<String, KernelError> {
    let mut out = string_with_capacity(value.len(), operation)?;
    out.push_str(value);
    Ok(out)
}

pub(crate) fn checked_add(
    left: usize,
    right: usize,
    operation: &'static str,
) -> Result<usize, KernelError> {
    left.checked_add(right)
        .ok_or_else(|| allocation_error(operation, usize::MAX))
}

pub(crate) fn checked_mul(
    left: usize,
    right: usize,
    operation: &'static str,
) -> Result<usize, KernelError> {
    left.checked_mul(right)
        .ok_or_else(|| allocation_error(operation, usize::MAX))
}
