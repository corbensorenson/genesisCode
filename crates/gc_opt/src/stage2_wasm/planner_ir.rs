use super::*;

#[derive(Debug, Clone)]
pub(super) enum Stmt {
    Def(String, Term),
    Expr(Term),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Ty {
    I64,
    BoolI32,
    NilI32,
    SymI32,
    StrI32,
    BytesI32,
}

#[derive(Debug, Clone, Copy)]
pub(super) struct Local {
    pub(super) idx: u32,
    pub(super) ty: Ty,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum PrimOp {
    Add,
    Sub,
    Mul,
    EqI64,
    EqI32,
    EqAlwaysFalse,
    Lt,
}

#[derive(Debug, Clone)]
pub(super) struct InlinableFnDef {
    pub(super) param: String,
    pub(super) body: Term,
    pub(super) capture: FnCapture,
}

#[derive(Debug, Clone)]
pub(super) enum FnCapture {
    GlobalFrame,
    Lexical(BTreeMap<String, Local>),
}

#[derive(Debug, Clone)]
pub(super) struct LetBinding {
    pub(super) idx: u32,
    pub(super) expr: PExpr,
}

#[derive(Debug, Clone)]
pub(super) struct CallableHead {
    pub(super) param: String,
    pub(super) body: Term,
    pub(super) base_env: BTreeMap<String, Local>,
    pub(super) def_name: Option<String>,
}

#[derive(Debug, Clone)]
pub(super) enum PExpr {
    Nil,
    Int(i64),
    Bool(bool),
    Sym(i32),
    Str(i32),
    Bytes(i32),
    Local(Local),
    Prim {
        op: PrimOp,
        lhs: Box<PExpr>,
        rhs: Box<PExpr>,
        ty: Ty,
    },
    If {
        cond: Box<PExpr>,
        then_expr: Box<PExpr>,
        else_expr: Box<PExpr>,
        cond_ty: Ty,
        ty: Ty,
    },
    Begin {
        exprs: Vec<PExpr>,
        ty: Ty,
    },
    Let {
        bindings: Vec<LetBinding>,
        body: Vec<PExpr>,
        ty: Ty,
    },
}

impl PExpr {
    pub(super) fn ty(&self) -> Ty {
        match self {
            PExpr::Nil => Ty::NilI32,
            PExpr::Int(_) => Ty::I64,
            PExpr::Bool(_) => Ty::BoolI32,
            PExpr::Sym(_) => Ty::SymI32,
            PExpr::Str(_) => Ty::StrI32,
            PExpr::Bytes(_) => Ty::BytesI32,
            PExpr::Local(l) => l.ty,
            PExpr::Prim { ty, .. } => *ty,
            PExpr::If { ty, .. } => *ty,
            PExpr::Begin { ty, .. } => *ty,
            PExpr::Let { ty, .. } => *ty,
        }
    }
}

#[derive(Debug, Clone)]
pub(super) enum PStmt {
    Def { name: String, idx: u32, expr: PExpr },
    Expr(PExpr),
}

#[derive(Debug, Default)]
pub(super) struct Planner {
    pub(super) locals: Vec<Ty>,
    pub(super) expanding_fn_defs: Vec<String>,
    pub(super) symbol_ids: BTreeMap<String, i32>,
    pub(super) string_ids: BTreeMap<String, i32>,
    pub(super) bytes_ids: BTreeMap<Vec<u8>, i32>,
    pub(super) global_const_vector_aliases: BTreeMap<String, Vec<Term>>,
    pub(super) global_const_map_aliases: BTreeMap<String, BTreeMap<TermOrdKey, Term>>,
    pub(super) local_const_int_values: BTreeMap<u32, i64>,
    pub(super) local_const_symbol_ids: BTreeMap<u32, i32>,
    pub(super) local_const_string_ids: BTreeMap<u32, i32>,
    pub(super) local_const_bytes_ids: BTreeMap<u32, i32>,
}

impl Planner {
    pub(super) fn alloc_local(&mut self, ty: Ty) -> Result<u32, Stage2CompileError> {
        let idx = u32::try_from(self.locals.len())
            .map_err(|_| Stage2CompileError::Internal("too many wasm locals".to_string()))?;
        self.locals.push(ty);
        Ok(idx)
    }

    pub(super) fn intern_symbol(&mut self, sym: &str) -> Result<i32, Stage2CompileError> {
        if let Some(id) = self.symbol_ids.get(sym) {
            return Ok(*id);
        }
        let next = i32::try_from(self.symbol_ids.len()).map_err(|_| {
            Stage2CompileError::Internal("too many interned symbols for stage2".to_string())
        })?;
        self.symbol_ids.insert(sym.to_string(), next);
        Ok(next)
    }

