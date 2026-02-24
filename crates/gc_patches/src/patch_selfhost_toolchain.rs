use super::*;

pub(super) struct SelfhostPatchToolchain {
    ctx: EvalCtx,
    error_token: SealId,
    validate_patch: Value,
    apply_replace_node: Value,
    print_module_forms: Value,
    print_module_from_content: Value,
    manifest_apply_add_module: Value,
    manifest_apply_remove_module: Value,
    manifest_apply_move_module: Value,
    manifest_apply_update_manifest_op: Value,
    rename_symbol_forms: Value,
    split_module_forms: Value,
    rewrite_meta_list_forms: Value,
    migrate_contract_signature_forms: Value,
}

fn summarize_protocol_error_payload(payload: &Value) -> String {
    let Some(t) = payload.as_data() else {
        return payload.debug_repr();
    };
    match t {
        Term::Map(m) => {
            let code = m
                .get(&TermOrdKey(Term::symbol(":error/code")))
                .and_then(|t| match t {
                    Term::Str(s) => Some(s.as_str()),
                    _ => None,
                })
                .unwrap_or("core/error");
            let msg = m
                .get(&TermOrdKey(Term::symbol(":error/message")))
                .and_then(|t| match t {
                    Term::Str(s) => Some(s.as_str()),
                    _ => None,
                })
                .unwrap_or("error");
            format!("{code}: {msg}")
        }
        _ => print_term(t),
    }
}

fn extract_protocol_error(out: &Value, error_token: SealId) -> Option<String> {
    match out {
        Value::Sealed { token, payload } if *token == error_token => {
            Some(summarize_protocol_error_payload(payload))
        }
        _ => None,
    }
}

impl SelfhostPatchToolchain {
    pub(super) fn init(
        cfg: &gc_obligations::SelfhostFrontendConfig,
        mem_limits: MemLimits,
    ) -> Result<Self, PatchError> {
        // Toolchain bootstrap is trusted and therefore uncharged.
        let mut ctx = EvalCtx::with_step_limit(None);
        ctx.set_mem_limits(mem_limits);
        let prelude = build_prelude(&mut ctx);
        let error_token = prelude.protocol.error;
        let mut env = prelude.env;
        load_selfhost_coreform_toolchain_v1_with_mode(
            &mut ctx,
            &mut env,
            cfg.bootstrap_mode,
            cfg.artifact.as_deref(),
        )
        .map_err(|e| PatchError::Validate(format!("selfhost/init: {e}")))?;

        let validate_patch = env.get("core/cli::validate-patch").ok_or_else(|| {
            PatchError::Validate("missing binding core/cli::validate-patch".to_string())
        })?;
        let apply_replace_node = env.get("core/cli::apply-replace-node").ok_or_else(|| {
            PatchError::Validate("missing binding core/cli::apply-replace-node".to_string())
        })?;
        let print_module_forms = env.get("core/cli::print-module-forms").ok_or_else(|| {
            PatchError::Validate("missing binding core/cli::print-module-forms".to_string())
        })?;
        let print_module_from_content =
            env.get("core/cli::print-module-from-content")
                .ok_or_else(|| {
                    PatchError::Validate(
                        "missing binding core/cli::print-module-from-content".to_string(),
                    )
                })?;
        let manifest_apply_add_module =
            env.get("core/cli::manifest-apply-add-module")
                .ok_or_else(|| {
                    PatchError::Validate(
                        "missing binding core/cli::manifest-apply-add-module".to_string(),
                    )
                })?;
        let manifest_apply_remove_module = env
            .get("core/cli::manifest-apply-remove-module")
            .ok_or_else(|| {
                PatchError::Validate(
                    "missing binding core/cli::manifest-apply-remove-module".to_string(),
                )
            })?;
        let manifest_apply_move_module = env
            .get("core/cli::manifest-apply-move-module")
            .ok_or_else(|| {
                PatchError::Validate(
                    "missing binding core/cli::manifest-apply-move-module".to_string(),
                )
            })?;
        let manifest_apply_update_manifest_op = env
            .get("core/cli::manifest-apply-update-manifest-op")
            .ok_or_else(|| {
                PatchError::Validate(
                    "missing binding core/cli::manifest-apply-update-manifest-op".to_string(),
                )
            })?;
        let rename_symbol_forms = env.get("core/cli::rename-symbol-forms").ok_or_else(|| {
            PatchError::Validate("missing binding core/cli::rename-symbol-forms".to_string())
        })?;
        let split_module_forms = env.get("core/cli::split-module-forms").ok_or_else(|| {
            PatchError::Validate("missing binding core/cli::split-module-forms".to_string())
        })?;
        let rewrite_meta_list_forms =
            env.get("core/cli::rewrite-meta-list-forms")
                .ok_or_else(|| {
                    PatchError::Validate(
                        "missing binding core/cli::rewrite-meta-list-forms".to_string(),
                    )
                })?;
        let migrate_contract_signature_forms = env
            .get("core/cli::migrate-contract-signature-forms")
            .ok_or_else(|| {
                PatchError::Validate(
                    "missing binding core/cli::migrate-contract-signature-forms".to_string(),
                )
            })?;
        Ok(SelfhostPatchToolchain {
            ctx,
            error_token,
            validate_patch,
            apply_replace_node,
            print_module_forms,
            print_module_from_content,
            manifest_apply_add_module,
            manifest_apply_remove_module,
            manifest_apply_move_module,
            manifest_apply_update_manifest_op,
            rename_symbol_forms,
            split_module_forms,
            rewrite_meta_list_forms,
            migrate_contract_signature_forms,
        })
    }

