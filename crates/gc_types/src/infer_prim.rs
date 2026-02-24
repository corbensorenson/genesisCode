use gc_coreform::Term;

use crate::infer::InferSession;
use crate::ty::{RowTail, Ty};

pub(crate) fn prim_type(op: &str, args: &[Ty], arg_terms: &[&Term], sess: &mut InferSession) -> Ty {
    match op {
        "int/add" | "int/sub" | "int/mul" => {
            if args.len() != 2 {
                sess.errors
                    .push(format!("prim {op} expects 2 args, got {}", args.len()));
                return Ty::Any;
            }
            if args[0] != Ty::Int || args[1] != Ty::Int {
                sess.errors.push(format!("prim {op} expects Int, Int"));
                return Ty::Any;
            }
            Ty::Int
        }
        "int/eq?" | "int/lt?" => {
            if args.len() != 2 {
                sess.errors
                    .push(format!("prim {op} expects 2 args, got {}", args.len()));
                return Ty::Any;
            }
            if args[0] != Ty::Int || args[1] != Ty::Int {
                sess.errors.push(format!("prim {op} expects Int, Int"));
                return Ty::Any;
            }
            Ty::Bool
        }
        "core/eq?" | "sym/eq?" => Ty::Bool,
        "str/concat" => Ty::Str,
        "bytes/len" => Ty::Int,
        "bytes/concat" => Ty::Bytes,
        "pair/cons" | "pair/car" | "pair/cdr" | "list/is-nil?" => Ty::Any,
        "map/get" => {
            if args.len() != 2 {
                sess.errors
                    .push(format!("prim {op} expects 2 args, got {}", args.len()));
                return Ty::Any;
            }
            match &args[0] {
                Ty::Rec { fields, tail } => {
                    let Some(key) = literal_map_key(arg_terms[1]) else {
                        return Ty::Any;
                    };
                    if let Some(found) = fields.get(&key) {
                        return found.clone();
                    }
                    if !tail.is_open() {
                        sess.warnings
                            .push(format!("prim map/get missing closed-row key {key}"));
                    }
                    Ty::Any
                }
                Ty::Any => Ty::Any,
                _ => {
                    sess.errors
                        .push("prim map/get expects Rec, key".to_string());
                    Ty::Any
                }
            }
        }
        "map/put" => {
            if args.len() != 3 {
                sess.errors
                    .push(format!("prim {op} expects 3 args, got {}", args.len()));
                return Ty::Any;
            }
            match &args[0] {
                Ty::Rec { fields, tail } => {
                    let mut next_fields = fields.clone();
                    if let Some(key) = literal_map_key(arg_terms[1]) {
                        next_fields.insert(key, args[2].clone());
                        Ty::Rec {
                            fields: next_fields,
                            tail: tail.clone(),
                        }
                    } else {
                        Ty::Rec {
                            fields: next_fields,
                            tail: RowTail::Any,
                        }
                    }
                }
                Ty::Any => Ty::Any,
                _ => {
                    sess.errors
                        .push("prim map/put expects Rec, key, value".to_string());
                    Ty::Any
                }
            }
        }
        "map/merge" => {
            if args.len() != 2 {
                sess.errors
                    .push(format!("prim {op} expects 2 args, got {}", args.len()));
                return Ty::Any;
            }
            match (&args[0], &args[1]) {
                (
                    Ty::Rec {
                        fields: lf,
                        tail: lt,
                    },
                    Ty::Rec {
                        fields: rf,
                        tail: rt,
                    },
                ) => {
                    let mut fields = lf.clone();
                    for (k, v) in rf {
                        fields.insert(k.clone(), v.clone());
                    }
                    let tail = if matches!(lt, RowTail::Closed) && matches!(rt, RowTail::Closed) {
                        RowTail::Closed
                    } else {
                        RowTail::Any
                    };
                    Ty::Rec { fields, tail }
                }
                (Ty::Any, _) | (_, Ty::Any) => Ty::Any,
                _ => {
                    sess.errors
                        .push("prim map/merge expects Rec, Rec".to_string());
                    Ty::Any
                }
            }
        }
        "vec/get" | "vec/push" => Ty::Any,
        _ => Ty::Any,
    }
}

fn literal_map_key(t: &Term) -> Option<String> {
    match t {
        Term::Symbol(s) => Some(s.clone()),
        Term::Str(s) => Some(s.clone()),
        _ => {
            let items = t.as_proper_list()?;
            if items.len() == 2 && matches!(items[0], Term::Symbol(s) if s == "quote") {
                match items[1] {
                    Term::Symbol(s) => Some(s.clone()),
                    Term::Str(s) => Some(s.clone()),
                    _ => None,
                }
            } else {
                None
            }
        }
    }
}
