use super::*;
use gc_kernel::Env;

pub(super) fn build_rust_vcs_program(
    cmd: &VcsCmd,
    ctx: &mut EvalCtx,
    env: &mut Env,
) -> Result<(Value, [u8; 32]), CliError> {
    let forms = match cmd {
        VcsCmd::Hash { .. } => {
            return Err(cli_err(
                EX_INTERNAL,
                "vcs/dispatch-drift",
                "vcs hash must be handled before effectful VCS dispatch",
            ));
        }
        VcsCmd::Diff {
            base,
            to,
            out,
            no_store,
        } => mk_vcs_diff_program(base, to, out.as_deref(), !*no_store),
        VcsCmd::Apply {
            base,
            patch,
            out,
            no_store,
        } => mk_vcs_apply_program(base, patch, out.as_deref(), !*no_store),
        VcsCmd::Log { root, max } => mk_vcs_log_program(root, *max),
        VcsCmd::Blame {
            snapshot,
            sym,
            path,
        } => mk_vcs_blame_program(snapshot, sym, path.as_deref())?,
        VcsCmd::Why { snapshot, sym, op } => mk_vcs_why_program(snapshot, sym, op.as_deref())?,
        VcsCmd::Merge3 {
            base,
            left,
            right,
            out,
        } => mk_vcs_merge3_program(base, left, right, out.as_deref()),
        VcsCmd::ResolveConflict {
            conflict,
            strategy,
            picks,
            sets,
            out,
        } => mk_vcs_resolve_conflict_program(
            conflict,
            strategy.as_deref(),
            picks,
            sets,
            out.as_deref(),
        )?,
    };

    let forms = canonicalize_module(forms)
        .map_err(|e| cli_err(EX_PARSE, "canon/coreform", e.to_string()))?;
    let program_hash = hash_module(&forms);
    let prog = eval_module(ctx, env, &forms)
        .map_err(|e| cli_err(EX_EVAL, "eval/error", format!("{e}")))?;
    Ok((prog, program_hash))
}
