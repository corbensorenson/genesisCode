use super::*;

pub(super) fn cmd_pkg(
    cli: &Cli,
    caps: &Path,
    log: Option<&Path>,
    cmd: &PkgCmd,
) -> Result<CmdOut, CliError> {
    let frontend = resolved_coreform_frontend(cli)?;
    let frontend_info = coreform_frontend_json(&frontend);

    let policy = CapsPolicy::load(caps)
        .with_context(|| format!("read {}", caps.display()))
        .map_err(caps_parse_cli_err)?;
    if let Some(out) =
        cmd_pkg_local_workspace_ops(cli, cmd, caps, log, &frontend, frontend_info.clone())?
    {
        return Ok(out);
    }

    let mut ctx = mk_ctx(cli);
    let prelude = build_prelude(&mut ctx);
    let mut env = prelude.env;
    let (prog, kind, log_op, program_hash) = if frontend_is_rust(&frontend) {
        let (forms, kind, log_op) = match cmd {
            PkgCmd::New { .. }
            | PkgCmd::Remove { .. }
            | PkgCmd::Migrate { .. }
            | PkgCmd::Run { .. }
            | PkgCmd::Test { .. }
            | PkgCmd::SelfOptimize { .. }
            | PkgCmd::Abi { .. }
            | PkgCmd::Env { .. } => {
                unreachable!("local workspace ops are handled before frontend dispatch")
            }
            PkgCmd::Init {
                workspace,
                lock,
                policy,
                registry_default,
            } => (
                mk_pkg_init_program(workspace, lock, policy, registry_default.as_deref()),
                "genesis/pkg-init-v0.1",
                "pkg-init",
            ),
            PkgCmd::Add {
                spec,
                lock,
                update_policy,
                registry,
                strategy,
                tag_policy,
            } => {
                let (name, selector) = parse_pkg_spec(spec)
                    .map_err(|e| cli_err(EX_PARSE, "pkg/spec", e.to_string()))?;
                let (strategy_norm, tag_policy_norm) = normalize_pkg_add_strategy(
                    &selector,
                    strategy.as_deref(),
                    tag_policy.as_deref(),
                )?;
                (
                    mk_pkg_add_program(
                        lock,
                        &name,
                        &selector,
                        update_policy,
                        registry.as_deref(),
                        strategy_norm.as_deref(),
                        tag_policy_norm.as_deref(),
                    ),
                    "genesis/pkg-add-v0.1",
                    "pkg-add",
                )
            }
            PkgCmd::Lock { lock, strict } => (
                mk_pkg_lock_program(lock, *strict),
                "genesis/pkg-lock-v0.1",
                "pkg-lock",
            ),
            PkgCmd::Update { lock } => (
                mk_pkg_update_program(lock),
                "genesis/pkg-update-v0.1",
                "pkg-update",
            ),
            PkgCmd::Install {
                lock,
                frozen,
                strict,
            } => (
                mk_pkg_install_program(lock, *frozen, *strict),
                "genesis/pkg-install-v0.1",
                "pkg-install",
            ),
            PkgCmd::Verify { lock } => (
                mk_pkg_verify_program(lock),
                "genesis/pkg-verify-v0.1",
                "pkg-verify",
            ),
            PkgCmd::Doctor { lock } => (
                mk_pkg_verify_program(lock),
                "genesis/pkg-doctor-v0.1",
                "pkg-doctor",
            ),
            PkgCmd::List { lock } => (
                mk_pkg_list_program(lock),
                "genesis/pkg-list-v0.1",
                "pkg-list",
            ),
            PkgCmd::Info { name, lock } => (
                mk_pkg_info_program(lock, name),
                "genesis/pkg-info-v0.1",
                "pkg-info",
            ),
            PkgCmd::Snapshot { pkg } => (
                mk_pkg_snapshot_program(pkg),
                "genesis/pkg-snapshot-v0.1",
                "pkg-snapshot",
            ),
            PkgCmd::Export {
                root,
                out,
                full,
                depth,
                include_evidence,
                include_deps,
                include_refs,
            } => (
                mk_gpk_export_program(
                    root,
                    out,
                    *full,
                    *depth,
                    include_evidence,
                    include_deps,
                    include_refs,
                ),
                "genesis/pkg-export-v0.1",
                "pkg-export",
            ),
            PkgCmd::Import {
                input,
                set_refs,
                policy,
            } => {
                let parsed = parse_local_set_refs(set_refs, policy.as_deref())?;
                (
                    mk_gpk_import_program(input, &parsed),
                    "genesis/pkg-import-v0.1",
                    "pkg-import",
                )
            }
            PkgCmd::Publish {
                remote,
                refname,
                policy: policy_h,
                expected_old,
                depth,
                commit,
            } => (
                mk_pkg_publish_program(
                    remote,
                    refname,
                    policy_h,
                    expected_old.as_deref(),
                    *depth,
                    commit.as_deref(),
                ),
                "genesis/pkg-publish-v0.1",
                "pkg-publish",
            ),
        };
        debug_assert_eq!(kind, pkg_contract::kind(cmd));
        debug_assert_eq!(log_op, pkg_contract::log_op(cmd));

        let forms = canonicalize_module(forms)
            .map_err(|e| cli_err(EX_PARSE, "canon/coreform", e.to_string()))?;
        let program_hash = hash_module(&forms);
        let prog = eval_module(&mut ctx, &mut env, &forms)
            .map_err(|e| cli_err(EX_EVAL, "eval/error", format!("{e}")))?;
        Ok::<_, CliError>((prog, kind, log_op, program_hash))
    } else {
        load_selfhost_toolchain(cli, &mut ctx, &mut env)?;

        let (prog, kind, log_op, desc) = match cmd {
            PkgCmd::New { .. }
            | PkgCmd::Remove { .. }
            | PkgCmd::Migrate { .. }
            | PkgCmd::Run { .. }
            | PkgCmd::Test { .. }
            | PkgCmd::SelfOptimize { .. }
            | PkgCmd::Abi { .. }
            | PkgCmd::Env { .. } => {
                unreachable!("local workspace ops are handled before frontend dispatch")
            }
            PkgCmd::Init {
                workspace,
                lock,
                policy,
                registry_default,
            } => {
                let f = env.get("core/cli::pkg-init-program").ok_or_else(|| {
                    cli_err(
                        EX_INTERNAL,
                        "selfhost/missing",
                        "missing binding core/cli::pkg-init-program",
                    )
                })?;
                let mut mm = std::collections::BTreeMap::new();
                mm.insert(
                    TermOrdKey(Term::symbol(":workspace")),
                    Term::Str(workspace.to_string()),
                );
                mm.insert(
                    TermOrdKey(Term::symbol(":lock")),
                    Term::Str(lock.display().to_string()),
                );
                mm.insert(
                    TermOrdKey(Term::symbol(":policy")),
                    Term::Str(policy.to_string()),
                );
                mm.insert(
                    TermOrdKey(Term::symbol(":registry-default")),
                    registry_default
                        .as_deref()
                        .map(|s| Term::Str(s.to_string()))
                        .unwrap_or(Term::Nil),
                );
                let req = Term::Map(mm);
                let prog = f.apply(&mut ctx, Value::Data(req)).map_err(|e| {
                    cli_err(
                        EX_EVAL,
                        "eval/error",
                        format!("core/cli pkg-init-program failed: {e}"),
                    )
                })?;
                let desc = Term::Map(
                    [
                        (
                            TermOrdKey(Term::symbol(":cmd")),
                            Term::Str("pkg/init".to_string()),
                        ),
                        (
                            TermOrdKey(Term::symbol(":workspace")),
                            Term::Str(workspace.to_string()),
                        ),
                        (
                            TermOrdKey(Term::symbol(":lock")),
                            Term::Str(lock.display().to_string()),
                        ),
                        (
                            TermOrdKey(Term::symbol(":policy")),
                            Term::Str(policy.to_string()),
                        ),
                    ]
                    .into_iter()
                    .collect(),
                );
                (prog, "genesis/pkg-init-v0.1", "pkg-init", desc)
            }
            PkgCmd::Add {
                spec,
                lock,
                update_policy,
                registry,
                strategy,
                tag_policy,
            } => {
                let (name, selector) = parse_pkg_spec(spec)
                    .map_err(|e| cli_err(EX_PARSE, "pkg/spec", e.to_string()))?;
                let (strategy_norm, tag_policy_norm) = normalize_pkg_add_strategy(
                    &selector,
                    strategy.as_deref(),
                    tag_policy.as_deref(),
                )?;
                let f = env.get("core/cli::pkg-add-program").ok_or_else(|| {
                    cli_err(
                        EX_INTERNAL,
                        "selfhost/missing",
                        "missing binding core/cli::pkg-add-program",
                    )
                })?;
                let mut mm = std::collections::BTreeMap::new();
                mm.insert(
                    TermOrdKey(Term::symbol(":lock")),
                    Term::Str(lock.display().to_string()),
                );
                mm.insert(TermOrdKey(Term::symbol(":name")), Term::Str(name.clone()));
                mm.insert(
                    TermOrdKey(Term::symbol(":selector")),
                    Term::Str(selector.clone()),
                );
                mm.insert(
                    TermOrdKey(Term::symbol(":update-policy")),
                    Term::Str(update_policy.to_string()),
                );
                mm.insert(
                    TermOrdKey(Term::symbol(":registry")),
                    registry
                        .as_deref()
                        .map(|s| Term::Str(s.to_string()))
                        .unwrap_or(Term::Nil),
                );
                mm.insert(
                    TermOrdKey(Term::symbol(":strategy")),
                    strategy_norm
                        .as_deref()
                        .map(|s| Term::Str(s.to_string()))
                        .unwrap_or(Term::Nil),
                );
                mm.insert(
                    TermOrdKey(Term::symbol(":tag-policy")),
                    tag_policy_norm
                        .as_deref()
                        .map(|s| Term::Str(s.to_string()))
                        .unwrap_or(Term::Nil),
                );
                let req = Term::Map(mm);
                let prog = f.apply(&mut ctx, Value::Data(req)).map_err(|e| {
                    cli_err(
                        EX_EVAL,
                        "eval/error",
                        format!("core/cli pkg-add-program failed: {e}"),
                    )
                })?;
                let desc = Term::Map(
                    [
                        (
                            TermOrdKey(Term::symbol(":cmd")),
                            Term::Str("pkg/add".to_string()),
                        ),
                        (
                            TermOrdKey(Term::symbol(":lock")),
                            Term::Str(lock.display().to_string()),
                        ),
                        (TermOrdKey(Term::symbol(":name")), Term::Str(name)),
                        (TermOrdKey(Term::symbol(":selector")), Term::Str(selector)),
                        (
                            TermOrdKey(Term::symbol(":update-policy")),
                            Term::Str(update_policy.to_string()),
                        ),
                        (
                            TermOrdKey(Term::symbol(":strategy")),
                            strategy_norm
                                .as_deref()
                                .map(|s| Term::Str(s.to_string()))
                                .unwrap_or(Term::Nil),
                        ),
                        (
                            TermOrdKey(Term::symbol(":tag-policy")),
                            tag_policy_norm
                                .as_deref()
                                .map(|s| Term::Str(s.to_string()))
                                .unwrap_or(Term::Nil),
                        ),
                    ]
                    .into_iter()
                    .collect(),
                );
                (prog, "genesis/pkg-add-v0.1", "pkg-add", desc)
            }
            PkgCmd::Lock { lock, strict } => {
                let f = env.get("core/cli::pkg-lock-program").ok_or_else(|| {
                    cli_err(
                        EX_INTERNAL,
                        "selfhost/missing",
                        "missing binding core/cli::pkg-lock-program",
                    )
                })?;
                let req = Term::Map(
                    [
                        (
                            TermOrdKey(Term::symbol(":lock")),
                            Term::Str(lock.display().to_string()),
                        ),
                        (TermOrdKey(Term::symbol(":strict")), Term::Bool(*strict)),
                    ]
                    .into_iter()
                    .collect(),
                );
                let prog = f.apply(&mut ctx, Value::Data(req)).map_err(|e| {
                    cli_err(
                        EX_EVAL,
                        "eval/error",
                        format!("core/cli pkg-lock-program failed: {e}"),
                    )
                })?;
                let desc = Term::Map(
                    [
                        (
                            TermOrdKey(Term::symbol(":cmd")),
                            Term::Str("pkg/lock".to_string()),
                        ),
                        (
                            TermOrdKey(Term::symbol(":lock")),
                            Term::Str(lock.display().to_string()),
                        ),
                        (TermOrdKey(Term::symbol(":strict")), Term::Bool(*strict)),
                    ]
                    .into_iter()
                    .collect(),
                );
                (prog, "genesis/pkg-lock-v0.1", "pkg-lock", desc)
            }
            PkgCmd::Update { lock } => {
                let f = env.get("core/cli::pkg-update-program").ok_or_else(|| {
                    cli_err(
                        EX_INTERNAL,
                        "selfhost/missing",
                        "missing binding core/cli::pkg-update-program",
                    )
                })?;
                let req = Term::Map(
                    [(
                        TermOrdKey(Term::symbol(":lock")),
                        Term::Str(lock.display().to_string()),
                    )]
                    .into_iter()
                    .collect(),
                );
                let prog = f.apply(&mut ctx, Value::Data(req)).map_err(|e| {
                    cli_err(
                        EX_EVAL,
                        "eval/error",
                        format!("core/cli pkg-update-program failed: {e}"),
                    )
                })?;
                let desc = Term::Map(
                    [
                        (
                            TermOrdKey(Term::symbol(":cmd")),
                            Term::Str("pkg/update".to_string()),
                        ),
                        (
                            TermOrdKey(Term::symbol(":lock")),
                            Term::Str(lock.display().to_string()),
                        ),
                    ]
                    .into_iter()
                    .collect(),
                );
                (prog, "genesis/pkg-update-v0.1", "pkg-update", desc)
            }
            PkgCmd::Install {
                lock,
                frozen,
                strict,
            } => {
                let f = env.get("core/cli::pkg-install-program").ok_or_else(|| {
                    cli_err(
                        EX_INTERNAL,
                        "selfhost/missing",
                        "missing binding core/cli::pkg-install-program",
                    )
                })?;
                let req = Term::Map(
                    [
                        (
                            TermOrdKey(Term::symbol(":lock")),
                            Term::Str(lock.display().to_string()),
                        ),
                        (TermOrdKey(Term::symbol(":frozen")), Term::Bool(*frozen)),
                        (TermOrdKey(Term::symbol(":strict")), Term::Bool(*strict)),
                    ]
                    .into_iter()
                    .collect(),
                );
                let prog = f.apply(&mut ctx, Value::Data(req)).map_err(|e| {
                    cli_err(
                        EX_EVAL,
                        "eval/error",
                        format!("core/cli pkg-install-program failed: {e}"),
                    )
                })?;
                let desc = Term::Map(
                    [
                        (
                            TermOrdKey(Term::symbol(":cmd")),
                            Term::Str("pkg/install".to_string()),
                        ),
                        (
                            TermOrdKey(Term::symbol(":lock")),
                            Term::Str(lock.display().to_string()),
                        ),
                        (TermOrdKey(Term::symbol(":frozen")), Term::Bool(*frozen)),
                        (TermOrdKey(Term::symbol(":strict")), Term::Bool(*strict)),
                    ]
                    .into_iter()
                    .collect(),
                );
                (prog, "genesis/pkg-install-v0.1", "pkg-install", desc)
            }
            PkgCmd::Verify { lock } => {
                let f = env.get("core/cli::pkg-verify-program").ok_or_else(|| {
                    cli_err(
                        EX_INTERNAL,
                        "selfhost/missing",
                        "missing binding core/cli::pkg-verify-program",
                    )
                })?;
                let req = Term::Map(
                    [(
                        TermOrdKey(Term::symbol(":lock")),
                        Term::Str(lock.display().to_string()),
                    )]
                    .into_iter()
                    .collect(),
                );
                let prog = f.apply(&mut ctx, Value::Data(req)).map_err(|e| {
                    cli_err(
                        EX_EVAL,
                        "eval/error",
                        format!("core/cli pkg-verify-program failed: {e}"),
                    )
                })?;
                let desc = Term::Map(
                    [
                        (
                            TermOrdKey(Term::symbol(":cmd")),
                            Term::Str("pkg/verify".to_string()),
                        ),
                        (
                            TermOrdKey(Term::symbol(":lock")),
                            Term::Str(lock.display().to_string()),
                        ),
                    ]
                    .into_iter()
                    .collect(),
                );
                (prog, "genesis/pkg-verify-v0.1", "pkg-verify", desc)
            }
            PkgCmd::Doctor { lock } => {
                let f = env.get("core/cli::pkg-verify-program").ok_or_else(|| {
                    cli_err(
                        EX_INTERNAL,
                        "selfhost/missing",
                        "missing binding core/cli::pkg-verify-program",
                    )
                })?;
                let req = Term::Map(
                    [(
                        TermOrdKey(Term::symbol(":lock")),
                        Term::Str(lock.display().to_string()),
                    )]
                    .into_iter()
                    .collect(),
                );
                let prog = f.apply(&mut ctx, Value::Data(req)).map_err(|e| {
                    cli_err(
                        EX_EVAL,
                        "eval/error",
                        format!("core/cli pkg-verify-program failed: {e}"),
                    )
                })?;
                let desc = Term::Map(
                    [
                        (
                            TermOrdKey(Term::symbol(":cmd")),
                            Term::Str("pkg/doctor".to_string()),
                        ),
                        (
                            TermOrdKey(Term::symbol(":lock")),
                            Term::Str(lock.display().to_string()),
                        ),
                    ]
                    .into_iter()
                    .collect(),
                );
                (prog, "genesis/pkg-doctor-v0.1", "pkg-doctor", desc)
            }
            PkgCmd::List { lock } => {
                let f = env.get("core/cli::pkg-list-program").ok_or_else(|| {
                    cli_err(
                        EX_INTERNAL,
                        "selfhost/missing",
                        "missing binding core/cli::pkg-list-program",
                    )
                })?;
                let req = Term::Map(
                    [(
                        TermOrdKey(Term::symbol(":lock")),
                        Term::Str(lock.display().to_string()),
                    )]
                    .into_iter()
                    .collect(),
                );
                let prog = f.apply(&mut ctx, Value::Data(req)).map_err(|e| {
                    cli_err(
                        EX_EVAL,
                        "eval/error",
                        format!("core/cli pkg-list-program failed: {e}"),
                    )
                })?;
                let desc = Term::Map(
                    [
                        (
                            TermOrdKey(Term::symbol(":cmd")),
                            Term::Str("pkg/list".to_string()),
                        ),
                        (
                            TermOrdKey(Term::symbol(":lock")),
                            Term::Str(lock.display().to_string()),
                        ),
                    ]
                    .into_iter()
                    .collect(),
                );
                (prog, "genesis/pkg-list-v0.1", "pkg-list", desc)
            }
            PkgCmd::Info { name, lock } => {
                let f = env.get("core/cli::pkg-info-program").ok_or_else(|| {
                    cli_err(
                        EX_INTERNAL,
                        "selfhost/missing",
                        "missing binding core/cli::pkg-info-program",
                    )
                })?;
                let req = Term::Map(
                    [
                        (
                            TermOrdKey(Term::symbol(":lock")),
                            Term::Str(lock.display().to_string()),
                        ),
                        (
                            TermOrdKey(Term::symbol(":name")),
                            Term::Str(name.to_string()),
                        ),
                    ]
                    .into_iter()
                    .collect(),
                );
                let prog = f.apply(&mut ctx, Value::Data(req)).map_err(|e| {
                    cli_err(
                        EX_EVAL,
                        "eval/error",
                        format!("core/cli pkg-info-program failed: {e}"),
                    )
                })?;
                let desc = Term::Map(
                    [
                        (
                            TermOrdKey(Term::symbol(":cmd")),
                            Term::Str("pkg/info".to_string()),
                        ),
                        (
                            TermOrdKey(Term::symbol(":lock")),
                            Term::Str(lock.display().to_string()),
                        ),
                        (
                            TermOrdKey(Term::symbol(":name")),
                            Term::Str(name.to_string()),
                        ),
                    ]
                    .into_iter()
                    .collect(),
                );
                (prog, "genesis/pkg-info-v0.1", "pkg-info", desc)
            }
            PkgCmd::Snapshot { pkg } => {
                let f = env.get("core/cli::pkg-snapshot-program").ok_or_else(|| {
                    cli_err(
                        EX_INTERNAL,
                        "selfhost/missing",
                        "missing binding core/cli::pkg-snapshot-program",
                    )
                })?;
                let req = Term::Map(
                    [(
                        TermOrdKey(Term::symbol(":pkg")),
                        Term::Str(pkg.display().to_string()),
                    )]
                    .into_iter()
                    .collect(),
                );
                let prog = f.apply(&mut ctx, Value::Data(req)).map_err(|e| {
                    cli_err(
                        EX_EVAL,
                        "eval/error",
                        format!("core/cli pkg-snapshot-program failed: {e}"),
                    )
                })?;
                let desc = Term::Map(
                    [
                        (
                            TermOrdKey(Term::symbol(":cmd")),
                            Term::Str("pkg/snapshot".to_string()),
                        ),
                        (
                            TermOrdKey(Term::symbol(":pkg")),
                            Term::Str(pkg.display().to_string()),
                        ),
                    ]
                    .into_iter()
                    .collect(),
                );
                (prog, "genesis/pkg-snapshot-v0.1", "pkg-snapshot", desc)
            }
            PkgCmd::Export {
                root,
                out,
                full,
                depth,
                include_evidence,
                include_deps,
                include_refs,
            } => {
                let f = env.get("core/cli::pkg-export-program").ok_or_else(|| {
                    cli_err(
                        EX_INTERNAL,
                        "selfhost/missing",
                        "missing binding core/cli::pkg-export-program",
                    )
                })?;
                let req = Term::Map(
                    [
                        (
                            TermOrdKey(Term::symbol(":root")),
                            Term::Str(root.to_string()),
                        ),
                        (
                            TermOrdKey(Term::symbol(":out")),
                            Term::Str(out.display().to_string()),
                        ),
                        (
                            TermOrdKey(Term::symbol(":mode")),
                            Term::Str(if *full {
                                ":full".to_string()
                            } else {
                                ":shallow".to_string()
                            }),
                        ),
                        (
                            TermOrdKey(Term::symbol(":depth")),
                            Term::Int((*depth as i64).into()),
                        ),
                        (
                            TermOrdKey(Term::symbol(":include-evidence")),
                            Term::Str(include_evidence.to_string()),
                        ),
                        (
                            TermOrdKey(Term::symbol(":include-deps")),
                            Term::Str(include_deps.to_string()),
                        ),
                        (
                            TermOrdKey(Term::symbol(":refs")),
                            Term::Vector(include_refs.iter().cloned().map(Term::Str).collect()),
                        ),
                    ]
                    .into_iter()
                    .collect(),
                );
                let prog = f.apply(&mut ctx, Value::Data(req)).map_err(|e| {
                    cli_err(
                        EX_EVAL,
                        "eval/error",
                        format!("core/cli pkg-export-program failed: {e}"),
                    )
                })?;
                let desc = Term::Map(
                    [
                        (
                            TermOrdKey(Term::symbol(":cmd")),
                            Term::Str("pkg/export".to_string()),
                        ),
                        (
                            TermOrdKey(Term::symbol(":root")),
                            Term::Str(root.to_string()),
                        ),
                        (
                            TermOrdKey(Term::symbol(":out")),
                            Term::Str(out.display().to_string()),
                        ),
                        (TermOrdKey(Term::symbol(":full")), Term::Bool(*full)),
                        (
                            TermOrdKey(Term::symbol(":depth")),
                            Term::Int((*depth as i64).into()),
                        ),
                        (
                            TermOrdKey(Term::symbol(":include-evidence")),
                            Term::Str(include_evidence.to_string()),
                        ),
                        (
                            TermOrdKey(Term::symbol(":include-deps")),
                            Term::Str(include_deps.to_string()),
                        ),
                        (
                            TermOrdKey(Term::symbol(":refs")),
                            Term::Vector(include_refs.iter().cloned().map(Term::Str).collect()),
                        ),
                    ]
                    .into_iter()
                    .collect(),
                );
                (prog, "genesis/pkg-export-v0.1", "pkg-export", desc)
            }
            PkgCmd::Import {
                input,
                set_refs,
                policy,
            } => {
                let f = env.get("core/cli::pkg-import-program").ok_or_else(|| {
                    cli_err(
                        EX_INTERNAL,
                        "selfhost/missing",
                        "missing binding core/cli::pkg-import-program",
                    )
                })?;
                let parsed = parse_local_set_refs(set_refs, policy.as_deref())?;

                let mut set_refs_term: Vec<Term> = Vec::new();
                for sr in &parsed {
                    let mut mm = std::collections::BTreeMap::new();
                    mm.insert(
                        TermOrdKey(Term::symbol(":name")),
                        Term::Str(sr.name.clone()),
                    );
                    mm.insert(
                        TermOrdKey(Term::symbol(":hash")),
                        if sr.hash == "nil" {
                            Term::Nil
                        } else {
                            Term::Str(sr.hash.clone())
                        },
                    );
                    mm.insert(
                        TermOrdKey(Term::symbol(":policy")),
                        Term::Str(sr.policy.clone()),
                    );
                    if let Some(exp) = &sr.expected_old {
                        mm.insert(
                            TermOrdKey(Term::symbol(":expected-old")),
                            if exp == "nil" {
                                Term::Nil
                            } else {
                                Term::Str(exp.clone())
                            },
                        );
                    }
                    set_refs_term.push(Term::Map(mm));
                }

                let req = Term::Map(
                    [
                        (
                            TermOrdKey(Term::symbol(":in")),
                            Term::Str(input.display().to_string()),
                        ),
                        (
                            TermOrdKey(Term::symbol(":set-refs")),
                            Term::Vector(set_refs_term),
                        ),
                    ]
                    .into_iter()
                    .collect(),
                );
                let prog = f.apply(&mut ctx, Value::Data(req)).map_err(|e| {
                    cli_err(
                        EX_EVAL,
                        "eval/error",
                        format!("core/cli pkg-import-program failed: {e}"),
                    )
                })?;
                let desc = Term::Map(
                    [
                        (
                            TermOrdKey(Term::symbol(":cmd")),
                            Term::Str("pkg/import".to_string()),
                        ),
                        (
                            TermOrdKey(Term::symbol(":in")),
                            Term::Str(input.display().to_string()),
                        ),
                        (
                            TermOrdKey(Term::symbol(":set-refs-len")),
                            Term::Int((parsed.len() as i64).into()),
                        ),
                    ]
                    .into_iter()
                    .collect(),
                );
                (prog, "genesis/pkg-import-v0.1", "pkg-import", desc)
            }
            PkgCmd::Publish {
                remote,
                refname,
                policy: policy_h,
                expected_old,
                depth,
                commit,
            } => {
                let f = env.get("core/cli::pkg-publish-program").ok_or_else(|| {
                    cli_err(
                        EX_INTERNAL,
                        "selfhost/missing",
                        "missing binding core/cli::pkg-publish-program",
                    )
                })?;
                let (present, expected) = match expected_old.as_deref() {
                    None => (false, Term::Nil),
                    Some(e) => {
                        if e == "nil" {
                            (true, Term::Nil)
                        } else {
                            (true, Term::Str(e.to_string()))
                        }
                    }
                };
                let req = Term::Map(
                    [
                        (
                            TermOrdKey(Term::symbol(":remote")),
                            Term::Str(remote.to_string()),
                        ),
                        (
                            TermOrdKey(Term::symbol(":ref")),
                            Term::Str(refname.to_string()),
                        ),
                        (
                            TermOrdKey(Term::symbol(":policy")),
                            Term::Str(policy_h.to_string()),
                        ),
                        (
                            TermOrdKey(Term::symbol(":expected-old-present")),
                            Term::Bool(present),
                        ),
                        (TermOrdKey(Term::symbol(":expected-old")), expected),
                        (
                            TermOrdKey(Term::symbol(":depth")),
                            Term::Int((*depth as i64).into()),
                        ),
                        (
                            TermOrdKey(Term::symbol(":commit")),
                            commit
                                .as_deref()
                                .map(|s| Term::Str(s.to_string()))
                                .unwrap_or(Term::Nil),
                        ),
                    ]
                    .into_iter()
                    .collect(),
                );
                let prog = f.apply(&mut ctx, Value::Data(req)).map_err(|e| {
                    cli_err(
                        EX_EVAL,
                        "eval/error",
                        format!("core/cli pkg-publish-program failed: {e}"),
                    )
                })?;
                let desc = Term::Map(
                    [
                        (
                            TermOrdKey(Term::symbol(":cmd")),
                            Term::Str("pkg/publish".to_string()),
                        ),
                        (
                            TermOrdKey(Term::symbol(":remote")),
                            Term::Str(remote.to_string()),
                        ),
                        (
                            TermOrdKey(Term::symbol(":ref")),
                            Term::Str(refname.to_string()),
                        ),
                        (
                            TermOrdKey(Term::symbol(":policy")),
                            Term::Str(policy_h.to_string()),
                        ),
                        (
                            TermOrdKey(Term::symbol(":expected-old-present")),
                            Term::Bool(present),
                        ),
                        (
                            TermOrdKey(Term::symbol(":depth")),
                            Term::Int((*depth as i64).into()),
                        ),
                        (
                            TermOrdKey(Term::symbol(":commit-present")),
                            Term::Bool(commit.is_some()),
                        ),
                    ]
                    .into_iter()
                    .collect(),
                );
                (prog, "genesis/pkg-publish-v0.1", "pkg-publish", desc)
            }
        };
        debug_assert_eq!(kind, pkg_contract::kind(cmd));
        debug_assert_eq!(log_op, pkg_contract::log_op(cmd));
        let program_hash = gc_coreform::hash_term(&desc);
        Ok((prog, kind, log_op, program_hash))
    }?;

    let toolchain = format!("genesis {}", env!("CARGO_PKG_VERSION"));
    let r = gc_effects::run(&mut ctx, &policy, prog, program_hash, toolchain)
        .map_err(|e| cli_err(EX_EVAL, "effects/run", format!("{e}")))?;
    enforce_no_legacy_semantic_fallback_in_selfhost_only(cli, "pkg", &r.log)?;

    let log_path = log
        .map(PathBuf::from)
        .unwrap_or_else(|| default_log_path(log_op));
    std::fs::write(&log_path, r.log.to_string_canonical() + "\n")
        .with_context(|| format!("write {}", log_path.display()))
        .map_err(|e| cli_err(EX_IO, "io/write", format!("{e}")))?;

    let mut ok = true;
    let mut exit_code = EX_OK;
    if let Some(proto) = ctx.protocol
        && let Value::Sealed { token, payload } = &r.value
        && *token == proto.error
    {
        ok = false;
        exit_code = EX_EVAL;
        if let Value::Data(Term::Map(m)) = payload.as_ref()
            && let Some(Term::Str(code)) =
                m.get(&gc_coreform::TermOrdKey(Term::symbol(":error/code")))
        {
            if code == "core/caps/denied" {
                exit_code = EX_CAPS_DENIED;
            } else if matches!(cmd, PkgCmd::Publish { .. })
                && (code.starts_with("core/pkg/")
                    || code.starts_with("core/refs/")
                    || code == "core/store/not-found")
            {
                exit_code = EX_OBLIGATIONS;
            }
        }
    } else if matches!(
        cmd,
        PkgCmd::Install { .. } | PkgCmd::Verify { .. } | PkgCmd::Doctor { .. }
    ) && let Some(false) = extract_pkg_ok_bool(&r.value)
    {
        ok = false;
        exit_code = EX_VERIFY;
    }

    let (value, value_format) = render_value_for_cli(&ctx, &r.value);
    if !ok
        && exit_code == EX_EVAL
        && matches!(cmd, PkgCmd::Publish { .. })
        && (value.contains("core/pkg/")
            || value.contains("core/refs/")
            || value.contains("core/store/not-found"))
    {
        exit_code = EX_OBLIGATIONS;
    }
    let stdout = if cli.json {
        String::new()
    } else {
        match cmd {
            PkgCmd::New { .. }
            | PkgCmd::Remove { .. }
            | PkgCmd::Migrate { .. }
            | PkgCmd::Run { .. }
            | PkgCmd::Test { .. }
            | PkgCmd::SelfOptimize { .. }
            | PkgCmd::Env { .. } => extract_pkg_lock_hash(&r.value)
                .map(|h| format!("{h}\n"))
                .unwrap_or_else(|| format!("{value}\n")),
            PkgCmd::Init { .. }
            | PkgCmd::Add { .. }
            | PkgCmd::Lock { .. }
            | PkgCmd::Update { .. } => extract_pkg_lock_hash(&r.value)
                .map(|h| format!("{h}\n"))
                .unwrap_or_else(|| format!("{value}\n")),
            PkgCmd::Install { .. } | PkgCmd::Verify { .. } => {
                if ok {
                    "ok\n".to_string()
                } else {
                    format!("{value}\n")
                }
            }
            PkgCmd::Doctor { .. } => {
                if ok {
                    "ok\n".to_string()
                } else {
                    format!("{value}\n")
                }
            }
            PkgCmd::List { .. } | PkgCmd::Info { .. } | PkgCmd::Abi { .. } => {
                format!("{value}\n")
            }
            PkgCmd::Snapshot { .. } => extract_pkg_snapshot_hash(&r.value)
                .map(|h| format!("{h}\n"))
                .unwrap_or_else(|| format!("{value}\n")),
            PkgCmd::Export { .. } => extract_pkg_export_bundle_hash(&r.value)
                .map(|h| format!("{h}\n"))
                .unwrap_or_else(|| format!("{value}\n")),
            PkgCmd::Import { .. } => extract_pkg_import_root(&r.value)
                .map(|h| format!("{h}\n"))
                .unwrap_or_else(|| format!("{value}\n")),
            PkgCmd::Publish { .. } => extract_pkg_publish_commit(&r.value)
                .map(|h| format!("{h}\n"))
                .unwrap_or_else(|| {
                    if ok {
                        "ok\n".to_string()
                    } else {
                        format!("{value}\n")
                    }
                }),
        }
    };

    let doctor_report = if let PkgCmd::Doctor { lock } = cmd {
        Some(pkg_doctor::build_pkg_doctor_report(
            &ctx, &r.value, caps, lock, ok, exit_code,
        ))
    } else {
        None
    };
    if let Some(report) = &doctor_report
        && !report.ok
    {
        ok = false;
        if exit_code == EX_OK {
            exit_code = EX_VERIFY;
        }
    }
    let ai_report = pkg_reports::build_pkg_ai_report(cmd, &r.value, caps);

    let mut data = serde_json::json!({
        "coreform_frontend": frontend_info,
        "caps": caps.display().to_string(),
        "log": log_path.display().to_string(),
        "value": value,
        "value_format": value_format,
    });
    if let Some(report) = doctor_report
        && let Some(obj) = data.as_object_mut()
    {
        obj.insert("doctor".to_string(), report.json);
    }
    if let Some(report) = ai_report
        && let Some(obj) = data.as_object_mut()
    {
        obj.insert("report".to_string(), report);
    }
    if let Some(obj) = data.as_object_mut() {
        let telemetry = pkg_telemetry::build_pkg_telemetry(
            cmd,
            ok,
            exit_code,
            &r.log,
            &r.value,
            obj.get("report"),
            obj.get("doctor"),
        );
        obj.insert("telemetry".to_string(), telemetry);
    }

    let env = JsonEnvelope {
        ok,
        kind,
        data: Some(data),
        error: if ok {
            None
        } else {
            Some(JsonError {
                code: "pkg/error",
                message: "pkg operation failed".to_string(),
                context: None,
            })
        },
    };

    Ok(CmdOut {
        exit_code,
        stdout,
        json: json_envelope_value(env)?,
    })
}

