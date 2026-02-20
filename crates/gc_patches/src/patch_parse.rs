use super::*;

impl Patch {
    pub(super) fn from_term(t: &Term) -> Result<Self, PatchError> {
        let Term::Map(m) = t else {
            return Err(PatchError::Validate("patch must be a map".to_string()));
        };
        let version = get_int(m, ":version")?.unwrap_or(1);
        let intent = get_str(m, ":intent")?.unwrap_or_else(|| "".to_string());
        let provenance = m
            .get(&TermOrdKey(Term::Symbol(":provenance".to_string())))
            .cloned()
            .unwrap_or(Term::Map(BTreeMap::new()));
        let ops_t = m
            .get(&TermOrdKey(Term::Symbol(":ops".to_string())))
            .ok_or_else(|| PatchError::Validate("missing :ops".to_string()))?;
        let Term::Vector(ops) = ops_t else {
            return Err(PatchError::Validate(":ops must be a vector".to_string()));
        };
        let mut parsed = Vec::new();
        for op in ops {
            parsed.push(parse_op(op)?);
        }
        Ok(Self {
            version,
            intent,
            provenance,
            ops: parsed,
        })
    }
}

pub(super) fn parse_op(t: &Term) -> Result<PatchOp, PatchError> {
    let Term::Map(m) = t else {
        return Err(PatchError::Validate("op must be a map".to_string()));
    };
    let op = match m.get(&TermOrdKey(Term::Symbol(":op".to_string()))) {
        Some(Term::Symbol(s)) => s.as_str(),
        Some(x) => {
            return Err(PatchError::Validate(format!(
                ":op must be symbol, got {}",
                print_term(x)
            )));
        }
        None => return Err(PatchError::Validate("missing :op".to_string())),
    };
    match op {
        ":replace-node" => {
            let module_path = get_str(m, ":module-path")?.ok_or_else(|| {
                PatchError::Validate("replace-node missing :module-path".to_string())
            })?;
            let path_t = m
                .get(&TermOrdKey(Term::Symbol(":path".to_string())))
                .ok_or_else(|| PatchError::Validate("replace-node missing :path".to_string()))?;
            let path = parse_path(path_t)?;
            let new_term = m
                .get(&TermOrdKey(Term::Symbol(":new".to_string())))
                .ok_or_else(|| PatchError::Validate("replace-node missing :new".to_string()))?
                .clone();
            Ok(PatchOp::ReplaceNode {
                module_path,
                path,
                new_term,
            })
        }
        ":replace-node-id" => {
            let module_path = get_str(m, ":module-path")?.ok_or_else(|| {
                PatchError::Validate("replace-node-id missing :module-path".to_string())
            })?;
            let node_id = get_str(m, ":node-id")?.ok_or_else(|| {
                PatchError::Validate("replace-node-id missing :node-id".to_string())
            })?;
            if node_id.trim().is_empty() {
                return Err(PatchError::Validate(
                    "replace-node-id :node-id must be non-empty".to_string(),
                ));
            }
            let new_term = m
                .get(&TermOrdKey(Term::Symbol(":new".to_string())))
                .ok_or_else(|| PatchError::Validate("replace-node-id missing :new".to_string()))?
                .clone();
            Ok(PatchOp::ReplaceNodeId {
                module_path,
                node_id,
                new_term,
            })
        }
        ":add-module" => {
            let module_path = get_str(m, ":module-path")?.ok_or_else(|| {
                PatchError::Validate("add-module missing :module-path".to_string())
            })?;
            let content_t = m
                .get(&TermOrdKey(Term::Symbol(":content".to_string())))
                .ok_or_else(|| PatchError::Validate("add-module missing :content".to_string()))?;
            let content = match content_t {
                Term::Str(s) => ModuleContent::Source(s.clone()),
                Term::Vector(xs) => ModuleContent::Forms(xs.clone()),
                _ => {
                    return Err(PatchError::Validate(
                        ":content must be string or vector".to_string(),
                    ));
                }
            };
            Ok(PatchOp::AddModule {
                module_path,
                content,
            })
        }
        ":remove-module" => {
            let module_path = get_str(m, ":module-path")?.ok_or_else(|| {
                PatchError::Validate("remove-module missing :module-path".to_string())
            })?;
            Ok(PatchOp::RemoveModule { module_path })
        }
        ":update-manifest" => {
            let set = m
                .get(&TermOrdKey(Term::Symbol(":set".to_string())))
                .cloned();
            let obligations_add = get_sym_vec(m, ":obligations-add")?;
            let obligations_remove = get_sym_vec(m, ":obligations-remove")?;
            let tests_add = get_sym_vec(m, ":tests-add")?;
            let tests_remove = get_sym_vec(m, ":tests-remove")?;
            let caps_policy = get_str(m, ":caps-policy")?;
            Ok(PatchOp::UpdateManifest {
                set,
                obligations_add,
                obligations_remove,
                tests_add,
                tests_remove,
                caps_policy,
            })
        }
        other => Err(PatchError::Validate(format!("unknown op {other}"))),
    }
}

