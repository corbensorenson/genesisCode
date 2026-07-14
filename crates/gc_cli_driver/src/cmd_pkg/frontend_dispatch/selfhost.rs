use super::*;
use gc_kernel::Env;
#[path = "selfhost/init_add.rs"]
mod init_add;

pub(super) fn build(
    cli: &Cli,
    cmd: &PkgCmd,
    ctx: &mut EvalCtx,
    env: &mut Env,
) -> Result<(Value, &'static str, &'static str, [u8; 32]), CliError> {
    load_selfhost_toolchain(cli, ctx, env)?;

    let (prog, kind, log_op, desc) = match cmd {
        PkgCmd::New { .. }
        | PkgCmd::Scaffold { .. }
        | PkgCmd::Remove { .. }
        | PkgCmd::Migrate { .. }
        | PkgCmd::Run { .. }
        | PkgCmd::Build { .. }
        | PkgCmd::Test { .. }
        | PkgCmd::SelfOptimize { .. }
        | PkgCmd::Abi { .. }
        | PkgCmd::Trace { .. }
        | PkgCmd::Qualify { .. }
        | PkgCmd::AssurancePack { .. }
        | PkgCmd::Env { .. }
        | PkgCmd::ProfileRuntime { .. } => {
            return Err(cli_err(
                EX_INTERNAL,
                "pkg/dispatch-drift",
                "local workspace pkg ops must be handled before frontend dispatch",
            ));
        }
        PkgCmd::Init {
            workspace,
            lock,
            policy,
            registry_default,
        } => init_add::build_pkg_init(ctx, env, workspace, lock, policy, registry_default)?,
        PkgCmd::Add {
            spec,
            lock,
            update_policy,
            registry,
            strategy,
            tag_policy,
        } => init_add::build_pkg_add(
            ctx,
            env,
            &init_add::PkgAddProgramRequest {
                spec,
                lock,
                update_policy: update_policy.as_str(),
                registry,
                strategy,
                tag_policy,
            },
        )?,
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
            let prog = f.apply(ctx, Value::data(req)).map_err(|e| {
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
        PkgCmd::Update { lock, only } => {
            let f = env.get("core/cli::pkg-update-program").ok_or_else(|| {
                cli_err(
                    EX_INTERNAL,
                    "selfhost/missing",
                    "missing binding core/cli::pkg-update-program",
                )
            })?;
            let req = Term::Map(
                [
                    (
                        TermOrdKey(Term::symbol(":lock")),
                        Term::Str(lock.display().to_string()),
                    ),
                    (
                        TermOrdKey(Term::symbol(":only")),
                        Term::Vector(only.iter().cloned().map(Term::Str).collect()),
                    ),
                ]
                .into_iter()
                .collect(),
            );
            let prog = f.apply(ctx, Value::data(req)).map_err(|e| {
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
                    (
                        TermOrdKey(Term::symbol(":only")),
                        Term::Vector(only.iter().cloned().map(Term::Str).collect()),
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
            let prog = f.apply(ctx, Value::data(req)).map_err(|e| {
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
        PkgCmd::Verify { lock, .. } => {
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
            let prog = f.apply(ctx, Value::data(req)).map_err(|e| {
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
        PkgCmd::Doctor { lock, .. } => {
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
            let prog = f.apply(ctx, Value::data(req)).map_err(|e| {
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
            let prog = f.apply(ctx, Value::data(req)).map_err(|e| {
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
            let prog = f.apply(ctx, Value::data(req)).map_err(|e| {
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
            let prog = f.apply(ctx, Value::data(req)).map_err(|e| {
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
            let prog = f.apply(ctx, Value::data(req)).map_err(|e| {
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
            let prog = f.apply(ctx, Value::data(req)).map_err(|e| {
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
            let prog = f.apply(ctx, Value::data(req)).map_err(|e| {
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
        PkgCmd::Bridge {
            ecosystem,
            name,
            version,
            source,
            source_hash,
            key_id,
            public_key,
            lock,
            dep_name,
            registry,
        } => {
            let forms = canonicalize_module(mk_pkg_bridge_program(
                ecosystem,
                name,
                version,
                source,
                source_hash,
                key_id,
                public_key,
                lock.as_deref(),
                dep_name.as_deref(),
                registry.as_deref(),
            ))
            .map_err(|e| cli_err(EX_PARSE, "canon/coreform", e.to_string()))?;
            let prog = eval_module(ctx, env, &forms)
                .map_err(|e| cli_err(EX_EVAL, "eval/error", format!("{e}")))?;
            let desc = Term::Map(
                [
                    (
                        TermOrdKey(Term::symbol(":cmd")),
                        Term::Str("pkg/bridge".to_string()),
                    ),
                    (
                        TermOrdKey(Term::symbol(":ecosystem")),
                        Term::Str(ecosystem.to_string()),
                    ),
                    (
                        TermOrdKey(Term::symbol(":name")),
                        Term::Str(name.to_string()),
                    ),
                    (
                        TermOrdKey(Term::symbol(":version")),
                        Term::Str(version.to_string()),
                    ),
                    (
                        TermOrdKey(Term::symbol(":source")),
                        Term::Str(source.to_string()),
                    ),
                    (
                        TermOrdKey(Term::symbol(":source-hash")),
                        Term::Str(source_hash.to_string()),
                    ),
                    (
                        TermOrdKey(Term::symbol(":key-id")),
                        Term::Str(key_id.to_string()),
                    ),
                    (
                        TermOrdKey(Term::symbol(":public-key")),
                        Term::Str(public_key.to_string()),
                    ),
                    (
                        TermOrdKey(Term::symbol(":lock-present")),
                        Term::Bool(lock.is_some()),
                    ),
                    (
                        TermOrdKey(Term::symbol(":dep-name-present")),
                        Term::Bool(dep_name.is_some()),
                    ),
                    (
                        TermOrdKey(Term::symbol(":registry-present")),
                        Term::Bool(registry.is_some()),
                    ),
                ]
                .into_iter()
                .collect(),
            );
            (prog, "genesis/pkg-bridge-v0.1", "pkg-bridge", desc)
        }
    };
    debug_assert_eq!(kind, pkg_contract::kind(cmd));
    debug_assert_eq!(log_op, pkg_contract::log_op(cmd));
    let program_hash = gc_coreform::hash_term(&desc);
    Ok((prog, kind, log_op, program_hash))
}
