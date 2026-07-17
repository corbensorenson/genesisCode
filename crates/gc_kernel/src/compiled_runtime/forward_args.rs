use super::super::*;

pub(super) fn apply_forwarded_owned(
    ctx: &mut EvalCtx,
    op: PrimOp,
    supplied: Vec<Value>,
    indices: &[usize],
) -> Result<Value, KernelError> {
    let mut seen = vec![false; supplied.len()];
    for index in indices {
        let Some(slot) = seen.get_mut(*index) else {
            return forward_error("index is out of range");
        };
        if *slot {
            return apply_forwarded_cloned(ctx, op, &supplied, indices);
        }
        *slot = true;
    }

    let mut slots = supplied.into_iter().map(Some).collect::<Vec<_>>();
    let mut ordered = Vec::with_capacity(indices.len());
    for index in indices {
        let Some(value) = slots.get_mut(*index).and_then(Option::take) else {
            return forward_error("value is unavailable");
        };
        ordered.push(value);
    }
    apply_ordered(ctx, op, ordered)
}

fn apply_forwarded_cloned(
    ctx: &mut EvalCtx,
    op: PrimOp,
    supplied: &[Value],
    indices: &[usize],
) -> Result<Value, KernelError> {
    let mut ordered = Vec::with_capacity(indices.len());
    for index in indices {
        let Some(value) = supplied.get(*index) else {
            return forward_error("index is out of range");
        };
        ordered.push(value.clone());
    }
    apply_ordered(ctx, op, ordered)
}

fn apply_ordered(ctx: &mut EvalCtx, op: PrimOp, ordered: Vec<Value>) -> Result<Value, KernelError> {
    if ordered.len() == 2 {
        let mut values = ordered.into_iter();
        let Some(left) = values.next() else {
            return forward_error("left argument is unavailable");
        };
        let Some(right) = values.next() else {
            return forward_error("right argument is unavailable");
        };
        return prim_op2(ctx, op, left, right);
    }
    prim_op(ctx, op, ordered)
}

fn forward_error<T>(detail: &str) -> Result<T, KernelError> {
    Err(KernelError::new(
        KernelErrorKind::Internal,
        format!("compiled primitive-forward {detail}"),
    ))
}
