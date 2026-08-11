use gc_coreform::Term;

use crate::infer::InferSession;
use crate::ty::{RowTail, Ty};

pub(crate) fn prim_type(op: &str, args: &[Ty], arg_terms: &[&Term], sess: &mut InferSession) -> Ty {
    match op {
        "int/add" | "int/sub" | "int/mul" | "int/div" | "int/mod" => {
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
        "dec/parse" => unary_prim(op, args, Ty::Str, Ty::Dec, sess),
        "dec/to-str" => unary_prim(op, args, Ty::Dec, Ty::Str, sess),
        "dec/from-int" => unary_prim(op, args, Ty::Int, Ty::Dec, sess),
        "dec/add" | "dec/sub" | "dec/mul" => binary_prim(op, args, Ty::Dec, Ty::Dec, sess),
        "dec/eq?" | "dec/lt?" => binary_prim(op, args, Ty::Dec, Ty::Bool, sess),
        "core/eq?" | "sym/eq?" => Ty::Bool,
        "str/concat" => binary_prim(op, args, Ty::Str, Ty::Str, sess),
        "str/len" | "str/scalar-len" | "str/grapheme-len" => {
            unary_prim(op, args, Ty::Str, Ty::Int, sess)
        }
        "str/nfc" => unary_prim(op, args, Ty::Str, Ty::Str, sess),
        "str/grapheme-slice" => {
            if args.len() != 3 {
                sess.errors
                    .push(format!("prim {op} expects 3 args, got {}", args.len()));
                return Ty::Any;
            }
            if args[0] != Ty::Str || args[1] != Ty::Int || args[2] != Ty::Int {
                sess.errors.push(format!("prim {op} expects Str, Int, Int"));
                return Ty::Any;
            }
            Ty::Str
        }
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

fn unary_prim(op: &str, args: &[Ty], input: Ty, output: Ty, sess: &mut InferSession) -> Ty {
    if args.len() != 1 {
        sess.errors
            .push(format!("prim {op} expects 1 arg, got {}", args.len()));
        return Ty::Any;
    }
    if args[0] != input {
        sess.errors.push(format!(
            "prim {op} expects {}",
            gc_coreform::print_term(&input.to_term())
        ));
        return Ty::Any;
    }
    output
}

fn binary_prim(op: &str, args: &[Ty], input: Ty, output: Ty, sess: &mut InferSession) -> Ty {
    if args.len() != 2 {
        sess.errors
            .push(format!("prim {op} expects 2 args, got {}", args.len()));
        return Ty::Any;
    }
    if args[0] != input || args[1] != input {
        let name = gc_coreform::print_term(&input.to_term());
        sess.errors
            .push(format!("prim {op} expects {name}, {name}"));
        return Ty::Any;
    }
    output
}

fn literal_map_key(t: &Term) -> Option<String> {
    match t {
        Term::Symbol(s) => Some(s.clone()),
        Term::Str(s) => Some(s.clone()),
        _ => {
            let items = t.as_proper_list()?;
            if items.len() == 2
                && matches!(items[0], Term::Symbol(s) if gc_coreform::SpecialForm::from_symbol(s) == Some(gc_coreform::SpecialForm::Quote))
            {
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
