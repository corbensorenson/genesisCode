use super::super::*;
use super::eval::eval_cexpr_runtime;

#[derive(Clone, Copy, Debug)]
enum PlanVar {
    State(usize),
    Temp(usize),
}

#[derive(Clone, Debug)]
enum PlanExpr {
    Var(PlanVar),
    Data(Value),
    Prim {
        op: PrimOp,
        args: Box<[PlanExpr]>,
    },
    Forward {
        op: PrimOp,
        supplied: Box<[PlanExpr]>,
        supplied_indices: Box<[usize]>,
    },
}

#[derive(Clone, Debug)]
enum PlanControl {
    Return(PlanExpr),
    Recur(Box<[PlanExpr]>),
    Let {
        bindings: Box<[(usize, PlanExpr)]>,
        body: Box<PlanControl>,
    },
    Begin {
        prefix: Box<[PlanExpr]>,
        body: Box<PlanControl>,
    },
}

#[derive(Clone, Debug)]
struct PlanBranch {
    control: PlanControl,
    steps: u64,
    state_uses: Box<[u32]>,
    temp_uses: Box<[u32]>,
}

#[derive(Clone, Debug)]
struct TailLoopPlan {
    arity: usize,
    temp_count: usize,
    condition: PlanExpr,
    condition_steps: u64,
    when_true: PlanBranch,
    when_false: PlanBranch,
}

#[derive(Debug)]
struct MutableUseCursor {
    state: Vec<u32>,
    temp: Vec<u32>,
}

enum ControlResult {
    Return(Value),
    Recur,
}

struct Lowerer<'a> {
    closure: &'a Rc<crate::value::CompiledClosureData>,
    arity: usize,
    next_temp: usize,
}

pub(super) fn eval_tail_loop_inline(
    ctx: &mut EvalCtx,
    caller_env: &RuntimeEnv,
    closure: Rc<crate::value::CompiledClosureData>,
    args: &[Arc<CExpr>],
) -> Result<Option<Value>, KernelError> {
    if ctx.step_limit.is_some()
        || ctx.mem_limits != crate::eval::MemLimits::default()
        || ctx.coverage_enabled()
        || caller_env.coverage_run.is_some()
    {
        return Ok(None);
    }
    let Some(plan) = TailLoopPlan::lower(&closure) else {
        return Ok(None);
    };
    if args.len() != plan.arity {
        return Ok(None);
    }

    let mut state = Vec::with_capacity(plan.arity);
    for (index, arg) in args.iter().enumerate() {
        state.push(Some(eval_cexpr_runtime(ctx, caller_env.clone(), arg)?));
        if index + 1 < plan.arity {
            ctx.tick()?;
        }
    }

    #[cfg(test)]
    TAIL_LOOP_EXECUTIONS.with(|count| count.set(count.get().saturating_add(1)));

    execute_plan(ctx, &plan, state).map(Some)
}

fn execute_plan(
    ctx: &mut EvalCtx,
    plan: &TailLoopPlan,
    mut state: Vec<Option<Value>>,
) -> Result<Value, KernelError> {
    let mut temps = vec![None; plan.temp_count];
    let mut next = Vec::with_capacity(plan.arity);
    let mut uses = MutableUseCursor {
        state: vec![0; plan.arity],
        temp: vec![0; plan.temp_count],
    };

    loop {
        charge_steps(ctx, 1u64.saturating_add(plan.condition_steps));
        let condition = eval_expr_borrowed(ctx, &plan.condition, &state, &temps)?;
        let branch = if condition.truthy() {
            &plan.when_true
        } else {
            &plan.when_false
        };
        charge_steps(ctx, branch.steps);
        uses.state.copy_from_slice(&branch.state_uses);
        uses.temp.copy_from_slice(&branch.temp_uses);
        for temp in &mut temps {
            *temp = None;
        }
        next.clear();
        match eval_control(
            ctx,
            &branch.control,
            &mut state,
            &mut temps,
            &mut uses,
            &mut next,
        )? {
            ControlResult::Return(value) => return Ok(value),
            ControlResult::Recur => {
                if next.len() != plan.arity {
                    return Err(KernelError::new(
                        KernelErrorKind::Internal,
                        "compiled tail-loop state arity drifted",
                    ));
                }
                std::mem::swap(&mut state, &mut next);
            }
        }
    }
}

