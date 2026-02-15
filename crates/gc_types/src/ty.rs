use std::collections::{BTreeMap, BTreeSet};

use gc_coreform::{Term, print_term};

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum RowTail {
    Closed,      // nil
    Any,         // ?
    Var(String), // symbol var
}

impl RowTail {
    pub fn is_open(&self) -> bool {
        !matches!(self, RowTail::Closed)
    }

    pub fn to_term(&self) -> Term {
        match self {
            RowTail::Closed => Term::Nil,
            RowTail::Any => Term::Symbol("?".to_string()),
            RowTail::Var(v) => Term::Symbol(v.clone()),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EffRow {
    pub ops: BTreeSet<String>,
    pub tail: RowTail, // nil, ?, or a symbol var
}

impl EffRow {
    pub fn empty() -> Self {
        Self {
            ops: BTreeSet::new(),
            tail: RowTail::Closed,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Ty {
    Any,
    Int,
    Bool,
    Nil,
    Str,
    Bytes,
    Symbol,

    Msg {
        op: Option<String>,
        payload: Box<Ty>,
    },

    Fn {
        param: Box<Ty>,
        ret: Box<Ty>,
        eff: EffRow,
    },

    Prog {
        ret: Box<Ty>,
        eff: EffRow,
    },

    Rec {
        fields: BTreeMap<String, Ty>,
        tail: RowTail,
    },

    Contract {
        methods: BTreeMap<String, Ty>, // op -> method type (usually Fn)
        tail: RowTail,
    },
}

impl Ty {
    pub fn to_term(&self) -> Term {
        match self {
            Ty::Any => Term::Symbol("?".to_string()),
            Ty::Int => Term::Symbol("Int".to_string()),
            Ty::Bool => Term::Symbol("Bool".to_string()),
            Ty::Nil => Term::Symbol("Nil".to_string()),
            Ty::Str => Term::Symbol("Str".to_string()),
            Ty::Bytes => Term::Symbol("Bytes".to_string()),
            Ty::Symbol => Term::Symbol("Symbol".to_string()),
            Ty::Msg { op: _, payload } => {
                Term::list(vec![Term::Symbol("Msg".to_string()), payload.to_term()])
            }
            Ty::Fn { param, ret, eff } => Term::list(vec![
                Term::Symbol("Fn".to_string()),
                param.to_term(),
                ret.to_term(),
                eff_to_term(eff),
            ]),
            Ty::Prog { ret, eff } => Term::list(vec![
                Term::Symbol("Prog".to_string()),
                ret.to_term(),
                eff_to_term(eff),
            ]),
            Ty::Rec { fields, tail } => {
                let mut xs = Vec::new();
                for (k, v) in fields {
                    xs.push(Term::Vector(vec![Term::Symbol(k.clone()), v.to_term()]));
                }
                Term::list(vec![
                    Term::Symbol("Rec".to_string()),
                    Term::Vector(xs),
                    tail.to_term(),
                ])
            }
            Ty::Contract { methods, tail } => {
                let mut xs = Vec::new();
                for (op, mt) in methods {
                    xs.push(Term::Vector(vec![Term::Symbol(op.clone()), mt.to_term()]));
                }
                Term::list(vec![
                    Term::Symbol("Contract".to_string()),
                    Term::Vector(xs),
                    tail.to_term(),
                ])
            }
        }
    }
}

pub fn parse_type_term(t: &Term) -> Result<Ty, String> {
    match t {
        Term::Symbol(s) if s == "?" => return Ok(Ty::Any),
        Term::Symbol(s) if s == "Int" => return Ok(Ty::Int),
        Term::Symbol(s) if s == "Bool" => return Ok(Ty::Bool),
        Term::Symbol(s) if s == "Nil" || s == "nil" => return Ok(Ty::Nil),
        Term::Symbol(s) if s == "Str" => return Ok(Ty::Str),
        Term::Symbol(s) if s == "Bytes" => return Ok(Ty::Bytes),
        Term::Symbol(s) if s == "Symbol" => return Ok(Ty::Symbol),
        Term::Pair(_, _) => {}
        _ => return Err(format!("unsupported type term: {}", print_term(t))),
    }

    let Some(items) = t.as_proper_list() else {
        return Err(format!(
            "type term must be a proper list: {}",
            print_term(t)
        ));
    };
    if items.is_empty() {
        return Err("empty type term list".to_string());
    }
    let Term::Symbol(h) = items[0] else {
        return Err(format!(
            "type head must be a symbol: {}",
            print_term(items[0])
        ));
    };
    match h.as_str() {
        "Fn" => {
            if items.len() != 3 && items.len() != 4 {
                return Err(format!(
                    "(Fn A B (Eff ...)) expects 2 or 3 arguments, got {}",
                    items.len() - 1
                ));
            }
            let param = parse_type_term(items[1])?;
            let ret = parse_type_term(items[2])?;
            let eff = if items.len() == 4 {
                parse_eff_term(items[3])?
            } else {
                EffRow::empty()
            };
            Ok(Ty::Fn {
                param: Box::new(param),
                ret: Box::new(ret),
                eff,
            })
        }
        "Prog" => {
            if items.len() != 3 {
                return Err(format!(
                    "(Prog T (Eff ...)) expects 2 arguments, got {}",
                    items.len() - 1
                ));
            }
            let ret = parse_type_term(items[1])?;
            let eff = parse_eff_term(items[2])?;
            Ok(Ty::Prog {
                ret: Box::new(ret),
                eff,
            })
        }
        "Msg" => {
            if items.len() != 2 {
                return Err(format!(
                    "(Msg Payload) expects 1 argument, got {}",
                    items.len() - 1
                ));
            }
            let payload = parse_type_term(items[1])?;
            Ok(Ty::Msg {
                op: None,
                payload: Box::new(payload),
            })
        }
        "Rec" => {
            if items.len() != 3 {
                return Err(format!(
                    "(Rec [[:k T] ...] tail) expects 2 arguments, got {}",
                    items.len() - 1
                ));
            }
            let fields = parse_row_fields(items[1])?;
            let tail = parse_row_tail(items[2])?;
            Ok(Ty::Rec { fields, tail })
        }
        "Contract" => {
            if items.len() != 3 {
                return Err(format!(
                    "(Contract [[op Ty] ...] tail) expects 2 arguments, got {}",
                    items.len() - 1
                ));
            }
            let methods = parse_row_fields(items[1])?;
            let tail = parse_row_tail(items[2])?;
            Ok(Ty::Contract { methods, tail })
        }
        "Eff" => Err("(Eff ...) is an effect-row term, not a type".to_string()),
        _ => Err(format!("unknown type constructor {h}")),
    }
}

fn parse_eff_term(t: &Term) -> Result<EffRow, String> {
    let Some(items) = t.as_proper_list() else {
        return Err(format!("eff row must be a list: {}", print_term(t)));
    };
    if items.len() != 3 || !matches!(items[0], Term::Symbol(s) if s == "Eff") {
        return Err(format!(
            "eff row must look like (Eff [ops] tail), got {}",
            print_term(t)
        ));
    }
    let ops_term = items[1];
    let tail_term = items[2];

    let mut ops = BTreeSet::new();
    match ops_term {
        Term::Vector(xs) => {
            for x in xs {
                if let Term::Symbol(s) = x {
                    ops.insert(s.clone());
                } else {
                    return Err(format!("eff op must be a symbol, got {}", print_term(x)));
                }
            }
        }
        Term::Nil => {}
        _ => {
            return Err(format!(
                "eff ops must be a vector, got {}",
                print_term(ops_term)
            ));
        }
    }

    let tail = parse_row_tail(tail_term)?;
    Ok(EffRow { ops, tail })
}

fn parse_row_tail(t: &Term) -> Result<RowTail, String> {
    match t {
        Term::Nil => Ok(RowTail::Closed),
        Term::Symbol(s) if s == "?" => Ok(RowTail::Any),
        Term::Symbol(var) => Ok(RowTail::Var(var.clone())),
        _ => Err(format!(
            "row tail must be nil, ?, or a symbol var, got {}",
            print_term(t)
        )),
    }
}

fn parse_row_fields(t: &Term) -> Result<BTreeMap<String, Ty>, String> {
    let Term::Vector(xs) = t else {
        return Err(format!(
            "row fields must be a vector, got {}",
            print_term(t)
        ));
    };
    let mut out = BTreeMap::new();
    for x in xs {
        let Term::Vector(pair) = x else {
            return Err(format!(
                "row entry must be a 2-vector, got {}",
                print_term(x)
            ));
        };
        if pair.len() != 2 {
            return Err(format!(
                "row entry must have 2 items, got {}",
                print_term(x)
            ));
        }
        let Term::Symbol(k) = &pair[0] else {
            return Err(format!(
                "row label must be a symbol, got {}",
                print_term(&pair[0])
            ));
        };
        out.insert(k.clone(), parse_type_term(&pair[1])?);
    }
    Ok(out)
}

fn eff_to_term(eff: &EffRow) -> Term {
    let ops: Vec<Term> = eff.ops.iter().cloned().map(Term::Symbol).collect();
    Term::list(vec![
        Term::Symbol("Eff".to_string()),
        Term::Vector(ops),
        eff.tail.to_term(),
    ])
}