    fn with_limits(&mut self, step_limit: StepLimit) {
        self.ctx.steps = 0;
        self.ctx.step_limit = step_limit.resolve();
    }

    pub(super) fn validate_patch_term(
        &mut self,
        patch_term: &Term,
        step_limit: StepLimit,
    ) -> Result<(), PatchError> {
        self.with_limits(step_limit);
        let out = self
            .validate_patch
            .clone()
            .apply(&mut self.ctx, Value::Data(patch_term.clone()))
            .map_err(|e| PatchError::Validate(format!("selfhost validate-patch apply: {e}")))?;
        if let Some(e) = extract_protocol_error(&out, self.error_token) {
            return Err(PatchError::Validate(format!(
                "selfhost core/cli validate-patch failed: {e}"
            )));
        }
        Ok(())
    }

    pub(super) fn apply_replace_node_term(
        &mut self,
        forms: &[Term],
        path: &Term,
        new_term: &Term,
        step_limit: StepLimit,
    ) -> Result<Vec<Term>, PatchError> {
        self.with_limits(step_limit);
        let mut req = BTreeMap::new();
        req.insert(
            TermOrdKey(Term::symbol(":forms")),
            Term::Vector(forms.to_vec()),
        );
        req.insert(TermOrdKey(Term::symbol(":path")), path.clone());
        req.insert(TermOrdKey(Term::symbol(":new")), new_term.clone());
        let req = Term::Map(req);

        let out = self
            .apply_replace_node
            .clone()
            .apply(&mut self.ctx, Value::Data(req))
            .map_err(|e| PatchError::Validate(format!("selfhost apply-replace-node apply: {e}")))?;
        if let Some(e) = extract_protocol_error(&out, self.error_token) {
            return Err(PatchError::Validate(format!(
                "selfhost core/cli apply-replace-node failed: {e}"
            )));
        }
        let Value::Data(Term::Vector(forms)) = out else {
            return Err(PatchError::Validate(format!(
                "selfhost core/cli apply-replace-node must return vector of forms, got {}",
                out.debug_repr()
            )));
        };
        Ok(forms)
    }

    pub(super) fn print_module_forms_term(
        &mut self,
        forms: &[Term],
        step_limit: StepLimit,
    ) -> Result<String, PatchError> {
        self.with_limits(step_limit);
        let out = self
            .print_module_forms
            .clone()
            .apply(&mut self.ctx, Value::Data(Term::Vector(forms.to_vec())))
            .map_err(|e| PatchError::Validate(format!("selfhost print-module-forms apply: {e}")))?;
        if let Some(e) = extract_protocol_error(&out, self.error_token) {
            return Err(PatchError::Validate(format!(
                "selfhost core/cli print-module-forms failed: {e}"
            )));
        }
        let Value::Data(Term::Str(s)) = out else {
            return Err(PatchError::Validate(format!(
                "selfhost core/cli print-module-forms must return string, got {}",
                out.debug_repr()
            )));
        };
        Ok(s)
    }