fn eval_control(
    ctx: &mut EvalCtx,
    control: &PlanControl,
    state: &mut [Option<Value>],
    temps: &mut [Option<Value>],
    uses: &mut MutableUseCursor,
    next: &mut Vec<Option<Value>>,
) -> Result<ControlResult, KernelError> {
    match control {
        PlanControl::Return(expr) => Ok(ControlResult::Return(eval_expr_owned(
            ctx, expr, state, temps, uses,
        )?)),
        PlanControl::Recur(exprs) => {
            for expr in exprs {
                next.push(Some(eval_expr_owned(ctx, expr, state, temps, uses)?));
            }
            Ok(ControlResult::Recur)
        }
        PlanControl::Let { bindings, body } => {
            for (slot, expr) in bindings {
                let value = eval_expr_owned(ctx, expr, state, temps, uses)?;
                let Some(target) = temps.get_mut(*slot) else {
                    return Err(KernelError::new(
                        KernelErrorKind::Internal,
                        "compiled tail-loop temporary is out of range",
                    ));
                };
                *target = Some(value);
            }
            eval_control(ctx, body, state, temps, uses, next)
        }
        PlanControl::Begin { prefix, body } => {
            for expr in prefix {
                let _ = eval_expr_owned(ctx, expr, state, temps, uses)?;
            }
            eval_control(ctx, body, state, temps, uses, next)
        }
    }
}

fn eval_expr_borrowed(
    ctx: &mut EvalCtx,
    expr: &PlanExpr,
    state: &[Option<Value>],
    temps: &[Option<Value>],
) -> Result<Value, KernelError> {
    match expr {
        PlanExpr::Var(var) => read_var(*var, state, temps),
        PlanExpr::Data(value) => {
            if let Some(term) = value.as_data() {
                ctx.mem_observe_data_term(term)?;
            }
            Ok(value.clone())
        }
        PlanExpr::Prim { op, args } => eval_prim_borrowed(ctx, *op, args, state, temps),
        PlanExpr::Forward {
            op,
            supplied,
            supplied_indices,
        } => {
            let mut values = Vec::with_capacity(supplied.len());
            for arg in supplied {
                values.push(eval_expr_borrowed(ctx, arg, state, temps)?);
            }
            apply_forwarded(ctx, *op, &values, supplied_indices)
        }
    }
}

fn eval_expr_owned(
    ctx: &mut EvalCtx,
    expr: &PlanExpr,
    state: &mut [Option<Value>],
    temps: &mut [Option<Value>],
    uses: &mut MutableUseCursor,
) -> Result<Value, KernelError> {
    match expr {
        PlanExpr::Var(var) => take_or_clone_var(*var, state, temps, uses),
        PlanExpr::Data(value) => {
            if let Some(term) = value.as_data() {
                ctx.mem_observe_data_term(term)?;
            }
            Ok(value.clone())
        }
        PlanExpr::Prim { op, args } => eval_prim_owned(ctx, *op, args, state, temps, uses),
        PlanExpr::Forward {
            op,
            supplied,
            supplied_indices,
        } => {
            let mut values = Vec::with_capacity(supplied.len());
            for arg in supplied {
                values.push(eval_expr_owned(ctx, arg, state, temps, uses)?);
            }
            apply_forwarded(ctx, *op, &values, supplied_indices)
        }
    }
}

fn eval_prim_borrowed(
    ctx: &mut EvalCtx,
    op: PrimOp,
    args: &[PlanExpr],
    state: &[Option<Value>],
    temps: &[Option<Value>],
) -> Result<Value, KernelError> {
    if let [left, right] = args {
        let left = eval_expr_borrowed(ctx, left, state, temps)?;
        let right = eval_expr_borrowed(ctx, right, state, temps)?;
        return prim_op2(ctx, op, left, right);
    }
    let mut values = Vec::with_capacity(args.len());
    for arg in args {
        values.push(eval_expr_borrowed(ctx, arg, state, temps)?);
    }
    prim_op(ctx, op, values)
}

fn eval_prim_owned(
    ctx: &mut EvalCtx,
    op: PrimOp,
    args: &[PlanExpr],
    state: &mut [Option<Value>],
    temps: &mut [Option<Value>],
    uses: &mut MutableUseCursor,
) -> Result<Value, KernelError> {
    if let [left, right] = args {
        let left = eval_expr_owned(ctx, left, state, temps, uses)?;
        let right = eval_expr_owned(ctx, right, state, temps, uses)?;
        return prim_op2(ctx, op, left, right);
    }
    let mut values = Vec::with_capacity(args.len());
    for arg in args {
        values.push(eval_expr_owned(ctx, arg, state, temps, uses)?);
    }
    prim_op(ctx, op, values)
}