fn cmd_pkg_local_workspace_ops(
    cli: &Cli,
    cmd: &PkgCmd,
    caps: &Path,
    log: Option<&Path>,
    frontend: &gc_obligations::CoreformFrontend,
    frontend_info: serde_json::Value,
) -> Result<Option<CmdOut>, CliError> {
    match cmd {
        PkgCmd::Run {
            task,
            workspace_file,
        } => {
            let action = pkg_task_runner::resolve_workspace_task(workspace_file, task)
                .map_err(|e| cli_err(EX_PARSE, "pkg/run", e))?;
            let out = match action {
                pkg_task_runner::WorkspaceTaskAction::Test { pkg } => {
                    cmd_test(cli, &pkg, Some(caps))?
                }
                pkg_task_runner::WorkspaceTaskAction::Pack { pkg } => cmd_pack(cli, &pkg)?,
                pkg_task_runner::WorkspaceTaskAction::Typecheck { pkg } => {
                    cmd_typecheck(cli, &pkg)?
                }
            };
            return Ok(Some(out));
        }
        PkgCmd::Test { pkg, caps: pcaps } => {
            let out = cmd_test(cli, pkg, pcaps.as_deref().or(Some(caps)))?;
            return Ok(Some(out));
        }
        PkgCmd::SelfOptimize {
            pkg,
            caps: pcaps,
            dry_run,
        } => {
            let local = pkg_self_opt::handle_self_optimize(
                pkg,
                pcaps.as_deref(),
                frontend,
                resolved_step_limit(cli),
                resolved_mem_limits(cli),
                *dry_run,
            )
            .map_err(|e| cli_err(EX_OBLIGATIONS, "pkg/self-optimize", e))?;

            let log_path = log
                .map(PathBuf::from)
                .unwrap_or_else(|| default_log_path(local.log_op));
            let log_obj = pkg_workspace_ops::empty_log(local.program_hash);
            std::fs::write(&log_path, log_obj.to_string_canonical() + "\n")
                .with_context(|| format!("write {}", log_path.display()))
                .map_err(|e| cli_err(EX_IO, "io/write", format!("{e}")))?;

            let value_v = Value::Data(local.value.clone());
            let ok = extract_pkg_ok_bool(&value_v).unwrap_or(true);
            let exit_code = if ok { EX_OK } else { EX_OBLIGATIONS };
            let value = gc_coreform::print_term(&local.value);
            let mut data = serde_json::json!({
                "coreform_frontend": frontend_info,
                "caps": caps.display().to_string(),
                "log": log_path.display().to_string(),
                "value": value,
                "value_format": "coreform",
            });
            if let Some(report) = pkg_reports::build_pkg_ai_report(cmd, &value_v, caps)
                && let Some(obj) = data.as_object_mut()
            {
                obj.insert("report".to_string(), report);
            }
            if let Some(obj) = data.as_object_mut() {
                obj.insert(
                    "telemetry".to_string(),
                    pkg_telemetry::build_pkg_telemetry(
                        cmd,
                        ok,
                        exit_code,
                        &log_obj,
                        &value_v,
                        obj.get("report"),
                        None,
                    ),
                );
            }

            let stdout = if cli.json {
                String::new()
            } else {
                format!("{value}\n")
            };
            let env = JsonEnvelope {
                ok,
                kind: local.kind,
                data: Some(data),
                error: if ok {
                    None
                } else {
                    Some(JsonError {
                        code: "pkg/self-optimize",
                        message: "self-optimization promotion failed".to_string(),
                        context: None,
                    })
                },
            };
            return Ok(Some(CmdOut {
                exit_code,
                stdout,
                json: json_envelope_value(env)?,
            }));
        }
        _ => {}
    }

    let local = match cmd {
        PkgCmd::New {
            workspace,
            lock,
            workspace_file,
            policy,
            registry_default,
            members,
        } => Some(
            pkg_workspace_ops::handle_new(
                workspace,
                lock,
                workspace_file,
                policy,
                registry_default.as_deref(),
                members,
            )
            .map_err(|e| cli_err(EX_PARSE, "pkg/new", e))?,
        ),
        PkgCmd::Remove { name, lock } => Some(
            pkg_workspace_ops::handle_remove(name, lock)
                .map_err(|e| cli_err(EX_PARSE, "pkg/remove", e))?,
        ),
        PkgCmd::Migrate {
            pkg,
            lock,
            workspace_file,
            workspace,
            registry_default,
        } => Some(
            pkg_workspace_ops::handle_migrate(
                pkg,
                lock,
                workspace_file,
                workspace.as_deref(),
                registry_default.as_deref(),
            )
            .map_err(|e| cli_err(EX_PARSE, "pkg/migrate", e))?,
        ),
        PkgCmd::Abi { pkg } => Some(
            pkg_abi::handle_abi(
                pkg,
                frontend,
                resolved_step_limit(cli),
                resolved_mem_limits(cli),
            )
            .map_err(|e| cli_err(EX_PARSE, "pkg/abi", e))?,
        ),
        PkgCmd::Env {
            profile,
            lock,
            workspace_file,
            out_dir,
        } => Some(
            pkg_workspace_ops::handle_env(profile, lock, workspace_file, out_dir)
                .map_err(|e| cli_err(EX_PARSE, "pkg/env", e))?,
        ),
        _ => None,
    };
    let Some(local) = local else {
        return Ok(None);
    };

    let log_path = log
        .map(PathBuf::from)
        .unwrap_or_else(|| default_log_path(local.log_op));
    let log_obj = pkg_workspace_ops::empty_log(local.program_hash);
    std::fs::write(&log_path, log_obj.to_string_canonical() + "\n")
        .with_context(|| format!("write {}", log_path.display()))
        .map_err(|e| cli_err(EX_IO, "io/write", format!("{e}")))?;

    let value_v = Value::Data(local.value.clone());
    let ok = extract_pkg_ok_bool(&value_v).unwrap_or(true);
    let exit_code = if ok { EX_OK } else { EX_VERIFY };
    let value = gc_coreform::print_term(&local.value);
    let value_format = "coreform";

    let mut data = serde_json::json!({
        "coreform_frontend": frontend_info,
        "caps": caps.display().to_string(),
        "log": log_path.display().to_string(),
        "value": value,
        "value_format": value_format,
    });
    if let Some(report) = pkg_reports::build_pkg_ai_report(cmd, &value_v, caps)
        && let Some(obj) = data.as_object_mut()
    {
        obj.insert("report".to_string(), report);
    }
    if let Some(obj) = data.as_object_mut() {
        obj.insert(
            "telemetry".to_string(),
            pkg_telemetry::build_pkg_telemetry(
                cmd,
                ok,
                exit_code,
                &log_obj,
                &value_v,
                obj.get("report"),
                None,
            ),
        );
    }

    let stdout = if cli.json {
        String::new()
    } else {
        extract_pkg_lock_hash(&value_v)
            .map(|h| format!("{h}\n"))
            .unwrap_or_else(|| format!("{value}\n"))
    };
    let env = JsonEnvelope {
        ok,
        kind: local.kind,
        data: Some(data),
        error: if ok {
            None
        } else {
            Some(JsonError {
                code: "pkg/error",
                message: "pkg operation failed".to_string(),
                context: None,
            })
        },
    };

    Ok(Some(CmdOut {
        exit_code,
        stdout,
        json: json_envelope_value(env)?,
    }))
}
