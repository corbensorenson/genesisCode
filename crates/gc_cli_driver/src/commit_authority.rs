use super::*;

pub(super) fn make(cli: &Cli, payload: Term) -> Result<Term, CliError> {
    load(cli)?.make(payload).map_err(map_error)
}

pub(super) fn validate(cli: &Cli, artifact: Term, command: &str) -> Result<Term, CliError> {
    load(cli)?
        .validate(artifact)
        .map_err(|error| map_error_for_command(error, command))
}

fn load(cli: &Cli) -> Result<gc_effects::CommitAuthority, CliError> {
    let (mode, artifact) = resolve_selfhost_toolchain_bootstrap(cli)?;
    gc_effects::CommitAuthority::load(mode, artifact.as_deref()).map_err(map_error)
}

fn map_error(error: gc_effects::CommitAuthorityError) -> CliError {
    map_error_for_command(error, "commit authority")
}

fn map_error_for_command(error: gc_effects::CommitAuthorityError, command: &str) -> CliError {
    match error {
        gc_effects::CommitAuthorityError::Rejected { code, message } => cli_err(
            EX_PARSE,
            "selfhost/error",
            format!("{command}: {code}: {message}"),
        ),
        gc_effects::CommitAuthorityError::Evaluation(message) => cli_err(
            EX_EVAL,
            "eval/error",
            format!("core/commit::authority failed for {command}: {message}"),
        ),
        gc_effects::CommitAuthorityError::Bootstrap(message) => {
            cli_err(EX_INTERNAL, "selfhost/init", message)
        }
        gc_effects::CommitAuthorityError::Protocol(message) => {
            cli_err(EX_INTERNAL, "selfhost/bad-return", message)
        }
    }
}