fn apply_forwarded(
    ctx: &mut EvalCtx,
    op: PrimOp,
    supplied: &[Value],
    indices: &[usize],
) -> Result<Value, KernelError> {
    if let [left, right] = indices {
        return prim_op2(ctx, op, supplied[*left].clone(), supplied[*right].clone());
    }
    prim_op(
        ctx,
        op,
        indices
            .iter()
            .map(|index| supplied[*index].clone())
            .collect(),
    )
}

fn read_var(
    var: PlanVar,
    state: &[Option<Value>],
    temps: &[Option<Value>],
) -> Result<Value, KernelError> {
    let value = match var {
        PlanVar::State(index) => state.get(index),
        PlanVar::Temp(index) => temps.get(index),
    }
    .and_then(Option::as_ref)
    .ok_or_else(|| {
        KernelError::new(
            KernelErrorKind::Internal,
            "compiled tail-loop variable is unavailable",
        )
    })?;
    Ok(value.clone())
}

fn take_or_clone_var(
    var: PlanVar,
    state: &mut [Option<Value>],
    temps: &mut [Option<Value>],
    uses: &mut MutableUseCursor,
) -> Result<Value, KernelError> {
    let (value, remaining) = match var {
        PlanVar::State(index) => (state.get_mut(index), uses.state.get_mut(index)),
        PlanVar::Temp(index) => (temps.get_mut(index), uses.temp.get_mut(index)),
    };
    let value = value.ok_or_else(|| {
        KernelError::new(
            KernelErrorKind::Internal,
            "compiled tail-loop variable slot is out of range",
        )
    })?;
    let remaining = remaining.ok_or_else(|| {
        KernelError::new(
            KernelErrorKind::Internal,
            "compiled tail-loop use counter is out of range",
        )
    })?;
    if *remaining == 0 {
        return Err(KernelError::new(
            KernelErrorKind::Internal,
            "compiled tail-loop use counter underflowed",
        ));
    }
    *remaining -= 1;
    if *remaining == 0 {
        value.take().ok_or_else(|| {
            KernelError::new(
                KernelErrorKind::Internal,
                "compiled tail-loop variable was consumed twice",
            )
        })
    } else {
        value.clone().ok_or_else(|| {
            KernelError::new(
                KernelErrorKind::Internal,
                "compiled tail-loop variable is unavailable",
            )
        })
    }
}

fn charge_steps(ctx: &mut EvalCtx, steps: u64) {
    debug_assert!(ctx.step_limit.is_none());
    ctx.steps = ctx.steps.saturating_add(steps);
}

impl TailLoopPlan {
    fn lower(closure: &Rc<crate::value::CompiledClosureData>) -> Option<Self> {
        let mut arity = 1usize;
        let mut body = closure.body_c.inner();
        while let CExpr::FnUnary {
            body: next_body, ..
        } = body.as_ref()
        {
            arity = arity.checked_add(1)?;
            body = next_body;
        }
        let CExpr::If {
            cond,
            then_expr,
            else_expr,
            ..
        } = body.as_ref()
        else {
            return None;
        };
        let vars = (0..arity).rev().map(PlanVar::State).collect::<Vec<_>>();
        let mut lowerer = Lowerer {
            closure,
            arity,
            next_temp: 0,
        };
        let (condition, condition_steps) = lowerer.lower_expr(cond, &vars)?;
        let (when_true, true_steps) = lowerer.lower_control(then_expr, &vars)?;
        let (when_false, false_steps) = lowerer.lower_control(else_expr, &vars)?;
        if !when_true.has_recur() && !when_false.has_recur() {
            return None;
        }
        let temp_count = lowerer.next_temp;
        Some(Self {
            arity,
            temp_count,
            condition,
            condition_steps,
            when_true: PlanBranch::new(when_true, true_steps, arity, temp_count),
            when_false: PlanBranch::new(when_false, false_steps, arity, temp_count),
        })
    }
}

