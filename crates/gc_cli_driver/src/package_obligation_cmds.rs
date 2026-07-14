use std::path::Path;

use super::*;

pub(crate) fn obligation_err(e: gc_obligations::ObligationError) -> CliError {
    let context = structured_failures::obligation_context("obligation/run", &e);
    match e {
        gc_obligations::ObligationError::Manifest(s) => {
            cli_err_with_context(EX_PARSE, "manifest/error", s, context)
        }
        gc_obligations::ObligationError::Module(s) => {
            cli_err_with_context(EX_PARSE, "module/error", s, context)
        }
        gc_obligations::ObligationError::Test(s) => {
            cli_err_with_context(EX_EVAL, "test/error", s, context)
        }
        gc_obligations::ObligationError::Typecheck(s) => {
            cli_err_with_context(EX_EVAL, "typecheck/error", s, context)
        }
        gc_obligations::ObligationError::Opt(s) => {
            cli_err_with_context(EX_INTERNAL, "opt/error", s, context)
        }
        gc_obligations::ObligationError::Store(s) => {
            cli_err_with_context(EX_INTERNAL, "store/error", s, context)
        }
        gc_obligations::ObligationError::Io(e) => {
            cli_err_with_context(EX_IO, "io/error", format!("{e}"), context)
        }
    }
}

pub(crate) fn cmd_test(cli: &Cli, pkg: &Path, caps: Option<&Path>) -> Result<CmdOut, CliError> {
    let frontend = resolved_coreform_frontend(cli)?;
    let frontend_info = coreform_frontend_json(&frontend);

    let r = gc_obligations::test_package_with_step_limit_and_frontend(
        pkg,
        caps,
        resolved_step_limit(cli),
        resolved_mem_limits(cli),
        frontend.clone(),
    )
    .map_err(obligation_err)?;
    let exit_code = if r.ok { EX_OK } else { EX_OBLIGATIONS };

    let obligations: Vec<serde_json::Value> = r
        .obligation_results
        .iter()
        .map(|o| {
            serde_json::json!({
                "name": o.name,
                "ok": o.ok,
                "artifact": o.artifact,
                "errors": o.errors,
            })
        })
        .collect();
    let failed_obligations: Vec<serde_json::Value> = r
        .obligation_results
        .iter()
        .filter(|obligation| !obligation.ok)
        .map(|obligation| {
            serde_json::json!({
                "name": obligation.name,
                "artifact": obligation.artifact,
                "errors": obligation.errors,
            })
        })
        .collect();
    let error = (!r.ok).then(|| JsonError {
        code: "test/error",
        message: format!(
            "{} package obligation{} failed",
            failed_obligations.len(),
            if failed_obligations.len() == 1 {
                ""
            } else {
                "s"
            }
        ),
        context: Some(
            structured_failures::FailureContext::new(
                "evaluator",
                "obligations-failed",
                "obligation/run",
            )
            .fact("acceptance_artifact", r.acceptance_artifact.clone())
            .fact("failed_obligations", failed_obligations)
            .into_value(),
        ),
    });

    let env = JsonEnvelope {
        ok: r.ok,
        kind: "genesis/test-v0.2",
        data: Some(serde_json::json!({
            "pkg": pkg.display().to_string(),
            "caps": caps.map(|p| p.display().to_string()),
            "coreform_frontend": frontend_info,
            "kernel_eval_backend_default": "compiled",
            "acceptance_artifact": r.acceptance_artifact,
            "obligations": obligations,
        })),
        error,
    };

    Ok(CmdOut {
        exit_code,
        stdout: if cli.json {
            String::new()
        } else {
            format!("{}\n", r.acceptance_artifact)
        },
        json: json_envelope_value(env)?,
    })
}

pub(crate) fn cmd_pack(cli: &Cli, pkg: &Path) -> Result<CmdOut, CliError> {
    let frontend = resolved_coreform_frontend(cli)?;
    let frontend_info = coreform_frontend_json(&frontend);

    let h = gc_obligations::pack_with_limits_and_frontend(
        pkg,
        resolved_step_limit(cli),
        resolved_mem_limits(cli),
        frontend,
    )
    .map_err(obligation_err)?;
    let env = JsonEnvelope {
        ok: true,
        kind: "genesis/pack-v0.2",
        data: Some(serde_json::json!({
            "pkg": pkg.display().to_string(),
            "coreform_frontend": frontend_info,
            "package_artifact": h,
        })),
        error: None,
    };
    Ok(CmdOut {
        exit_code: EX_OK,
        stdout: if cli.json {
            String::new()
        } else {
            format!("{h}\n")
        },
        json: json_envelope_value(env)?,
    })
}