    pub(super) fn print_module_from_content_term(
        &mut self,
        content: &Term,
        step_limit: StepLimit,
    ) -> Result<String, PatchError> {
        self.with_limits(step_limit);
        let out = self
            .print_module_from_content
            .clone()
            .apply(&mut self.ctx, Value::Data(content.clone()))
            .map_err(|e| {
                PatchError::Validate(format!("selfhost print-module-from-content apply: {e}"))
            })?;
        if let Some(e) = extract_protocol_error(&out, self.error_token) {
            return Err(PatchError::Validate(format!(
                "selfhost core/cli print-module-from-content failed: {e}"
            )));
        }
        let Value::Data(Term::Str(s)) = out else {
            return Err(PatchError::Validate(format!(
                "selfhost core/cli print-module-from-content must return string, got {}",
                out.debug_repr()
            )));
        };
        Ok(s)
    }

    pub(super) fn manifest_apply_add_module_term(
        &mut self,
        manifest: &Term,
        module_path: &str,
        step_limit: StepLimit,
    ) -> Result<Term, PatchError> {
        self.with_limits(step_limit);
        let out = self
            .manifest_apply_add_module
            .clone()
            .apply(&mut self.ctx, Value::Data(manifest.clone()))
            .and_then(|f| {
                f.apply(
                    &mut self.ctx,
                    Value::Data(Term::Str(module_path.to_string())),
                )
            })
            .map_err(|e| {
                PatchError::Validate(format!("selfhost manifest-apply-add-module apply: {e}"))
            })?;
        if let Some(e) = extract_protocol_error(&out, self.error_token) {
            return Err(PatchError::Validate(format!(
                "selfhost core/cli manifest-apply-add-module failed: {e}"
            )));
        }
        let Value::Data(t) = out else {
            return Err(PatchError::Validate(format!(
                "selfhost core/cli manifest-apply-add-module must return data term, got {}",
                out.debug_repr()
            )));
        };
        Ok(t)
    }

    pub(super) fn manifest_apply_remove_module_term(
        &mut self,
        manifest: &Term,
        module_path: &str,
        step_limit: StepLimit,
    ) -> Result<Term, PatchError> {
        self.with_limits(step_limit);
        let out = self
            .manifest_apply_remove_module
            .clone()
            .apply(&mut self.ctx, Value::Data(manifest.clone()))
            .and_then(|f| {
                f.apply(
                    &mut self.ctx,
                    Value::Data(Term::Str(module_path.to_string())),
                )
            })
            .map_err(|e| {
                PatchError::Validate(format!("selfhost manifest-apply-remove-module apply: {e}"))
            })?;
        if let Some(e) = extract_protocol_error(&out, self.error_token) {
            return Err(PatchError::Validate(format!(
                "selfhost core/cli manifest-apply-remove-module failed: {e}"
            )));
        }
        let Value::Data(t) = out else {
            return Err(PatchError::Validate(format!(
                "selfhost core/cli manifest-apply-remove-module must return data term, got {}",
                out.debug_repr()
            )));
        };
        Ok(t)
    }