impl PlanBranch {
    fn new(control: PlanControl, steps: u64, arity: usize, temp_count: usize) -> Self {
        let mut state_uses = vec![0; arity];
        let mut temp_uses = vec![0; temp_count];
        control.count_uses(&mut state_uses, &mut temp_uses);
        Self {
            control,
            steps,
            state_uses: state_uses.into_boxed_slice(),
            temp_uses: temp_uses.into_boxed_slice(),
        }
    }
}

impl PlanControl {
    fn has_recur(&self) -> bool {
        match self {
            Self::Recur(_) => true,
            Self::Let { body, .. } | Self::Begin { body, .. } => body.has_recur(),
            Self::Return(_) => false,
        }
    }

    fn count_uses(&self, state: &mut [u32], temp: &mut [u32]) {
        match self {
            Self::Return(expr) => expr.count_uses(state, temp),
            Self::Recur(exprs) => {
                for expr in exprs {
                    expr.count_uses(state, temp);
                }
            }
            Self::Let { bindings, body } => {
                for (_, expr) in bindings {
                    expr.count_uses(state, temp);
                }
                body.count_uses(state, temp);
            }
            Self::Begin { prefix, body } => {
                for expr in prefix {
                    expr.count_uses(state, temp);
                }
                body.count_uses(state, temp);
            }
        }
    }
}

impl PlanExpr {
    fn count_uses(&self, state: &mut [u32], temp: &mut [u32]) {
        match self {
            Self::Var(PlanVar::State(index)) => state[*index] = state[*index].saturating_add(1),
            Self::Var(PlanVar::Temp(index)) => temp[*index] = temp[*index].saturating_add(1),
            Self::Prim { args, .. } => {
                for arg in args {
                    arg.count_uses(state, temp);
                }
            }
            Self::Forward { supplied, .. } => {
                for arg in supplied {
                    arg.count_uses(state, temp);
                }
            }
            Self::Data(_) => {}
        }
    }
}