pub(super) fn parse_path(t: &Term) -> Result<Vec<PathStep>, PatchError> {
    let Term::Vector(steps) = t else {
        return Err(PatchError::Validate(":path must be a vector".to_string()));
    };
    let mut out = Vec::new();
    for s in steps {
        let Term::Vector(items) = s else {
            return Err(PatchError::Validate(
                "path step must be a vector".to_string(),
            ));
        };
        if items.is_empty() {
            return Err(PatchError::Validate("empty path step".to_string()));
        }
        let tag = match &items[0] {
            Term::Symbol(x) => x.as_str(),
            other => {
                return Err(PatchError::Validate(format!(
                    "bad path tag {}",
                    print_term(other)
                )));
            }
        };
        match tag {
            ":form" => {
                if items.len() != 2 {
                    return Err(PatchError::Validate(":form expects 1 arg".to_string()));
                }
                let idx = term_to_usize(&items[1])?;
                out.push(PathStep::Form(idx));
            }
            ":pair-car" => out.push(PathStep::PairCar),
            ":pair-cdr" => out.push(PathStep::PairCdr),
            ":vec" => {
                if items.len() != 2 {
                    return Err(PatchError::Validate(":vec expects 1 arg".to_string()));
                }
                out.push(PathStep::Vec(term_to_usize(&items[1])?));
            }
            ":map" => {
                if items.len() != 2 {
                    return Err(PatchError::Validate(":map expects 1 arg".to_string()));
                }
                out.push(PathStep::Map(items[1].clone()));
            }
            other => return Err(PatchError::Validate(format!("unknown path step {other}"))),
        }
    }
    Ok(out)
}

pub(super) fn term_to_usize(t: &Term) -> Result<usize, PatchError> {
    match t {
        Term::Int(i) => i
            .to_usize()
            .ok_or_else(|| PatchError::Validate("index out of range".to_string())),
        _ => Err(PatchError::Validate("index must be int".to_string())),
    }
}

pub(super) fn get_int(m: &BTreeMap<TermOrdKey, Term>, k: &str) -> Result<Option<u64>, PatchError> {
    match m.get(&TermOrdKey(Term::Symbol(k.to_string()))) {
        None => Ok(None),
        Some(Term::Int(i)) => {
            Ok(Some(i.to_u64().ok_or_else(|| {
                PatchError::Validate(format!("{k} out of range"))
            })?))
        }
        Some(x) => Err(PatchError::Validate(format!(
            "{k} must be int, got {}",
            print_term(x)
        ))),
    }
}

pub(super) fn get_str(
    m: &BTreeMap<TermOrdKey, Term>,
    k: &str,
) -> Result<Option<String>, PatchError> {
    match m.get(&TermOrdKey(Term::Symbol(k.to_string()))) {
        None => Ok(None),
        Some(Term::Str(s)) => Ok(Some(s.clone())),
        Some(x) => Err(PatchError::Validate(format!(
            "{k} must be string, got {}",
            print_term(x)
        ))),
    }
}

pub(super) fn get_sym_vec(
    m: &BTreeMap<TermOrdKey, Term>,
    k: &str,
) -> Result<Vec<String>, PatchError> {
    match m.get(&TermOrdKey(Term::Symbol(k.to_string()))) {
        None => Ok(Vec::new()),
        Some(Term::Vector(xs)) => Ok(xs
            .iter()
            .filter_map(|t| match t {
                Term::Symbol(s) => Some(s.clone()),
                _ => None,
            })
            .collect()),
        Some(x) => Err(PatchError::Validate(format!(
            "{k} must be vector, got {}",
            print_term(x)
        ))),
    }
}
