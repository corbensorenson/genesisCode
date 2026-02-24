use super::*;
pub fn apply_patch_with_step_limit_and_frontend(
    patch_path: &Path,
    pkg_toml: &Path,
    caps_override: Option<&Path>,
    step_limit: StepLimit,
    mem_limits: MemLimits,
    frontend: CoreformFrontend,
) -> Result<PatchApplyResult, PatchError> {
    let patch_src = std::fs::read_to_string(patch_path)?;
    let patch_term = parse_term(&patch_src).map_err(|e| PatchError::Parse(e.to_string()))?;

    let mut selfhost = if coreform_frontend_is_rust(&frontend) {
        None
    } else {
        let CoreformFrontend::Selfhost(cfg) = &frontend else {
            return Err(PatchError::Validate(
                "invalid frontend dispatch while initializing patch toolchain".to_string(),
            ));
        };
        Some(SelfhostPatchToolchain::init(cfg, mem_limits)?)
    };

    // Selfhost frontend is authoritative for patch-schema acceptance.
    if let Some(sh) = selfhost.as_mut() {
        sh.validate_patch_term(&patch_term, step_limit)?;
    }

    let patch = Patch::from_term(&patch_term)?;
    if patch.version != 1 {
        return Err(PatchError::Validate(format!(
            "unsupported patch :version {}",
            patch.version
        )));
    }

    let (_manifest, pkg_dir) =
        PackageManifest::load(pkg_toml).map_err(|e| PatchError::Validate(format!("{e}")))?;
    let store = EvidenceStore::open(&pkg_dir)?;

    // Store the patch artifact itself (as canonical CoreForm bytes).
    let patch_artifact = store.put_term(&patch_term)?;

    // Apply ops.
    let mut semantic_edits = Vec::new();
    for op in &patch.ops {
        if let Some(edit) = apply_one_op(
            &pkg_dir,
            pkg_toml,
            op,
            &frontend,
            step_limit,
            mem_limits,
            selfhost.as_mut(),
        )? {
            semantic_edits.push(edit);
        }
    }

    // Re-pack to compute updated package artifact record and module hashes.
    let package_artifact = Some(pack(pkg_toml)?);

    // Re-run obligations using updated manifest.
    let acceptance = Some(test_package_with_step_limit_and_frontend(
        pkg_toml,
        caps_override,
        step_limit,
        mem_limits,
        frontend,
    )?);

    let ok = acceptance.as_ref().is_some_and(|r| r.ok);

    let report = report_term(
        &patch,
        ok,
        &package_artifact,
        acceptance.as_ref(),
        &semantic_edits,
    );
    let report_artifact = store.put_term(&report)?;

    Ok(PatchApplyResult {
        ok,
        patch_artifact,
        report_artifact,
        acceptance_artifact: acceptance.as_ref().map(|r| r.acceptance_artifact.clone()),
        package_artifact,
    })
}

fn report_term(
    patch: &Patch,
    ok: bool,
    package_artifact: &Option<String>,
    acceptance: Option<&PackageTestResult>,
    semantic_edits: &[AppliedSemanticEdit],
) -> Term {
    let mut m = BTreeMap::new();
    m.insert(
        TermOrdKey(Term::symbol(":kind")),
        Term::Str("genesis/patch-apply-v0.2".to_string()),
    );
    m.insert(TermOrdKey(Term::symbol(":ok")), Term::Bool(ok));
    m.insert(
        TermOrdKey(Term::symbol(":intent")),
        Term::Str(patch.intent.clone()),
    );
    m.insert(
        TermOrdKey(Term::symbol(":provenance")),
        patch.provenance.clone(),
    );
    m.insert(
        TermOrdKey(Term::symbol(":ops-count")),
        Term::Int((patch.ops.len() as i64).into()),
    );
    if let Some(p) = package_artifact {
        m.insert(
            TermOrdKey(Term::symbol(":package-artifact")),
            Term::Str(p.clone()),
        );
    }
    if let Some(a) = acceptance {
        m.insert(
            TermOrdKey(Term::symbol(":acceptance-artifact")),
            Term::Str(a.acceptance_artifact.clone()),
        );
    }
    if !semantic_edits.is_empty() {
        let mut edits = Vec::with_capacity(semantic_edits.len());
        for edit in semantic_edits {
            let mut em = BTreeMap::new();
            em.insert(
                TermOrdKey(Term::symbol(":op")),
                Term::Symbol(edit.op.to_string()),
            );
            em.insert(
                TermOrdKey(Term::symbol(":module-path")),
                Term::Str(edit.module_path.clone()),
            );
            if let Some(node_id) = &edit.node_id {
                em.insert(
                    TermOrdKey(Term::symbol(":node-id")),
                    Term::Str(node_id.clone()),
                );
            }
            if let Some(path) = &edit.path {
                em.insert(
                    TermOrdKey(Term::symbol(":path")),
                    path_steps_to_term(path).unwrap_or(Term::Vector(Vec::new())),
                );
            }
            if let Some(new_term_hash) = &edit.new_term_hash {
                em.insert(
                    TermOrdKey(Term::symbol(":new-term-h")),
                    Term::Str(new_term_hash.clone()),
                );
            }
            if let Some(before_h) = &edit.before_module_hash {
                em.insert(
                    TermOrdKey(Term::symbol(":before-module-h")),
                    Term::Str(before_h.clone()),
                );
            }
            if let Some(after_h) = &edit.after_module_hash {
                em.insert(
                    TermOrdKey(Term::symbol(":after-module-h")),
                    Term::Str(after_h.clone()),
                );
            }
            if let Some(detail) = &edit.detail {
                em.insert(TermOrdKey(Term::symbol(":detail")), detail.clone());
            }
            edits.push(Term::Map(em));
        }
        m.insert(
            TermOrdKey(Term::symbol(":semantic-edits")),
            Term::Vector(edits),
        );
    }
    Term::Map(m)
}

