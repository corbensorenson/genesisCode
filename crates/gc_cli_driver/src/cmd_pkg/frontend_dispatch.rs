use super::*;
use gc_kernel::Env;

mod rust;
mod selfhost;

pub(super) fn build_pkg_effect_program(
    cli: &Cli,
    cmd: &PkgCmd,
    frontend: &gc_obligations::CoreformFrontend,
    ctx: &mut EvalCtx,
    env: &mut Env,
) -> Result<(Value, &'static str, &'static str, [u8; 32]), CliError> {
    if frontend_is_rust(frontend) {
        rust::build(cmd, ctx, env)
    } else {
        selfhost::build(cli, cmd, ctx, env)
    }
}
