use super::*;
#[path = "cmd_vcs_hash.rs"]
mod cmd_vcs_hash;
#[path = "cmd_vcs_render.rs"]
mod cmd_vcs_render;
#[cfg(feature = "parity-harness")]
mod rust_program;
mod selfhost_program;

pub(super) fn cmd_vcs(
    cli: &Cli,
    caps: Option<&Path>,
    log: Option<&Path>,
    cmd: &VcsCmd,
) -> Result<CmdOut, CliError> {
    if matches!(cmd, VcsCmd::Hash { .. }) {
        return cmd_vcs_hash::handle_vcs_hash(cli, cmd);
    }

    let frontend = resolved_coreform_frontend(cli)?;
    let frontend_info = coreform_frontend_json(&frontend);

    let caps = caps.ok_or_else(|| {
        cli_err(
            EX_PARSE,
            "caps/missing",
            "missing --caps (required for effectful vcs operations)",
        )
    })?;

    let policy = CapsPolicy::load(caps)
        .with_context(|| format!("read {}", caps.display()))
        .map_err(caps_parse_cli_err)?;

    let mut ctx = mk_ctx(cli);
    let prelude = build_prelude(&mut ctx);
    let mut env = prelude.env;
    let kind = vcs_contract::kind(cmd);
    let log_op = vcs_contract::log_op(cmd);
    #[cfg(feature = "parity-harness")]
    let (prog, program_hash) = if frontend_is_rust(&frontend) {
        rust_program::build_rust_vcs_program(cmd, &mut ctx, &mut env)?
    } else {
        selfhost_program::build_selfhost_vcs_program(cli, cmd, &mut ctx, &mut env)?
    };

    #[cfg(not(feature = "parity-harness"))]
    let (prog, program_hash) =
        selfhost_program::build_selfhost_vcs_program(cli, cmd, &mut ctx, &mut env)?;

    let toolchain = format!("genesis {}", env!("CARGO_PKG_VERSION"));
    let r = gc_effects::run(&mut ctx, &policy, prog, program_hash, toolchain)
        .map_err(|e| cli_err(EX_EVAL, "effects/run", format!("{e}")))?;

    let log_path = log
        .map(PathBuf::from)
        .unwrap_or_else(|| default_log_path(log_op));
    std::fs::write(&log_path, r.log.to_string_canonical() + "\n")
        .with_context(|| format!("write {}", log_path.display()))
        .map_err(|e| cli_err(EX_IO, "io/write", format!("{e}")))?;

    cmd_vcs_render::finalize_vcs_cmd_output(
        cli,
        cmd,
        kind,
        frontend_info,
        caps,
        &log_path,
        &ctx,
        &r.value,
    )
}