    pub(super) fn manifest_apply_move_module_term(
        &mut self,
        manifest: &Term,
        from_module_path: &str,
        to_module_path: &str,
        step_limit: StepLimit,
    ) -> Result<Term, PatchError> {
        self.with_limits(step_limit);
        let out = self
            .manifest_apply_move_module
            .clone()
            .apply(&mut self.ctx, Value::Data(manifest.clone()))
            .and_then(|f| {
                f.apply(
                    &mut self.ctx,
                    Value::Data(Term::Str(from_module_path.to_string())),
                )
            })
            .and_then(|f| {
                f.apply(
                    &mut self.ctx,
                    Value::Data(Term::Str(to_module_path.to_string())),
                )
            })
            .map_err(|e| {
                PatchError::Validate(format!("selfhost manifest-apply-move-module apply: {e}"))
            })?;
        if let Some(e) = extract_protocol_error(&out, self.error_token) {
            return Err(PatchError::Validate(format!(
                "selfhost core/cli manifest-apply-move-module failed: {e}"
            )));
        }
        let Value::Data(t) = out else {
            return Err(PatchError::Validate(format!(
                "selfhost core/cli manifest-apply-move-module must return data term, got {}",
                out.debug_repr()
            )));
        };
        Ok(t)
    }

    pub(super) fn manifest_apply_update_manifest_op_term(
        &mut self,
        manifest: &Term,
        op: &Term,
        step_limit: StepLimit,
    ) -> Result<Term, PatchError> {
        self.with_limits(step_limit);
        let out = self
            .manifest_apply_update_manifest_op
            .clone()
            .apply(&mut self.ctx, Value::Data(manifest.clone()))
            .and_then(|f| f.apply(&mut self.ctx, Value::Data(op.clone())))
            .map_err(|e| {
                PatchError::Validate(format!(
                    "selfhost manifest-apply-update-manifest-op apply: {e}"
                ))
            })?;
        if let Some(e) = extract_protocol_error(&out, self.error_token) {
            return Err(PatchError::Validate(format!(
                "selfhost core/cli manifest-apply-update-manifest-op failed: {e}"
            )));
        }
        let Value::Data(t) = out else {
            return Err(PatchError::Validate(format!(
                "selfhost core/cli manifest-apply-update-manifest-op must return data term, got {}",
                out.debug_repr()
            )));
        };
        Ok(t)
    }

    pub(super) fn rename_symbol_forms_term(
        &mut self,
        forms: &[Term],
        from: &str,
        to: &str,
        step_limit: StepLimit,
    ) -> Result<(Vec<Term>, usize), PatchError> {
        self.with_limits(step_limit);
        let req = Term::Map(
            [
                (
                    TermOrdKey(Term::symbol(":forms")),
                    Term::Vector(forms.to_vec()),
                ),
                (
                    TermOrdKey(Term::symbol(":from")),
                    Term::Symbol(from.to_string()),
                ),
                (
                    TermOrdKey(Term::symbol(":to")),
                    Term::Symbol(to.to_string()),
                ),
            ]
            .into_iter()
            .collect(),
        );
        let out = self
            .rename_symbol_forms
            .clone()
            .apply(&mut self.ctx, Value::Data(req))
            .map_err(|e| {
                PatchError::Validate(format!("selfhost rename-symbol-forms apply: {e}"))
            })?;
        if let Some(e) = extract_protocol_error(&out, self.error_token) {
            return Err(PatchError::Validate(format!(
                "selfhost core/cli rename-symbol-forms failed: {e}"
            )));
        }
        let Value::Data(Term::Map(m)) = out else {
            return Err(PatchError::Validate(format!(
                "selfhost core/cli rename-symbol-forms must return map, got {}",
                out.debug_repr()
            )));
        };
        let forms_t = m.get(&TermOrdKey(Term::symbol(":forms"))).ok_or_else(|| {
            PatchError::Validate("rename-symbol-forms missing :forms".to_string())
        })?;
        let Term::Vector(next_forms) = forms_t else {
            return Err(PatchError::Validate(
                "rename-symbol-forms :forms must be vector".to_string(),
            ));
        };
        let count_t = m
            .get(&TermOrdKey(Term::symbol(":rewrite-count")))
            .ok_or_else(|| {
                PatchError::Validate("rename-symbol-forms missing :rewrite-count".to_string())
            })?;
        let Term::Int(i) = count_t else {
            return Err(PatchError::Validate(
                "rename-symbol-forms :rewrite-count must be int".to_string(),
            ));
        };
        let count = i.to_usize().ok_or_else(|| {
            PatchError::Validate("rename-symbol-forms :rewrite-count out of range".to_string())
        })?;
        Ok((next_forms.clone(), count))
    }

