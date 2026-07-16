use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};
use std::rc::Rc;
use std::sync::{Arc, OnceLock};

use num_traits::ToPrimitive;

use crate::env::Env;
use crate::error::{KernelError, KernelErrorKind};
use crate::eval::{CoverageRunId, EvalCtx, PrimOp, prim, prim_op, prim_op2, type_err};
use crate::value::{NativeFn, Value};
use gc_coreform::{Term, TermOrdKey};

#[path = "compiled_blob.rs"]
mod compiled_blob;
#[path = "compiled_compile.rs"]
mod compiled_compile;
#[path = "compiled_coverage.rs"]
mod compiled_coverage;
#[path = "compiled_runtime/mod.rs"]
mod compiled_runtime;

const COMPILED_MODULE_BLOB_MAGIC: &[u8] = b"GCKM5\0";

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub(crate) struct Sym(u32);

#[derive(Clone, Debug, Default)]
pub(crate) struct SymbolInterner {
    ids: BTreeMap<String, Sym>,
    names: Vec<String>,
}

impl SymbolInterner {
    pub(crate) fn intern(&mut self, name: &str) -> Result<Sym, KernelError> {
        if let Some(sym) = self.ids.get(name) {
            return Ok(*sym);
        }
        let id = u32::try_from(self.names.len()).map_err(|_| {
            KernelError::new(
                KernelErrorKind::Internal,
                "compiled symbol table exceeds u32 range",
            )
        })?;
        let sym = Sym(id);
        self.names.push(name.to_string());
        self.ids.insert(name.to_string(), sym);
        Ok(sym)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum VarResolution {
    Local { depth: u16, slot: u16 },
    Module { slot: u32 },
    External,
}

#[derive(Clone, Debug)]
pub(crate) struct CompiledExprBundle {
    expr: Arc<CExpr>,
    coverage_sites: Arc<CompiledCoverageSites>,
}

#[derive(Debug, Default, Eq, PartialEq)]
pub(crate) struct CompiledCoverageSites {
    statement_sites: Box<[String]>,
    decision_sites: Box<[String]>,
    decision_conditions: Box<[BTreeSet<String>]>,
}

impl CompiledCoverageSites {
    fn from_parts(
        statement_sites: Vec<String>,
        decision_sites: Vec<String>,
        decision_conditions: Vec<BTreeSet<String>>,
    ) -> Result<Arc<Self>, KernelError> {
        if decision_sites.len() != decision_conditions.len() {
            return Err(KernelError::new(
                KernelErrorKind::Internal,
                "compiled coverage decision table length mismatch",
            ));
        }
        Ok(Arc::new(Self {
            statement_sites: statement_sites.into_boxed_slice(),
            decision_sites: decision_sites.into_boxed_slice(),
            decision_conditions: decision_conditions.into_boxed_slice(),
        }))
    }

    fn manifest(&self) -> CoverageSiteManifest {
        let mut manifest = CoverageSiteManifest::default();
        manifest
            .statement_sites
            .extend(self.statement_sites.iter().cloned());
        manifest
            .decision_sites
            .extend(self.decision_sites.iter().cloned());
        for (site_id, conditions) in self
            .decision_sites
            .iter()
            .zip(self.decision_conditions.iter())
        {
            manifest
                .decision_conditions
                .insert(site_id.clone(), conditions.clone());
        }
        manifest
    }

    fn statement_sites(&self) -> &[String] {
        &self.statement_sites
    }

    fn decision_sites(&self) -> &[String] {
        &self.decision_sites
    }

    fn same_table(a: &Arc<Self>, b: &Arc<Self>) -> bool {
        Arc::ptr_eq(a, b) || a.as_ref() == b.as_ref()
    }
}

#[derive(Clone, Debug)]
pub struct CompiledLexicalEnv(Rc<CompiledLexicalFrame>);

#[derive(Debug)]
struct CompiledLexicalFrame {
    parent: Option<CompiledLexicalEnv>,
    slots: CompiledLexicalSlots,
}

#[derive(Debug)]
enum CompiledLexicalSlots {
    Empty,
    One(Value),
    Many(Box<[Value]>),
    Sparse {
        span: usize,
        values: BTreeMap<usize, Value>,
    },
}

impl CompiledLexicalEnv {
    fn empty() -> Self {
        Self(Rc::new(CompiledLexicalFrame {
            parent: None,
            slots: CompiledLexicalSlots::Empty,
        }))
    }

    fn with_slot(parent: &Self, value: Value) -> Self {
        Self(Rc::new(CompiledLexicalFrame {
            parent: Some(parent.clone()),
            slots: CompiledLexicalSlots::One(value),
        }))
    }

    fn with_slots(parent: &Self, mut values_oldest_to_newest: Vec<Value>) -> Self {
        if values_oldest_to_newest.len() == 1 {
            let value = values_oldest_to_newest.remove(0);
            return Self::with_slot(parent, value);
        }
        values_oldest_to_newest.reverse();
        Self(Rc::new(CompiledLexicalFrame {
            parent: Some(parent.clone()),
            slots: CompiledLexicalSlots::Many(values_oldest_to_newest.into_boxed_slice()),
        }))
    }

    fn get(&self, depth: u16, slot: u16) -> Option<Value> {
        let mut cur = self.clone();
        let mut depth = usize::from(depth);
        loop {
            match &cur.0.slots {
                CompiledLexicalSlots::Empty => {}
                CompiledLexicalSlots::One(value) => {
                    if slot == 0 && depth == 0 {
                        return Some(value.clone());
                    }
                    depth = depth.checked_sub(1)?;
                }
                CompiledLexicalSlots::Many(values) => {
                    if slot == 0 && depth < values.len() {
                        return Some(values[depth].clone());
                    }
                    depth = depth.checked_sub(values.len())?;
                }
                CompiledLexicalSlots::Sparse { span, values } => {
                    if slot == 0
                        && let Some(value) = values.get(&depth)
                    {
                        return Some(value.clone());
                    }
                    depth = depth.checked_sub(*span)?;
                }
            }
            cur = cur.0.parent.as_ref()?.clone();
        }
    }

    fn capture(&self, depths: &BTreeSet<usize>) -> Result<Self, KernelError> {
        let Some(max_depth) = depths.last().copied() else {
            return Ok(Self::empty());
        };
        let mut values = BTreeMap::new();
        for depth in depths {
            let depth_u16 = u16::try_from(*depth).map_err(|_| {
                KernelError::new(
                    KernelErrorKind::Internal,
                    "compiled closure capture depth exceeds u16 range",
                )
            })?;
            let value = self.get(depth_u16, 0).ok_or_else(|| {
                KernelError::new(
                    KernelErrorKind::Internal,
                    format!("compiled closure capture slot is missing at depth {depth}"),
                )
            })?;
            values.insert(*depth, value);
        }
        Ok(Self(Rc::new(CompiledLexicalFrame {
            parent: None,
            slots: CompiledLexicalSlots::Sparse {
                span: max_depth.saturating_add(1),
                values,
            },
        })))
    }

    #[cfg(test)]
    pub(crate) fn captured_value_count(&self) -> usize {
        match &self.0.slots {
            CompiledLexicalSlots::Empty => 0,
            CompiledLexicalSlots::One(_) => 1,
            CompiledLexicalSlots::Many(values) => values.len(),
            CompiledLexicalSlots::Sparse { values, .. } => values.len(),
        }
    }
}

#[derive(Clone, Debug)]
pub struct CompiledModuleCells(Rc<RefCell<Vec<Option<Value>>>>);

impl CompiledModuleCells {
    fn new(len: usize) -> Self {
        Self(Rc::new(RefCell::new(vec![None; len])))
    }

    fn empty() -> Self {
        Self::new(0)
    }

    fn get(&self, slot: u32) -> Option<Value> {
        let slot = usize::try_from(slot).ok()?;
        self.0.borrow().get(slot).cloned().flatten()
    }

    fn set(&self, slot: u32, value: Value) -> Result<(), KernelError> {
        let slot = usize::try_from(slot).map_err(|_| {
            KernelError::new(KernelErrorKind::Internal, "module slot exceeds usize range")
        })?;
        let mut cells = self.0.borrow_mut();
        let Some(cell) = cells.get_mut(slot) else {
            return Err(KernelError::new(
                KernelErrorKind::Internal,
                format!("module slot out of range: {slot}"),
            ));
        };
        *cell = Some(value);
        Ok(())
    }
}

#[derive(Clone, Debug)]
struct RuntimeEnv {
    lexical: CompiledLexicalEnv,
    inline_slot: Option<Value>,
    module: CompiledModuleCells,
    external: Env,
    coverage_sites: Arc<CompiledCoverageSites>,
    coverage_run: Option<CoverageRunId>,
}

impl RuntimeEnv {
    fn new(
        external: Env,
        module: CompiledModuleCells,
        coverage_sites: Arc<CompiledCoverageSites>,
        coverage_run: Option<CoverageRunId>,
    ) -> Self {
        Self {
            lexical: CompiledLexicalEnv::empty(),
            inline_slot: None,
            module,
            external,
            coverage_sites,
            coverage_run,
        }
    }

    fn with_slot(&self, _name: &str, value: Value) -> Self {
        Self {
            lexical: self.lexical_with_inline_spilled(),
            inline_slot: Some(value),
            module: self.module.clone(),
            external: self.external.clone(),
            coverage_sites: self.coverage_sites.clone(),
            coverage_run: self.coverage_run,
        }
    }

    fn with_slots(&self, values_oldest_to_newest: Vec<Value>) -> Self {
        if values_oldest_to_newest.len() == 1 {
            let mut values = values_oldest_to_newest;
            let value = values.remove(0);
            return self.with_slot("", value);
        }
        Self {
            lexical: CompiledLexicalEnv::with_slots(
                &self.lexical_with_inline_spilled(),
                values_oldest_to_newest,
            ),
            inline_slot: None,
            module: self.module.clone(),
            external: self.external.clone(),
            coverage_sites: self.coverage_sites.clone(),
            coverage_run: self.coverage_run,
        }
    }

    fn with_slot_and_external(&self, name: &str, value: Value) -> Self {
        Self {
            lexical: self.lexical_with_inline_spilled(),
            inline_slot: Some(value.clone()),
            module: self.module.clone(),
            external: Env::with_binding(&self.external, name.to_string(), value),
            coverage_sites: self.coverage_sites.clone(),
            coverage_run: self.coverage_run,
        }
    }

    fn local_get(&self, depth: u16, slot: u16) -> Option<Value> {
        if let Some(value) = &self.inline_slot {
            if depth == 0 && slot == 0 {
                return Some(value.clone());
            }
            let depth = depth.checked_sub(1)?;
            return self.lexical.get(depth, slot);
        }
        self.lexical.get(depth, slot)
    }

    fn lexical_for_capture(
        &self,
        plan: &ClosureCapturePlan,
    ) -> Result<CompiledLexicalEnv, KernelError> {
        self.lexical_with_inline_spilled()
            .capture(&plan.lexical_depths)
    }

    fn external_for_capture(&self, plan: &ClosureCapturePlan) -> Env {
        self.external.capture(&plan.external_names)
    }

    fn lexical_with_inline_spilled(&self) -> CompiledLexicalEnv {
        match &self.inline_slot {
            Some(value) => CompiledLexicalEnv::with_slot(&self.lexical, value.clone()),
            None => self.lexical.clone(),
        }
    }
}

#[derive(Clone, Debug)]
pub(crate) enum CExpr {
    Atom(Term),
    Var {
        name: String,
        sym: Sym,
        resolution: VarResolution,
        statement_site: u32,
    },
    Vector(Vec<Term>),
    Map(Vec<(TermOrdKey, Arc<CExpr>)>),
    Quote(Term),
    If {
        cond: Arc<CExpr>,
        then_expr: Arc<CExpr>,
        else_expr: Arc<CExpr>,
        decision_site: u32,
    },
    Begin(Vec<Arc<CExpr>>),
    Let(Vec<(String, Arc<CExpr>)>, Arc<CExpr>),
    FnUnary {
        param: String,
        body_term: Term,
        body: Arc<CExpr>,
        capture_plan: OnceLock<ClosureCapturePlan>,
    },
    Prim {
        op: PrimOp,
        args: Vec<Arc<CExpr>>,
    },
    PrimUnknown {
        op: String,
        args: Vec<Arc<CExpr>>,
    },
    SealNew,
    Seal(Arc<CExpr>, Arc<CExpr>),
    Unseal(Arc<CExpr>, Arc<CExpr>),
    App(Arc<CExpr>, Arc<CExpr>),
    AppN {
        callee: Arc<CExpr>,
        args: Box<[Arc<CExpr>]>,
        extra_app_ticks: u32,
    },
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct ClosureCapturePlan {
    lexical_depths: BTreeSet<usize>,
    external_names: BTreeSet<String>,
}

impl ClosureCapturePlan {
    pub(crate) fn for_body(body: &Arc<CExpr>) -> Self {
        let mut plan = Self::default();
        collect_compiled_captures(body, 1, &mut plan);
        plan
    }
}

fn collect_compiled_captures(expr: &Arc<CExpr>, introduced: usize, plan: &mut ClosureCapturePlan) {
    match expr.as_ref() {
        CExpr::Var {
            name, resolution, ..
        } => match resolution {
            VarResolution::Local { depth, .. } => {
                let depth = usize::from(*depth);
                if depth >= introduced {
                    plan.lexical_depths.insert(depth - introduced);
                }
            }
            VarResolution::External => {
                plan.external_names.insert(name.clone());
            }
            VarResolution::Module { .. } => {}
        },
        CExpr::Map(entries) => {
            for (_, value) in entries {
                collect_compiled_captures(value, introduced, plan);
            }
        }
        CExpr::If {
            cond,
            then_expr,
            else_expr,
            ..
        } => {
            collect_compiled_captures(cond, introduced, plan);
            collect_compiled_captures(then_expr, introduced, plan);
            collect_compiled_captures(else_expr, introduced, plan);
        }
        CExpr::Begin(items) => {
            for item in items {
                collect_compiled_captures(item, introduced, plan);
            }
        }
        CExpr::Let(bindings, body) => {
            for (index, (_, rhs)) in bindings.iter().enumerate() {
                collect_compiled_captures(rhs, introduced.saturating_add(index), plan);
            }
            collect_compiled_captures(body, introduced.saturating_add(bindings.len()), plan);
        }
        CExpr::FnUnary { body, .. } => {
            collect_compiled_captures(body, introduced.saturating_add(1), plan);
        }
        CExpr::Prim { args, .. } | CExpr::PrimUnknown { args, .. } => {
            for arg in args {
                collect_compiled_captures(arg, introduced, plan);
            }
        }
        CExpr::Seal(value, token) | CExpr::Unseal(value, token) | CExpr::App(value, token) => {
            collect_compiled_captures(value, introduced, plan);
            collect_compiled_captures(token, introduced, plan);
        }
        CExpr::AppN { callee, args, .. } => {
            collect_compiled_captures(callee, introduced, plan);
            for arg in args {
                collect_compiled_captures(arg, introduced, plan);
            }
        }
        CExpr::Atom(_) | CExpr::Vector(_) | CExpr::Quote(_) | CExpr::SealNew => {}
    }
}

#[derive(Clone, Debug)]
pub struct CompiledModule {
    forms: Vec<CompiledForm>,
    module_names: Vec<String>,
    coverage_sites: Arc<CompiledCoverageSites>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct CoverageSiteManifest {
    pub statement_sites: BTreeSet<String>,
    pub decision_sites: BTreeSet<String>,
    pub decision_conditions: BTreeMap<String, BTreeSet<String>>,
}

#[derive(Clone, Debug)]
enum CompiledForm {
    Def {
        name: String,
        module_slot: u32,
        expr: Arc<CExpr>,
    },
    Expr(Arc<CExpr>),
}

pub fn compile_module(forms: &[Term]) -> Result<CompiledModule, KernelError> {
    compile_module_with_site_namespace(forms, "")
}

pub fn compile_module_with_site_namespace(
    forms: &[Term],
    site_namespace: &str,
) -> Result<CompiledModule, KernelError> {
    compiled_compile::compile_module_with_site_namespace_impl(forms, site_namespace)
}

pub fn compiled_module_coverage_manifest(
    forms: &[Term],
    site_namespace: &str,
) -> Result<CoverageSiteManifest, KernelError> {
    let compiled = compile_module_with_site_namespace(forms, site_namespace)?;
    Ok(compiled_module_coverage_manifest_from_compiled(&compiled))
}

pub fn compiled_module_coverage_manifest_from_compiled(
    compiled: &CompiledModule,
) -> CoverageSiteManifest {
    compiled_coverage::compiled_module_coverage_manifest_from_compiled(compiled)
}

pub fn eval_compiled_module(
    ctx: &mut EvalCtx,
    env: &mut Env,
    m: &CompiledModule,
) -> Result<Value, KernelError> {
    env.mark_module_scope();
    let module = CompiledModuleCells::new(m.module_names.len());
    let coverage_run = ctx.coverage_begin_indexed_run(
        m.coverage_sites.statement_sites().len(),
        m.coverage_sites.decision_sites().len(),
    );
    let runtime = RuntimeEnv::new(env.clone(), module, m.coverage_sites.clone(), coverage_run);
    let result = ctx.run_panic_guarded("eval_compiled_module", |ctx| {
        let mut last = Value::data(Term::Nil);
        for f in &m.forms {
            match f {
                CompiledForm::Def {
                    name,
                    module_slot,
                    expr,
                } => {
                    let v = compiled_runtime::eval_cexpr_runtime(ctx, runtime.clone(), expr)?;
                    runtime.module.set(*module_slot, v.clone())?;
                    env.set_local(name.clone(), v);
                    last = Value::data(Term::Nil);
                }
                CompiledForm::Expr(e) => {
                    last = compiled_runtime::eval_cexpr_runtime(ctx, runtime.clone(), e)?;
                }
            }
        }
        Ok(last)
    });
    if let Some(run_id) = coverage_run {
        ctx.coverage_flush_indexed_run(
            run_id,
            m.coverage_sites.statement_sites(),
            m.coverage_sites.decision_sites(),
        )?;
    }
    result
}

pub fn eval_module_compiled(
    ctx: &mut EvalCtx,
    env: &mut Env,
    forms: &[Term],
) -> Result<Value, KernelError> {
    let m = compile_module(forms)?;
    eval_compiled_module(ctx, env, &m)
}

pub fn encode_compiled_module_blob(m: &CompiledModule) -> Result<Vec<u8>, KernelError> {
    compiled_blob::encode_compiled_module_blob(m)
}

pub fn decode_compiled_module_blob(bytes: &[u8]) -> Result<CompiledModule, KernelError> {
    compiled_blob::decode_compiled_module_blob(bytes)
}

pub(crate) struct CompiledClosureCall {
    pub(crate) external_env: Env,
    pub(crate) lexical_env: Option<CompiledLexicalEnv>,
    pub(crate) module_env: Option<CompiledModuleCells>,
    pub(crate) coverage_sites: Arc<CompiledCoverageSites>,
    pub(crate) param: crate::value::Sym,
    pub(crate) bind_external_param: bool,
    pub(crate) body: Arc<CExpr>,
    pub(crate) arg: Value,
}

pub(crate) fn apply_compiled_closure(
    ctx: &mut EvalCtx,
    call: CompiledClosureCall,
) -> Result<Value, KernelError> {
    compiled_runtime::eval_compiled_closure_body_scoped(ctx, call)
}