    pub(super) fn intern_string(&mut self, s: &str) -> Result<i32, Stage2CompileError> {
        if let Some(id) = self.string_ids.get(s) {
            return Ok(*id);
        }
        let next = i32::try_from(self.string_ids.len()).map_err(|_| {
            Stage2CompileError::Internal("too many interned strings for stage2".to_string())
        })?;
        self.string_ids.insert(s.to_string(), next);
        Ok(next)
    }

    pub(super) fn intern_bytes(&mut self, bs: &[u8]) -> Result<i32, Stage2CompileError> {
        if let Some(id) = self.bytes_ids.get(bs) {
            return Ok(*id);
        }
        let next = i32::try_from(self.bytes_ids.len()).map_err(|_| {
            Stage2CompileError::Internal("too many interned byte strings for stage2".to_string())
        })?;
        self.bytes_ids.insert(bs.to_vec(), next);
        Ok(next)
    }
}

pub(super) fn planner_symbol_table(planner: &Planner) -> Result<Vec<String>, Stage2CompileError> {
    if planner.symbol_ids.is_empty() {
        return Ok(Vec::new());
    }
    let mut out = vec![String::new(); planner.symbol_ids.len()];
    for (sym, id) in &planner.symbol_ids {
        let idx = usize::try_from(*id)
            .map_err(|_| Stage2CompileError::Internal("negative symbol id".to_string()))?;
        if idx >= out.len() {
            return Err(Stage2CompileError::Internal(
                "symbol id out of range".to_string(),
            ));
        }
        out[idx] = sym.clone();
    }
    Ok(out)
}

pub(super) fn planner_string_table(planner: &Planner) -> Result<Vec<String>, Stage2CompileError> {
    if planner.string_ids.is_empty() {
        return Ok(Vec::new());
    }
    let mut out = vec![String::new(); planner.string_ids.len()];
    for (s, id) in &planner.string_ids {
        let idx = usize::try_from(*id)
            .map_err(|_| Stage2CompileError::Internal("negative string id".to_string()))?;
        if idx >= out.len() {
            return Err(Stage2CompileError::Internal(
                "string id out of range".to_string(),
            ));
        }
        out[idx] = s.clone();
    }
    Ok(out)
}

pub(super) fn planner_bytes_table(planner: &Planner) -> Result<Vec<Vec<u8>>, Stage2CompileError> {
    if planner.bytes_ids.is_empty() {
        return Ok(Vec::new());
    }
    let mut out = vec![Vec::new(); planner.bytes_ids.len()];
    for (bs, id) in &planner.bytes_ids {
        let idx = usize::try_from(*id)
            .map_err(|_| Stage2CompileError::Internal("negative bytes id".to_string()))?;
        if idx >= out.len() {
            return Err(Stage2CompileError::Internal(
                "bytes id out of range".to_string(),
            ));
        }
        out[idx] = bs.clone();
    }
    Ok(out)
}

pub(super) fn const_string_id_with_map(expr: &PExpr, map: &BTreeMap<u32, i32>) -> Option<i32> {
    match expr {
        PExpr::Str(id) => Some(*id),
        PExpr::Local(local) if local.ty == Ty::StrI32 => map.get(&local.idx).copied(),
        PExpr::Begin { exprs, ty } if *ty == Ty::StrI32 => {
            exprs.last().and_then(|e| const_string_id_with_map(e, map))
        }
        PExpr::Let { bindings, body, ty } if *ty == Ty::StrI32 => {
            let mut scoped = map.clone();
            for b in bindings {
                if let Some(id) = const_string_id_with_map(&b.expr, &scoped) {
                    scoped.insert(b.idx, id);
                } else {
                    scoped.remove(&b.idx);
                }
            }
            body.last()
                .and_then(|e| const_string_id_with_map(e, &scoped))
        }
        PExpr::If {
            then_expr,
            else_expr,
            ty,
            ..
        } if *ty == Ty::StrI32 => {
            let a = const_string_id_with_map(then_expr, map);
            let b = const_string_id_with_map(else_expr, map);
            if a.is_some() && a == b { a } else { None }
        }
        _ => None,
    }
}

pub(super) fn const_int_value_with_map(expr: &PExpr, map: &BTreeMap<u32, i64>) -> Option<i64> {
    match expr {
        PExpr::Int(n) => Some(*n),
        PExpr::Local(local) if local.ty == Ty::I64 => map.get(&local.idx).copied(),
        PExpr::Prim {
            op: PrimOp::Add,
            lhs,
            rhs,
            ty: Ty::I64,
        } => {
            let a = const_int_value_with_map(lhs, map)?;
            let b = const_int_value_with_map(rhs, map)?;
            a.checked_add(b)
        }
        PExpr::Prim {
            op: PrimOp::Sub,
            lhs,
            rhs,
            ty: Ty::I64,
        } => {
            let a = const_int_value_with_map(lhs, map)?;
            let b = const_int_value_with_map(rhs, map)?;
            a.checked_sub(b)
        }
        PExpr::Prim {
            op: PrimOp::Mul,
            lhs,
            rhs,
            ty: Ty::I64,
        } => {
            let a = const_int_value_with_map(lhs, map)?;
            let b = const_int_value_with_map(rhs, map)?;
            a.checked_mul(b)
        }
        PExpr::Begin { exprs, ty } if *ty == Ty::I64 => {
            exprs.last().and_then(|e| const_int_value_with_map(e, map))
        }
        PExpr::Let { bindings, body, ty } if *ty == Ty::I64 => {
            let mut scoped = map.clone();
            for b in bindings {
                if let Some(n) = const_int_value_with_map(&b.expr, &scoped) {
                    scoped.insert(b.idx, n);
                } else {
                    scoped.remove(&b.idx);
                }
            }
            body.last()
                .and_then(|e| const_int_value_with_map(e, &scoped))
        }
        PExpr::If {
            then_expr,
            else_expr,
            ty,
            ..
        } if *ty == Ty::I64 => {
            let a = const_int_value_with_map(then_expr, map);
            let b = const_int_value_with_map(else_expr, map);
            if a.is_some() && a == b { a } else { None }
        }
        _ => None,
    }
}

pub(super) fn const_symbol_id_with_map(expr: &PExpr, map: &BTreeMap<u32, i32>) -> Option<i32> {
    match expr {
        PExpr::Sym(id) => Some(*id),
        PExpr::Local(local) if local.ty == Ty::SymI32 => map.get(&local.idx).copied(),
        PExpr::Begin { exprs, ty } if *ty == Ty::SymI32 => {
            exprs.last().and_then(|e| const_symbol_id_with_map(e, map))
        }
        PExpr::Let { bindings, body, ty } if *ty == Ty::SymI32 => {
            let mut scoped = map.clone();
            for b in bindings {
                if let Some(id) = const_symbol_id_with_map(&b.expr, &scoped) {
                    scoped.insert(b.idx, id);
                } else {
                    scoped.remove(&b.idx);
                }
            }
            body.last()
                .and_then(|e| const_symbol_id_with_map(e, &scoped))
        }
        PExpr::If {
            then_expr,
            else_expr,
            ty,
            ..
        } if *ty == Ty::SymI32 => {
            let a = const_symbol_id_with_map(then_expr, map);
            let b = const_symbol_id_with_map(else_expr, map);
            if a.is_some() && a == b { a } else { None }
        }
        _ => None,
    }
}

pub(super) fn const_bytes_id_with_map(expr: &PExpr, map: &BTreeMap<u32, i32>) -> Option<i32> {
    match expr {
        PExpr::Bytes(id) => Some(*id),
        PExpr::Local(local) if local.ty == Ty::BytesI32 => map.get(&local.idx).copied(),
        PExpr::Begin { exprs, ty } if *ty == Ty::BytesI32 => {
            exprs.last().and_then(|e| const_bytes_id_with_map(e, map))
        }
        PExpr::Let { bindings, body, ty } if *ty == Ty::BytesI32 => {
            let mut scoped = map.clone();
            for b in bindings {
                if let Some(id) = const_bytes_id_with_map(&b.expr, &scoped) {
                    scoped.insert(b.idx, id);
                } else {
                    scoped.remove(&b.idx);
                }
            }
            body.last()
                .and_then(|e| const_bytes_id_with_map(e, &scoped))
        }
        PExpr::If {
            then_expr,
            else_expr,
            ty,
            ..
        } if *ty == Ty::BytesI32 => {
            let a = const_bytes_id_with_map(then_expr, map);
            let b = const_bytes_id_with_map(else_expr, map);
            if a.is_some() && a == b { a } else { None }
        }
        _ => None,
    }
}

pub(super) fn record_local_const_ids(planner: &mut Planner, idx: u32, expr: &PExpr) {
    if let Some(n) = const_int_value_with_map(expr, &planner.local_const_int_values) {
        planner.local_const_int_values.insert(idx, n);
    } else {
        planner.local_const_int_values.remove(&idx);
    }
    if let Some(id) = const_symbol_id_with_map(expr, &planner.local_const_symbol_ids) {
        planner.local_const_symbol_ids.insert(idx, id);
    } else {
        planner.local_const_symbol_ids.remove(&idx);
    }
    if let Some(id) = const_string_id_with_map(expr, &planner.local_const_string_ids) {
        planner.local_const_string_ids.insert(idx, id);
    } else {
        planner.local_const_string_ids.remove(&idx);
    }
    if let Some(id) = const_bytes_id_with_map(expr, &planner.local_const_bytes_ids) {
        planner.local_const_bytes_ids.insert(idx, id);
    } else {
        planner.local_const_bytes_ids.remove(&idx);
    }
}
