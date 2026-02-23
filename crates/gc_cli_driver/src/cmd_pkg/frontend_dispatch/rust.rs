use super::*;
use gc_kernel::Env;

pub(super) fn build(
    cmd: &PkgCmd,
    ctx: &mut EvalCtx,
    env: &mut Env,
) -> Result<(Value, &'static str, &'static str, [u8; 32]), CliError> {
    let (forms, kind, log_op) = match cmd {
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
            let (name, selector) =
                parse_pkg_spec(spec).map_err(|e| cli_err(EX_PARSE, "pkg/spec", e.to_string()))?;
            let (strategy_norm, tag_policy_norm) =
                normalize_pkg_add_strategy(&selector, strategy.as_deref(), tag_policy.as_deref())?;
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
        PkgCmd::Update { lock, only } => (
            mk_pkg_update_program(lock, only),
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
    let prog = eval_module(ctx, env, &forms)
        .map_err(|e| cli_err(EX_EVAL, "eval/error", format!("{e}")))?;
    Ok::<_, CliError>((prog, kind, log_op, program_hash))
}