impl Lowerer<'_> {
    fn lower_control(&mut self, expr: &Arc<CExpr>, vars: &[PlanVar]) -> Option<(PlanControl, u64)> {
        if let Some((args, call_steps)) = self.recursive_call(expr, vars) {
            return Some((PlanControl::Recur(args.into_boxed_slice()), call_steps));
        }
        match expr.as_ref() {
            CExpr::Let(bindings, body) => {
                let mut scoped = vars.to_vec();
                let mut lowered = Vec::with_capacity(bindings.len());
                let mut steps = 1u64;
                for (_, rhs) in bindings {
                    let (rhs, rhs_steps) = self.lower_expr(rhs, &scoped)?;
                    steps = steps.saturating_add(rhs_steps);
                    let slot = self.next_temp;
                    self.next_temp = self.next_temp.checked_add(1)?;
                    lowered.push((slot, rhs));
                    scoped.insert(0, PlanVar::Temp(slot));
                }
                let (body, body_steps) = self.lower_control(body, &scoped)?;
                steps = steps.saturating_add(body_steps);
                Some((
                    PlanControl::Let {
                        bindings: lowered.into_boxed_slice(),
                        body: Box::new(body),
                    },
                    steps,
                ))
            }
            CExpr::Begin(items) if !items.is_empty() => {
                let mut prefix = Vec::with_capacity(items.len().saturating_sub(1));
                let mut steps = 1u64;
                for item in items.iter().take(items.len() - 1) {
                    let (item, item_steps) = self.lower_expr(item, vars)?;
                    steps = steps.saturating_add(item_steps);
                    prefix.push(item);
                }
                let (body, body_steps) = self.lower_control(&items[items.len() - 1], vars)?;
                steps = steps.saturating_add(body_steps);
                Some((
                    PlanControl::Begin {
                        prefix: prefix.into_boxed_slice(),
                        body: Box::new(body),
                    },
                    steps,
                ))
            }
            _ => {
                let (expr, steps) = self.lower_expr(expr, vars)?;
                Some((PlanControl::Return(expr), steps))
            }
        }
    }

    fn lower_expr(&self, expr: &Arc<CExpr>, vars: &[PlanVar]) -> Option<(PlanExpr, u64)> {
        match expr.as_ref() {
            CExpr::Var {
                resolution: VarResolution::Local { depth, slot: 0 },
                ..
            } => Some((PlanExpr::Var(*vars.get(usize::from(*depth))?), 1)),
            CExpr::Atom(term) | CExpr::Quote(term) => {
                Some((PlanExpr::Data(Value::data(term.clone())), 1))
            }
            CExpr::Prim { op, args } => {
                let (args, child_steps) = self.lower_args(args, vars)?;
                Some((
                    PlanExpr::Prim {
                        op: *op,
                        args: args.into_boxed_slice(),
                    },
                    1u64.saturating_add(child_steps),
                ))
            }
            CExpr::App(callee, arg) => {
                self.lower_forwarded_call(callee, std::slice::from_ref(arg), 0, vars)
            }
            CExpr::AppN {
                callee,
                args,
                extra_app_ticks,
            } => self.lower_forwarded_call(callee, args, *extra_app_ticks, vars),
            _ => None,
        }
    }

    fn lower_args(&self, args: &[Arc<CExpr>], vars: &[PlanVar]) -> Option<(Vec<PlanExpr>, u64)> {
        let mut lowered = Vec::with_capacity(args.len());
        let mut steps = 0u64;
        for arg in args {
            let (arg, arg_steps) = self.lower_expr(arg, vars)?;
            lowered.push(arg);
            steps = steps.saturating_add(arg_steps);
        }
        Some((lowered, steps))
    }

    fn lower_forwarded_call(
        &self,
        callee: &Arc<CExpr>,
        args: &[Arc<CExpr>],
        extra_app_ticks: u32,
        vars: &[PlanVar],
    ) -> Option<(PlanExpr, u64)> {
        let CExpr::Var {
            name,
            resolution: VarResolution::External,
            ..
        } = callee.as_ref()
        else {
            return None;
        };
        let Value::CompiledClosure(target) = self.closure.env.get(name)? else {
            return None;
        };
        let forward = target.primitive_forward_plan.as_ref()?;
        if args.len() != forward.arity() {
            return None;
        }
        let (supplied, child_steps) = self.lower_args(args, vars)?;
        let supplied_indices = forward.supplied_indices().collect::<Vec<_>>();
        let direct = supplied_indices.len() == supplied.len()
            && supplied_indices.iter().copied().eq(0..supplied.len());
        let expr = if direct {
            PlanExpr::Prim {
                op: forward.op(),
                args: supplied.into_boxed_slice(),
            }
        } else {
            PlanExpr::Forward {
                op: forward.op(),
                supplied: supplied.into_boxed_slice(),
                supplied_indices: supplied_indices.into_boxed_slice(),
            }
        };
        let steps = 1u64
            .saturating_add(u64::from(extra_app_ticks))
            .saturating_add(1)
            .saturating_add(child_steps)
            .saturating_add(u64::try_from(forward.arity().saturating_sub(1)).ok()?)
            .saturating_add(1)
            .saturating_add(u64::try_from(forward.supplied_indices().len()).ok()?);
        Some((expr, steps))
    }

    fn recursive_call(&self, expr: &Arc<CExpr>, vars: &[PlanVar]) -> Option<(Vec<PlanExpr>, u64)> {
        let (callee, args, extra_app_ticks) = match expr.as_ref() {
            CExpr::App(callee, arg) => (callee, std::slice::from_ref(arg), 0),
            CExpr::AppN {
                callee,
                args,
                extra_app_ticks,
            } => (callee, args.as_ref(), *extra_app_ticks),
            _ => return None,
        };
        let CExpr::Var {
            resolution: VarResolution::Module { slot },
            ..
        } = callee.as_ref()
        else {
            return None;
        };
        let module = self.closure.module_env.as_ref()?;
        let Value::CompiledClosure(target) = module.get(*slot)? else {
            return None;
        };
        if !Rc::ptr_eq(&target, self.closure) || args.len() != self.arity {
            return None;
        }
        let (args, child_steps) = self.lower_args(args, vars)?;
        let steps = 1u64
            .saturating_add(u64::from(extra_app_ticks))
            .saturating_add(1)
            .saturating_add(child_steps)
            .saturating_add(u64::try_from(self.arity.saturating_sub(1)).ok()?);
        Some((args, steps))
    }
}

#[cfg(test)]
thread_local! {
    static TAIL_LOOP_EXECUTIONS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

#[cfg(test)]
pub(crate) fn reset_tail_loop_executions() {
    TAIL_LOOP_EXECUTIONS.with(|count| count.set(0));
}

#[cfg(test)]
pub(crate) fn tail_loop_executions() -> usize {
    TAIL_LOOP_EXECUTIONS.with(std::cell::Cell::get)
}