    pub(super) fn split_module_forms_term(
        &mut self,
        forms: &[Term],
        symbols: &[String],
        step_limit: StepLimit,
    ) -> Result<(Vec<Term>, Vec<Term>, usize), PatchError> {
        self.with_limits(step_limit);
        let symbols_t = Term::Vector(symbols.iter().cloned().map(Term::Symbol).collect());
        let req = Term::Map(
            [
                (
                    TermOrdKey(Term::symbol(":forms")),
                    Term::Vector(forms.to_vec()),
                ),
                (TermOrdKey(Term::symbol(":symbols")), symbols_t),
            ]
            .into_iter()
            .collect(),
        );
        let out = self
            .split_module_forms
            .clone()
            .apply(&mut self.ctx, Value::Data(req))
            .map_err(|e| PatchError::Validate(format!("selfhost split-module-forms apply: {e}")))?;
        if let Some(e) = extract_protocol_error(&out, self.error_token) {
            return Err(PatchError::Validate(format!(
                "selfhost core/cli split-module-forms failed: {e}"
            )));
        }
        let Value::Data(Term::Map(m)) = out else {
            return Err(PatchError::Validate(format!(
                "selfhost core/cli split-module-forms must return map, got {}",
                out.debug_repr()
            )));
        };
        let keep_t = m
            .get(&TermOrdKey(Term::symbol(":keep")))
            .ok_or_else(|| PatchError::Validate("split-module-forms missing :keep".to_string()))?;
        let Term::Vector(keep) = keep_t else {
            return Err(PatchError::Validate(
                "split-module-forms :keep must be vector".to_string(),
            ));
        };
        let extracted_t = m
            .get(&TermOrdKey(Term::symbol(":extracted")))
            .ok_or_else(|| {
                PatchError::Validate("split-module-forms missing :extracted".to_string())
            })?;
        let Term::Vector(extracted) = extracted_t else {
            return Err(PatchError::Validate(
                "split-module-forms :extracted must be vector".to_string(),
            ));
        };
        let moved_t = m
            .get(&TermOrdKey(Term::symbol(":moved-def-count")))
            .ok_or_else(|| {
                PatchError::Validate("split-module-forms missing :moved-def-count".to_string())
            })?;
        let Term::Int(i) = moved_t else {
            return Err(PatchError::Validate(
                "split-module-forms :moved-def-count must be int".to_string(),
            ));
        };
        let moved = i.to_usize().ok_or_else(|| {
            PatchError::Validate("split-module-forms :moved-def-count out of range".to_string())
        })?;
        Ok((keep.clone(), extracted.clone(), moved))
    }

    pub(super) fn rewrite_meta_list_forms_term(
        &mut self,
        forms: &[Term],
        field: &str,
        add: &[String],
        remove: &[String],
        replace: Option<&[String]>,
        step_limit: StepLimit,
    ) -> Result<(Vec<Term>, usize), PatchError> {
        self.with_limits(step_limit);
        let mut req = BTreeMap::new();
        req.insert(
            TermOrdKey(Term::symbol(":forms")),
            Term::Vector(forms.to_vec()),
        );
        req.insert(
            TermOrdKey(Term::symbol(":field")),
            Term::Symbol(field.to_string()),
        );
        req.insert(
            TermOrdKey(Term::symbol(":add")),
            Term::Vector(add.iter().cloned().map(Term::Symbol).collect()),
        );
        req.insert(
            TermOrdKey(Term::symbol(":remove")),
            Term::Vector(remove.iter().cloned().map(Term::Symbol).collect()),
        );
        if let Some(replace) = replace {
            req.insert(
                TermOrdKey(Term::symbol(":replace")),
                Term::Vector(replace.iter().cloned().map(Term::Symbol).collect()),
            );
        }
        let out = self
            .rewrite_meta_list_forms
            .clone()
            .apply(&mut self.ctx, Value::Data(Term::Map(req)))
            .map_err(|e| {
                PatchError::Validate(format!("selfhost rewrite-meta-list-forms apply: {e}"))
            })?;
        if let Some(e) = extract_protocol_error(&out, self.error_token) {
            return Err(PatchError::Validate(format!(
                "selfhost core/cli rewrite-meta-list-forms failed: {e}"
            )));
        }
        let Value::Data(Term::Map(m)) = out else {
            return Err(PatchError::Validate(format!(
                "selfhost core/cli rewrite-meta-list-forms must return map, got {}",
                out.debug_repr()
            )));
        };
        let forms_t = m.get(&TermOrdKey(Term::symbol(":forms"))).ok_or_else(|| {
            PatchError::Validate("rewrite-meta-list-forms missing :forms".to_string())
        })?;
        let Term::Vector(next_forms) = forms_t else {
            return Err(PatchError::Validate(
                "rewrite-meta-list-forms :forms must be vector".to_string(),
            ));
        };
        let changed_t = m
            .get(&TermOrdKey(Term::symbol(":changed-entries")))
            .ok_or_else(|| {
                PatchError::Validate("rewrite-meta-list-forms missing :changed-entries".to_string())
            })?;
        let Term::Int(i) = changed_t else {
            return Err(PatchError::Validate(
                "rewrite-meta-list-forms :changed-entries must be int".to_string(),
            ));
        };
        let changed = i.to_usize().ok_or_else(|| {
            PatchError::Validate(
                "rewrite-meta-list-forms :changed-entries out of range".to_string(),
            )
        })?;
        Ok((next_forms.clone(), changed))
    }

