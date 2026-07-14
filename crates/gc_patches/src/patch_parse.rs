use super::*;

impl Patch {
    pub(super) fn from_term(t: &Term) -> Result<Self, PatchError> {
        let Term::Map(m) = t else {
            return Err(PatchError::Validate("patch must be a map".to_string()));
        };
        let version = get_int(m, ":version")?
            .ok_or_else(|| PatchError::Validate("missing :version".to_string()))?;
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
        ":rename-symbol" => {
            let module_path = get_str(m, ":module-path")?.ok_or_else(|| {
                PatchError::Validate("rename-symbol missing :module-path".to_string())
            })?;
            let from = req_sym_or_str(m, ":from", "rename-symbol")?;
            let to = req_sym_or_str(m, ":to", "rename-symbol")?;
            if from.trim().is_empty() || to.trim().is_empty() {
                return Err(PatchError::Validate(
                    "rename-symbol :from/:to must be non-empty".to_string(),
                ));
            }
            Ok(PatchOp::RenameSymbol {
                module_path,
                from,
                to,
            })
        }
        ":move-module" => {
            let from_module_path = get_str(m, ":from-module-path")?.ok_or_else(|| {
                PatchError::Validate("move-module missing :from-module-path".to_string())
            })?;
            let to_module_path = get_str(m, ":to-module-path")?.ok_or_else(|| {
                PatchError::Validate("move-module missing :to-module-path".to_string())
            })?;
            Ok(PatchOp::MoveModule {
                from_module_path,
                to_module_path,
            })
        }
        ":split-module" => {
            let from_module_path = get_str(m, ":from-module-path")?.ok_or_else(|| {
                PatchError::Validate("split-module missing :from-module-path".to_string())
            })?;
            let to_module_path = get_str(m, ":to-module-path")?.ok_or_else(|| {
                PatchError::Validate("split-module missing :to-module-path".to_string())
            })?;
            let symbols = get_sym_or_str_vec(m, ":symbols")?;
            if symbols.is_empty() {
                return Err(PatchError::Validate(
                    "split-module :symbols must be non-empty vector".to_string(),
                ));
            }
            Ok(PatchOp::SplitModule {
                from_module_path,
                to_module_path,
                symbols,
            })
        }
        ":rewrite-imports" | ":rewrite-exports" => {
            let module_path = get_str(m, ":module-path")?
                .ok_or_else(|| PatchError::Validate(format!("{op} missing :module-path")))?;
            let add = get_sym_or_str_vec(m, ":add")?;
            let remove = get_sym_or_str_vec(m, ":remove")?;
            let replace = get_optional_sym_or_str_vec(m, ":replace")?;
            let field = if op == ":rewrite-imports" {
                MetaListField::Imports
            } else {
                MetaListField::Exports
            };
            Ok(PatchOp::RewriteMetaList {
                module_path,
                field,
                add,
                remove,
                replace,
            })
        }
        ":migrate-contract-signature" => {
            let module_path = get_str(m, ":module-path")?.ok_or_else(|| {
                PatchError::Validate("migrate-contract-signature missing :module-path".to_string())
            })?;
            let contract_symbol =
                req_sym_or_str(m, ":contract-symbol", "migrate-contract-signature")?;
            let from_param = req_sym_or_str(m, ":from-param", "migrate-contract-signature")?;
            let to_param = req_sym_or_str(m, ":to-param", "migrate-contract-signature")?;
            Ok(PatchOp::MigrateContractSignature {
                module_path,
                contract_symbol,
                from_param,
                to_param,
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

pub(super) fn get_sym_or_str(
    m: &BTreeMap<TermOrdKey, Term>,
    k: &str,
) -> Result<Option<String>, PatchError> {
    match m.get(&TermOrdKey(Term::Symbol(k.to_string()))) {
        None => Ok(None),
        Some(Term::Symbol(s)) => Ok(Some(s.clone())),
        Some(Term::Str(s)) => Ok(Some(s.clone())),
        Some(x) => Err(PatchError::Validate(format!(
            "{k} must be symbol|string, got {}",
            print_term(x)
        ))),
    }
}

pub(super) fn req_sym_or_str(
    m: &BTreeMap<TermOrdKey, Term>,
    k: &str,
    op: &str,
) -> Result<String, PatchError> {
    get_sym_or_str(m, k)?.ok_or_else(|| PatchError::Validate(format!("{op} missing {k}")))
}

pub(super) fn get_sym_or_str_vec(
    m: &BTreeMap<TermOrdKey, Term>,
    k: &str,
) -> Result<Vec<String>, PatchError> {
    match m.get(&TermOrdKey(Term::Symbol(k.to_string()))) {
        None => Ok(Vec::new()),
        Some(Term::Vector(xs)) => xs
            .iter()
            .map(|t| match t {
                Term::Symbol(s) => Ok(s.clone()),
                Term::Str(s) => Ok(s.clone()),
                other => Err(PatchError::Validate(format!(
                    "{k} entries must be symbol|string, got {}",
                    print_term(other)
                ))),
            })
            .collect(),
        Some(x) => Err(PatchError::Validate(format!(
            "{k} must be vector, got {}",
            print_term(x)
        ))),
    }
}

pub(super) fn get_optional_sym_or_str_vec(
    m: &BTreeMap<TermOrdKey, Term>,
    k: &str,
) -> Result<Option<Vec<String>>, PatchError> {
    if m.contains_key(&TermOrdKey(Term::Symbol(k.to_string()))) {
        return Ok(Some(get_sym_or_str_vec(m, k)?));
    }
    Ok(None)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn patch_term(version: Option<u64>) -> Term {
        let mut m = BTreeMap::from([
            (
                TermOrdKey(Term::Symbol(":intent".to_string())),
                Term::Str(String::new()),
            ),
            (
                TermOrdKey(Term::Symbol(":provenance".to_string())),
                Term::Map(BTreeMap::new()),
            ),
            (
                TermOrdKey(Term::Symbol(":ops".to_string())),
                Term::Vector(Vec::new()),
            ),
        ]);
        if let Some(version) = version {
            m.insert(
                TermOrdKey(Term::Symbol(":version".to_string())),
                Term::Int(version.into()),
            );
        }
        Term::Map(m)
    }

    #[test]
    fn semantic_patch_requires_explicit_version() {
        let err = Patch::from_term(&patch_term(None)).expect_err("missing version must fail");
        assert!(err.to_string().contains("missing :version"));
    }

    #[test]
    fn semantic_patch_accepts_current_version() {
        let patch = Patch::from_term(&patch_term(Some(SEMANTIC_PATCH_VERSION)))
            .expect("current version must parse");
        assert_eq!(patch.version, SEMANTIC_PATCH_VERSION);
    }

    #[test]
    fn semantic_patch_preserves_future_version_for_apply_rejection() {
        let future = SEMANTIC_PATCH_VERSION + 1;
        let patch = Patch::from_term(&patch_term(Some(future)))
            .expect("schema parsing must preserve explicit future versions");
        assert_eq!(patch.version, future);
    }
}