fn apply_one_op(
    pkg_dir: &Path,
    pkg_toml: &Path,
    op: &PatchOp,
    frontend: &CoreformFrontend,
    step_limit: StepLimit,
    mem_limits: MemLimits,
    mut selfhost: Option<&mut SelfhostPatchToolchain>,
) -> Result<Option<AppliedSemanticEdit>, PatchError> {
    match op {
        PatchOp::ReplaceNode {
            module_path,
            path,
            new_term,
        } => {
            let abs = pkg_dir.join(module_path);
            let src = std::fs::read_to_string(&abs)?;
            let forms = parse_canonicalize_module_src(&src, frontend, step_limit, mem_limits)?;

            if let Some(sh) = selfhost.as_mut() {
                let path_term = path_steps_to_term(path)?;
                let next_forms =
                    sh.apply_replace_node_term(&forms, &path_term, new_term, step_limit)?;
                let out = sh.print_module_forms_term(&next_forms, step_limit)?;
                std::fs::write(&abs, out)?;
            } else {
                let mut forms = forms;
                apply_replace(&mut forms, path, new_term.clone())?;
                let forms =
                    canonicalize_module(forms).map_err(|e| PatchError::Validate(e.to_string()))?;
                let out = print_module(&forms);
                std::fs::write(&abs, out)?;
            }
            Ok(Some(AppliedSemanticEdit {
                op: ":replace-node",
                module_path: module_path.clone(),
                node_id: Some(semantic_node_id(module_path, path)?),
                path: Some(path.clone()),
                new_term_hash: Some(hash32_hex(hash_term(new_term))),
                before_module_hash: None,
                after_module_hash: None,
                detail: None,
            }))
        }
        PatchOp::ReplaceNodeId {
            module_path,
            node_id,
            new_term,
        } => {
            let abs = pkg_dir.join(module_path);
            let src = std::fs::read_to_string(&abs)?;
            let forms = parse_canonicalize_module_src(&src, frontend, step_limit, mem_limits)?;
            let path = resolve_node_id_path(module_path, &forms, node_id)?;

            if let Some(sh) = selfhost.as_mut() {
                let path_term = path_steps_to_term(&path)?;
                let next_forms =
                    sh.apply_replace_node_term(&forms, &path_term, new_term, step_limit)?;
                let out = sh.print_module_forms_term(&next_forms, step_limit)?;
                std::fs::write(&abs, out)?;
            } else {
                let mut forms = forms;
                apply_replace(&mut forms, &path, new_term.clone())?;
                let forms =
                    canonicalize_module(forms).map_err(|e| PatchError::Validate(e.to_string()))?;
                let out = print_module(&forms);
                std::fs::write(&abs, out)?;
            }
            Ok(Some(AppliedSemanticEdit {
                op: ":replace-node-id",
                module_path: module_path.clone(),
                node_id: Some(node_id.clone()),
                path: Some(path),
                new_term_hash: Some(hash32_hex(hash_term(new_term))),
                before_module_hash: None,
                after_module_hash: None,
                detail: None,
            }))
        }
        PatchOp::AddModule {
            module_path,
            content,
        } => {
            let abs = pkg_dir.join(module_path);
            if abs.exists() {
                return Err(PatchError::Validate(format!(
                    "add-module target already exists: {}",
                    abs.display()
                )));
            }
            if let Some(parent) = abs.parent() {
                std::fs::create_dir_all(parent)?;
            }
            let out = if let Some(sh) = selfhost.as_mut() {
                let content_term = match content {
                    ModuleContent::Source(s) => Term::Str(s.clone()),
                    ModuleContent::Forms(fs) => Term::Vector(fs.clone()),
                };
                sh.print_module_from_content_term(&content_term, step_limit)?
            } else {
                let forms = match content {
                    ModuleContent::Source(s) => {
                        parse_canonicalize_module_src(s, frontend, step_limit, mem_limits)?
                    }
                    ModuleContent::Forms(fs) => canonicalize_module(fs.clone())
                        .map_err(|e| PatchError::Validate(e.to_string()))?,
                };
                print_module(&forms)
            };
            std::fs::write(&abs, out)?;

            // Update manifest modules list by appending; pack will pin hashes.
            let mut s = std::fs::read_to_string(pkg_toml)?;
            let v0: toml::Value =
                toml::from_str(&s).map_err(|e| PatchError::Parse(e.to_string()))?;
            let v = if let Some(sh) = selfhost.as_mut() {
                let manifest_term = toml_to_coreform(&v0)?;
                let out_term =
                    sh.manifest_apply_add_module_term(&manifest_term, module_path, step_limit)?;
                coreform_to_toml(&out_term)?
            } else {
                let mut v = v0;
                let mods = v
                    .get_mut("modules")
                    .and_then(|x| x.as_array_mut())
                    .ok_or_else(|| {
                        PatchError::Validate("manifest missing modules array".to_string())
                    })?;
                mods.push(toml::Value::Table(
                    [
                        ("path".to_string(), toml::Value::String(module_path.clone())),
                        ("hash".to_string(), toml::Value::String("".to_string())),
                    ]
                    .into_iter()
                    .collect(),
                ));
                v
            };
            s = toml::to_string_pretty(&v).map_err(|e| PatchError::Parse(e.to_string()))?;
            std::fs::write(pkg_toml, s)?;
            Ok(None)
        }
        PatchOp::RemoveModule { module_path } => {
            let abs = pkg_dir.join(module_path);
            if abs.exists() {
                std::fs::remove_file(&abs)?;
            }
            // Remove from manifest modules array.
            let mut s = std::fs::read_to_string(pkg_toml)?;
            let v0: toml::Value =
                toml::from_str(&s).map_err(|e| PatchError::Parse(e.to_string()))?;
            let v = if let Some(sh) = selfhost.as_mut() {
                let manifest_term = toml_to_coreform(&v0)?;
                let out_term =
                    sh.manifest_apply_remove_module_term(&manifest_term, module_path, step_limit)?;
                coreform_to_toml(&out_term)?
            } else {
                let mut v = v0;
                let mods = v
                    .get_mut("modules")
                    .and_then(|x| x.as_array_mut())
                    .ok_or_else(|| {
                        PatchError::Validate("manifest missing modules array".to_string())
                    })?;
                mods.retain(|m| {
                    m.get("path").and_then(|p| p.as_str()) != Some(module_path.as_str())
                });
                v
            };
            s = toml::to_string_pretty(&v).map_err(|e| PatchError::Parse(e.to_string()))?;
            std::fs::write(pkg_toml, s)?;
            Ok(None)
        }
        PatchOp::UpdateManifest {
            set,
            obligations_add,
            obligations_remove,
            tests_add,
            tests_remove,
            caps_policy,
        } => {
            let mut s = std::fs::read_to_string(pkg_toml)?;
            let v0: toml::Value =
                toml::from_str(&s).map_err(|e| PatchError::Parse(e.to_string()))?;
            let v = if let Some(sh) = selfhost {
                let manifest_term = toml_to_coreform(&v0)?;
                let op_term = update_manifest_op_to_term(
                    set.as_ref(),
                    obligations_add,
                    obligations_remove,
                    tests_add,
                    tests_remove,
                    caps_policy.as_deref(),
                )?;
                let out_term = sh.manifest_apply_update_manifest_op_term(
                    &manifest_term,
                    &op_term,
                    step_limit,
                )?;
                coreform_to_toml(&out_term)?
            } else {
                let mut v = v0;
                if let Some(set) = set {
                    apply_manifest_set(&mut v, set)?;
                }
                if !obligations_add.is_empty() || !obligations_remove.is_empty() {
                    patch_string_vec_field(
                        &mut v,
                        "obligations",
                        obligations_add,
                        obligations_remove,
                    )?;
                }
                if !tests_add.is_empty() || !tests_remove.is_empty() {
                    patch_string_vec_field(&mut v, "tests", tests_add, tests_remove)?;
                }
                if let Some(p) = caps_policy {
                    v.as_table_mut()
                        .ok_or_else(|| {
                            PatchError::Validate("manifest must be a table".to_string())
                        })?
                        .insert("caps_policy".to_string(), toml::Value::String(p.clone()));
                }
                v
            };
            s = toml::to_string_pretty(&v).map_err(|e| PatchError::Parse(e.to_string()))?;
            std::fs::write(pkg_toml, s)?;
            Ok(None)
        }
        PatchOp::RenameSymbol {
            module_path,
            from,
            to,
        } => {
            let abs = pkg_dir.join(module_path);
            let src = std::fs::read_to_string(&abs)?;
            let forms = parse_canonicalize_module_src(&src, frontend, step_limit, mem_limits)?;
            let before_module_hash = hash32_hex(hash_module(&forms));
            let (next, rewrites) = if let Some(sh) = selfhost.as_mut() {
                sh.rename_symbol_forms_term(&forms, from, to, step_limit)?
            } else {
                let (next, rewrites) = patch_refactor::rename_symbol_in_forms(forms, from, to)?;
                let next =
                    canonicalize_module(next).map_err(|e| PatchError::Validate(e.to_string()))?;
                (next, rewrites)
            };
            if rewrites == 0 {
                return Err(PatchError::Validate(format!(
                    "rename-symbol found no `{from}` references in {module_path}"
                )));
            }
            let after_module_hash = hash32_hex(hash_module(&next));
            if let Some(sh) = selfhost.as_mut() {
                let out = sh.print_module_forms_term(&next, step_limit)?;
                std::fs::write(&abs, out)?;
            } else {
                std::fs::write(&abs, print_module(&next))?;
            }
            let mut detail = BTreeMap::new();
            detail.insert(
                TermOrdKey(Term::symbol(":from")),
                Term::symbol(from.clone()),
            );
            detail.insert(TermOrdKey(Term::symbol(":to")), Term::symbol(to.clone()));
            detail.insert(
                TermOrdKey(Term::symbol(":rewrite-count")),
                Term::Int((rewrites as i64).into()),
            );
            Ok(Some(AppliedSemanticEdit {
                op: ":rename-symbol",
                module_path: module_path.clone(),
                node_id: None,
                path: None,
                new_term_hash: None,
                before_module_hash: Some(before_module_hash),
                after_module_hash: Some(after_module_hash),
                detail: Some(Term::Map(detail)),
            }))
        }
        PatchOp::MoveModule {
            from_module_path,
            to_module_path,
        } => {
            let from_abs = pkg_dir.join(from_module_path);
            let to_abs = pkg_dir.join(to_module_path);
            if !from_abs.exists() {
                return Err(PatchError::Validate(format!(
                    "move-module source does not exist: {}",
                    from_abs.display()
                )));
            }
            if to_abs.exists() {
                return Err(PatchError::Validate(format!(
                    "move-module target already exists: {}",
                    to_abs.display()
                )));
            }
            let src = std::fs::read_to_string(&from_abs)?;
            let forms = parse_canonicalize_module_src(&src, frontend, step_limit, mem_limits)?;
            let module_hash = hash32_hex(hash_module(&forms));
            if let Some(parent) = to_abs.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::rename(&from_abs, &to_abs)?;
            if let Some(sh) = selfhost.as_mut() {
                let mut s = std::fs::read_to_string(pkg_toml)?;
                let v0: toml::Value =
                    toml::from_str(&s).map_err(|e| PatchError::Parse(e.to_string()))?;
                let manifest_term = toml_to_coreform(&v0)?;
                let out_term = sh.manifest_apply_move_module_term(
                    &manifest_term,
                    from_module_path,
                    to_module_path,
                    step_limit,
                )?;
                let v = coreform_to_toml(&out_term)?;
                s = toml::to_string_pretty(&v).map_err(|e| PatchError::Parse(e.to_string()))?;
                std::fs::write(pkg_toml, s)?;
            } else {
                patch_manifest_move_module_path(pkg_toml, from_module_path, to_module_path)?;
            }
            let mut detail = BTreeMap::new();
            detail.insert(
                TermOrdKey(Term::symbol(":from-module-path")),
                Term::Str(from_module_path.clone()),
            );
            detail.insert(
                TermOrdKey(Term::symbol(":to-module-path")),
                Term::Str(to_module_path.clone()),
            );
            Ok(Some(AppliedSemanticEdit {
                op: ":move-module",
                module_path: to_module_path.clone(),
                node_id: None,
                path: None,
                new_term_hash: None,
                before_module_hash: Some(module_hash.clone()),
                after_module_hash: Some(module_hash),
                detail: Some(Term::Map(detail)),
            }))
        }
        PatchOp::SplitModule {
            from_module_path,
            to_module_path,
            symbols,
        } => {
            let from_abs = pkg_dir.join(from_module_path);
            let to_abs = pkg_dir.join(to_module_path);
            if !from_abs.exists() {
                return Err(PatchError::Validate(format!(
                    "split-module source does not exist: {}",
                    from_abs.display()
                )));
            }
            if to_abs.exists() {
                return Err(PatchError::Validate(format!(
                    "split-module target already exists: {}",
                    to_abs.display()
                )));
            }
            let src = std::fs::read_to_string(&from_abs)?;
            let forms = parse_canonicalize_module_src(&src, frontend, step_limit, mem_limits)?;
            let before_module_hash = hash32_hex(hash_module(&forms));
            let (keep, extracted, moved) = if let Some(sh) = selfhost.as_mut() {
                sh.split_module_forms_term(&forms, symbols, step_limit)?
            } else {
                let symbol_set = symbols
                    .iter()
                    .cloned()
                    .collect::<std::collections::BTreeSet<_>>();
                let (keep, extracted, moved) =
                    patch_refactor::split_module_forms(forms, &symbol_set)?;
                let keep =
                    canonicalize_module(keep).map_err(|e| PatchError::Validate(e.to_string()))?;
                let extracted = canonicalize_module(extracted)
                    .map_err(|e| PatchError::Validate(e.to_string()))?;
                (keep, extracted, moved)
            };
            let after_module_hash = hash32_hex(hash_module(&keep));
            let extracted_module_hash = hash32_hex(hash_module(&extracted));
            if let Some(parent) = to_abs.parent() {
                std::fs::create_dir_all(parent)?;
            }
            if let Some(sh) = selfhost.as_mut() {
                let keep_out = sh.print_module_forms_term(&keep, step_limit)?;
                std::fs::write(&from_abs, keep_out)?;
                let extracted_out = sh.print_module_forms_term(&extracted, step_limit)?;
                std::fs::write(&to_abs, extracted_out)?;
            } else {
                std::fs::write(&from_abs, print_module(&keep))?;
                std::fs::write(&to_abs, print_module(&extracted))?;
            }
            patch_manifest_add_module_path(pkg_toml, to_module_path)?;
            let mut detail = BTreeMap::new();
            detail.insert(
                TermOrdKey(Term::symbol(":to-module-path")),
                Term::Str(to_module_path.clone()),
            );
            detail.insert(
                TermOrdKey(Term::symbol(":moved-def-count")),
                Term::Int((moved as i64).into()),
            );
            detail.insert(
                TermOrdKey(Term::symbol(":extracted-module-h")),
                Term::Str(extracted_module_hash),
            );
            detail.insert(
                TermOrdKey(Term::symbol(":symbols")),
                Term::Vector(symbols.iter().cloned().map(Term::symbol).collect()),
            );
            Ok(Some(AppliedSemanticEdit {
                op: ":split-module",
                module_path: from_module_path.clone(),
                node_id: None,
                path: None,
                new_term_hash: None,
                before_module_hash: Some(before_module_hash),
                after_module_hash: Some(after_module_hash),
                detail: Some(Term::Map(detail)),
            }))
        }
        PatchOp::RewriteMetaList {
            module_path,
            field,
            add,
            remove,
            replace,
        } => {
            let abs = pkg_dir.join(module_path);
            let src = std::fs::read_to_string(&abs)?;
            let forms = parse_canonicalize_module_src(&src, frontend, step_limit, mem_limits)?;
            let before_module_hash = hash32_hex(hash_module(&forms));
            let (next, changed_entries) = if let Some(sh) = selfhost.as_mut() {
                sh.rewrite_meta_list_forms_term(
                    &forms,
                    field.key_symbol(),
                    add,
                    remove,
                    replace.as_deref(),
                    step_limit,
                )?
            } else {
                let (next, changed_entries) = patch_refactor::rewrite_meta_list(
                    forms,
                    *field,
                    add,
                    remove,
                    replace.as_deref(),
                )?;
                let next =
                    canonicalize_module(next).map_err(|e| PatchError::Validate(e.to_string()))?;
                (next, changed_entries)
            };
            let after_module_hash = hash32_hex(hash_module(&next));
            if let Some(sh) = selfhost.as_mut() {
                let out = sh.print_module_forms_term(&next, step_limit)?;
                std::fs::write(&abs, out)?;
            } else {
                std::fs::write(&abs, print_module(&next))?;
            }
            let mut detail = BTreeMap::new();
            detail.insert(
                TermOrdKey(Term::symbol(":field")),
                Term::symbol(field.key_symbol()),
            );
            detail.insert(
                TermOrdKey(Term::symbol(":changed-entries")),
                Term::Int((changed_entries as i64).into()),
            );
            if !add.is_empty() {
                detail.insert(
                    TermOrdKey(Term::symbol(":add")),
                    Term::Vector(add.iter().cloned().map(Term::symbol).collect()),
                );
            }
            if !remove.is_empty() {
                detail.insert(
                    TermOrdKey(Term::symbol(":remove")),
                    Term::Vector(remove.iter().cloned().map(Term::symbol).collect()),
                );
            }
            if let Some(replace) = replace {
                detail.insert(
                    TermOrdKey(Term::symbol(":replace")),
                    Term::Vector(replace.iter().cloned().map(Term::symbol).collect()),
                );
            }
            Ok(Some(AppliedSemanticEdit {
                op: field.op_symbol(),
                module_path: module_path.clone(),
                node_id: None,
                path: None,
                new_term_hash: None,
                before_module_hash: Some(before_module_hash),
                after_module_hash: Some(after_module_hash),
                detail: Some(Term::Map(detail)),
            }))
        }
        PatchOp::MigrateContractSignature {
            module_path,
            contract_symbol,
            from_param,
            to_param,
        } => {
            let abs = pkg_dir.join(module_path);
            let src = std::fs::read_to_string(&abs)?;
            let forms = parse_canonicalize_module_src(&src, frontend, step_limit, mem_limits)?;
            let before_module_hash = hash32_hex(hash_module(&forms));
            let (next, changed_entries) = if let Some(sh) = selfhost.as_mut() {
                sh.migrate_contract_signature_forms_term(
                    &forms,
                    contract_symbol,
                    from_param,
                    to_param,
                    step_limit,
                )?
            } else {
                let (next, changed_entries) = patch_refactor::migrate_contract_signature(
                    forms,
                    contract_symbol,
                    from_param,
                    to_param,
                )?;
                let next =
                    canonicalize_module(next).map_err(|e| PatchError::Validate(e.to_string()))?;
                (next, changed_entries)
            };
            let after_module_hash = hash32_hex(hash_module(&next));
            if let Some(sh) = selfhost.as_mut() {
                let out = sh.print_module_forms_term(&next, step_limit)?;
                std::fs::write(&abs, out)?;
            } else {
                std::fs::write(&abs, print_module(&next))?;
            }
            let mut detail = BTreeMap::new();
            detail.insert(
                TermOrdKey(Term::symbol(":contract-symbol")),
                Term::symbol(contract_symbol.clone()),
            );
            detail.insert(
                TermOrdKey(Term::symbol(":from-param")),
                Term::symbol(from_param.clone()),
            );
            detail.insert(
                TermOrdKey(Term::symbol(":to-param")),
                Term::symbol(to_param.clone()),
            );
            detail.insert(
                TermOrdKey(Term::symbol(":changed-entries")),
                Term::Int((changed_entries as i64).into()),
            );
            Ok(Some(AppliedSemanticEdit {
                op: ":migrate-contract-signature",
                module_path: module_path.clone(),
                node_id: None,
                path: None,
                new_term_hash: None,
                before_module_hash: Some(before_module_hash),
                after_module_hash: Some(after_module_hash),
                detail: Some(Term::Map(detail)),
            }))
        }
    }
}