    pub(super) fn migrate_contract_signature_forms_term(
        &mut self,
        forms: &[Term],
        contract_symbol: &str,
        from_param: &str,
        to_param: &str,
        step_limit: StepLimit,
    ) -> Result<(Vec<Term>, usize), PatchError> {
        self.with_limits(step_limit);
        let req = Term::Map(
            [
                (
                    TermOrdKey(Term::symbol(":forms")),
                    Term::Vector(forms.to_vec()),
                ),
                (
                    TermOrdKey(Term::symbol(":contract-symbol")),
                    Term::Symbol(contract_symbol.to_string()),
                ),
                (
                    TermOrdKey(Term::symbol(":from-param")),
                    Term::Symbol(from_param.to_string()),
                ),
                (
                    TermOrdKey(Term::symbol(":to-param")),
                    Term::Symbol(to_param.to_string()),
                ),
            ]
            .into_iter()
            .collect(),
        );
        let out = self
            .migrate_contract_signature_forms
            .clone()
            .apply(&mut self.ctx, Value::Data(req))
            .map_err(|e| {
                PatchError::Validate(format!(
                    "selfhost migrate-contract-signature-forms apply: {e}"
                ))
            })?;
        if let Some(e) = extract_protocol_error(&out, self.error_token) {
            return Err(PatchError::Validate(format!(
                "selfhost core/cli migrate-contract-signature-forms failed: {e}"
            )));
        }
        let Value::Data(Term::Map(m)) = out else {
            return Err(PatchError::Validate(format!(
                "selfhost core/cli migrate-contract-signature-forms must return map, got {}",
                out.debug_repr()
            )));
        };
        let forms_t = m.get(&TermOrdKey(Term::symbol(":forms"))).ok_or_else(|| {
            PatchError::Validate("migrate-contract-signature-forms missing :forms".to_string())
        })?;
        let Term::Vector(next_forms) = forms_t else {
            return Err(PatchError::Validate(
                "migrate-contract-signature-forms :forms must be vector".to_string(),
            ));
        };
        let changed_t = m
            .get(&TermOrdKey(Term::symbol(":changed-entries")))
            .ok_or_else(|| {
                PatchError::Validate(
                    "migrate-contract-signature-forms missing :changed-entries".to_string(),
                )
            })?;
        let Term::Int(i) = changed_t else {
            return Err(PatchError::Validate(
                "migrate-contract-signature-forms :changed-entries must be int".to_string(),
            ));
        };
        let changed = i.to_usize().ok_or_else(|| {
            PatchError::Validate(
                "migrate-contract-signature-forms :changed-entries out of range".to_string(),
            )
        })?;
        Ok((next_forms.clone(), changed))
    }
}
