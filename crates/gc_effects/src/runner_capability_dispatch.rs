use super::*;

#[expect(
    clippy::too_many_arguments,
    reason = "central capability dispatcher forwards explicit runner context"
)]
pub(super) fn call_capability(
    op: &str,
    payload: &Term,
    pol: Option<&OpPolicy>,
    policy: &CapsPolicy,
    store: Option<&ArtifactStore>,
    refs: Option<&RefsDb>,
    budget: &mut ArtifactBudgetState,
    error_tok: SealId,
) -> Result<Value, EffectsError> {
    let op_eff = dispatch_op_alias(op);
    let timeout_ms = pol.and_then(|p| p.timeout_ms).filter(|ms| *ms > 0);
    if timeout_ms.is_some() && op_eff == "io/fs::write" {
        return Ok(mk_error(
            error_tok,
            "core/caps/policy-error",
            "timeout_ms is not supported for io/fs::write (mutating op)".to_string(),
            Some(op),
        ));
    }
    match op_eff {
        "core/sync::pull" => capability_sync_pull(
            payload, pol, policy, store, refs, budget, error_tok, op, timeout_ms,
        ),

        "core/sync::push" => capability_sync_push(payload, pol, store, error_tok, op, timeout_ms),

        s if s.starts_with("core/pkg-low::") => capability_pkg_low(
            s, payload, pol, policy, store, refs, budget, error_tok, op, timeout_ms,
        ),
        "core/store::put" => cap_store_put(op, payload, pol, policy, store, budget, error_tok),
        "core/store::has" => cap_store_has(op, payload, pol, policy, store, timeout_ms, error_tok),
        "core/store::get" => cap_store_get(
            op, payload, pol, policy, store, budget, timeout_ms, error_tok,
        ),
        "core/store::verify" => cap_store_verify(op, payload, store, error_tok),
        s if s.starts_with("core/vcs-low::") => capability_vcs_low(
            s, payload, pol, policy, store, refs, budget, error_tok, op, timeout_ms,
        ),
        s if s.starts_with("core/gc-low::") || s.starts_with("core/gpk-low::") => {
            capability_gc_gpk_low(
                s, payload, pol, policy, store, refs, budget, error_tok, op, timeout_ms,
            )
        }
        "core/refs::get" => cap_refs_get(payload, refs),
        "core/refs::list" => cap_refs_list(payload, refs),
        "core/refs::set" => cap_refs_set(op, payload, store, refs, error_tok),
        "core/refs::delete" => cap_refs_delete(op, payload, store, refs, error_tok),
        "sys/time::now" => {
            if let Some(ms) = timeout_ms {
                let r = with_timeout(ms, || {
                    Ok(std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_millis())
                })?;
                return Ok(match r {
                    Some(t) => Value::Data(Term::Int(BigInt::from(t))),
                    None => mk_error(
                        error_tok,
                        "core/caps/timeout",
                        format!("capability timed out after {ms}ms: sys/time::now"),
                        Some(op),
                    ),
                });
            }
            let t = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis();
            Ok(Value::Data(Term::Int(BigInt::from(t))))
        }
        "gfx/time::frame-tick" => {
            if let Some(ms) = timeout_ms {
                let r = with_timeout(ms, || {
                    Ok(std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_millis())
                })?;
                return Ok(match r {
                    Some(t) => {
                        let mut m = BTreeMap::new();
                        m.insert(
                            TermOrdKey(Term::Symbol(":time-ms".to_string())),
                            Term::Int(BigInt::from(t)),
                        );
                        Value::Data(Term::Map(m))
                    }
                    None => mk_error(
                        error_tok,
                        "core/caps/timeout",
                        format!("capability timed out after {ms}ms: gfx/time::frame-tick"),
                        Some(op),
                    ),
                });
            }
            let t = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis();
            let mut m = BTreeMap::new();
            m.insert(
                TermOrdKey(Term::Symbol(":time-ms".to_string())),
                Term::Int(BigInt::from(t)),
            );
            Ok(Value::Data(Term::Map(m)))
        }
        "io/fs::read" => {
            let path_s = payload_path(payload)?;
            let base_dir = effective_base_dir(pol)?;
            let max_read_bytes = match op_extra_positive_usize(pol, "max_bytes") {
                Ok(v) => v,
                Err(e) => {
                    return Ok(mk_error(error_tok, "core/caps/policy-error", e, Some(op)));
                }
            };
            if let Some(ms) = timeout_ms {
                let base_dir2 = base_dir.clone();
                let path_s2 = path_s.clone();
                let max_read_bytes2 = max_read_bytes;
                let r = with_timeout_cancellable(ms, move |cancel| {
                    let path = sandbox_path_read(&base_dir2, &path_s2)?;
                    let bytes =
                        read_file_with_optional_limit(&path, max_read_bytes2, Some(&cancel));
                    Ok((path, bytes))
                })?;
                return Ok(match r {
                    Some((_path, Ok(bytes))) => Value::Data(Term::Bytes(bytes.into())),
                    Some((path, Err(FsReadError::Io(e)))) => Value::Sealed {
                        token: error_tok,
                        payload: Box::new(Value::Data(io_error_payload(op, &base_dir, &path, &e))),
                    },
                    Some((path, Err(FsReadError::LimitExceeded { observed, limit }))) => {
                        mk_error_with_ctx(
                            error_tok,
                            "core/caps/resource-limit",
                            format!(
                                "file read exceeds configured limit ({observed} > {limit} bytes)"
                            ),
                            Some(op),
                            Term::Map(
                                [
                                    (
                                        TermOrdKey(Term::symbol(":path")),
                                        Term::Str(path_to_slash(
                                            path.strip_prefix(&base_dir).unwrap_or(&path),
                                        )),
                                    ),
                                    (
                                        TermOrdKey(Term::symbol(":limit-bytes")),
                                        Term::Int((limit as i64).into()),
                                    ),
                                ]
                                .into_iter()
                                .collect(),
                            ),
                        )
                    }
                    Some((_path, Err(FsReadError::Cancelled))) => mk_error(
                        error_tok,
                        "core/caps/timeout",
                        format!("capability timed out after {ms}ms: io/fs::read"),
                        Some(op),
                    ),
                    None => mk_error(
                        error_tok,
                        "core/caps/timeout",
                        format!("capability timed out after {ms}ms: io/fs::read"),
                        Some(op),
                    ),
                });
            }
            let path = sandbox_path_read(&base_dir, &path_s)?;
            match read_file_with_optional_limit(&path, max_read_bytes, None) {
                Ok(bytes) => Ok(Value::Data(Term::Bytes(bytes.into()))),
                Err(FsReadError::Io(e)) => Ok(Value::Sealed {
                    token: error_tok,
                    payload: Box::new(Value::Data(io_error_payload(op, &base_dir, &path, &e))),
                }),
                Err(FsReadError::LimitExceeded { observed, limit }) => Ok(mk_error_with_ctx(
                    error_tok,
                    "core/caps/resource-limit",
                    format!("file read exceeds configured limit ({observed} > {limit} bytes)"),
                    Some(op),
                    Term::Map(
                        [
                            (
                                TermOrdKey(Term::symbol(":path")),
                                Term::Str(path_to_slash(
                                    path.strip_prefix(&base_dir).unwrap_or(&path),
                                )),
                            ),
                            (
                                TermOrdKey(Term::symbol(":limit-bytes")),
                                Term::Int((limit as i64).into()),
                            ),
                        ]
                        .into_iter()
                        .collect(),
                    ),
                )),
                Err(FsReadError::Cancelled) => Ok(mk_error(
                    error_tok,
                    "core/caps/timeout",
                    "io/fs::read cancelled".to_string(),
                    Some(op),
                )),
            }
        }
        "io/fs::write" => {
            let path_s = payload_path(payload)?;
            let data = payload_data(payload)?;
            let base_dir = effective_base_dir(pol)?;
            let create_dirs = pol.is_some_and(|p| p.create_dirs);
            let path = sandbox_path_write(&base_dir, &path_s, create_dirs)?;
            match write_file_no_follow(&path, &data) {
                Ok(()) => Ok(Value::Data(Term::Nil)),
                Err(e) => Ok(Value::Sealed {
                    token: error_tok,
                    payload: Box::new(Value::Data(io_error_payload(op, &base_dir, &path, &e))),
                }),
            }
        }
        "gfx/gpu::create-buffer"
        | "core/task::channel-close"
        | "core/task::channel-open"
        | "core/task::channel-recv"
        | "core/task::channel-send"
        | "core/task::channel-status"
        | "core/task::spawn"
        | "core/task::await"
        | "core/task::cancel"
        | "core/task::status"
        | "core/task::scope"
        | "gfx/gpu::create-texture"
        | "gfx/gpu::create-sampler"
        | "gfx/gpu::create-shader-module"
        | "gfx/gpu::create-bind-group-layout"
        | "gfx/gpu::create-bind-group"
        | "gfx/gpu::create-pipeline-layout"
        | "gfx/gpu::create-render-pipeline"
        | "gfx/gpu::create-compute-pipeline"
        | "gpu/compute::create-buffer"
        | "gpu/compute::create-shader-module"
        | "gpu/compute::create-bind-group-layout"
        | "gpu/compute::create-bind-group"
        | "gpu/compute::create-pipeline-layout"
        | "gpu/compute::create-compute-pipeline"
        | "gpu/compute::create-kernel"
        | "gfx/gpu::destroy-resource"
        | "gpu/compute::destroy-resource"
        | "gfx/gpu::write-buffer"
        | "gpu/compute::write-buffer"
        | "gfx/gpu::write-texture"
        | "gfx/gpu::read-buffer"
        | "gpu/compute::read-buffer"
        | "gfx/gpu::read-texture"
        | "gfx/gpu::submit-frame-graph"
        | "gfx/gpu::submit-compute-graph"
        | "gpu/compute::submit"
        | "gfx/gpu::limits"
        | "gpu/compute::limits"
        | "gfx/gpu::features"
        | "gpu/compute::features"
        | "gfx/window::create-surface"
        | "gfx/window::resize-surface"
        | "gfx/window::set-title"
        | "gfx/window::request-redraw"
        | "gfx/window::surface-info"
        | "gfx/input::poll-events"
        | "gfx/input::set-cursor-mode"
        | "gfx/audio::enqueue"
        | "gfx/audio::set-master"
        | "editor/clipboard::get"
        | "editor/clipboard::set"
        | "editor/dialog::open"
        | "editor/dialog::save"
        | "editor/plugin::command"
        | "editor/task::spawn"
        | "editor/task::fmt-coreform"
        | "editor/task::lint-module"
        | "editor/task::optimize-module"
        | "editor/task::parse-module"
        | "editor/task::poll"
        | "editor/task::cancel"
        | "editor/task::test-pkg"
        | "editor/task::typecheck-pkg"
        | "editor/watch::subscribe"
        | "editor/watch::poll"
        | "editor/watch::unsubscribe" => Ok(mk_error(
            error_tok,
            "core/caps/not-supported",
            format!("capability not supported in this host runtime: {op}"),
            Some(op),
        )),
        _ => Ok(mk_error(
            error_tok,
            "core/caps/unknown-op",
            format!("unknown capability op: {op}"),
            Some(op),
        )),
    }
}
