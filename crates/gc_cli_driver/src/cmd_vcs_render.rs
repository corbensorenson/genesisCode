use super::*;

#[expect(
    clippy::too_many_arguments,
    reason = "final response assembly spans cli/json/stdout/contracts"
)]
pub(super) fn finalize_vcs_cmd_output(
    cli: &Cli,
    cmd: &VcsCmd,
    kind: &'static str,
    frontend_info: serde_json::Value,
    caps: &Path,
    log_path: &Path,
    ctx: &EvalCtx,
    result_value: &Value,
) -> Result<CmdOut, CliError> {
    let mut ok = true;
    let mut exit_code = EX_OK;
    if let Some(proto) = ctx.protocol
        && let Value::Sealed { token, payload } = result_value
        && *token == proto.error
    {
        ok = false;
        exit_code = EX_EVAL;
        if let Value::Data(Term::Map(m)) = payload.as_ref()
            && matches!(
                m.get(&gc_coreform::TermOrdKey(Term::symbol(":error/code"))),
                Some(Term::Str(s)) if s == "core/caps/denied"
            )
        {
            exit_code = EX_CAPS_DENIED;
        }
    }

    let (value, value_format) = render_value_for_cli(ctx, result_value);

    let map_is_conflict = |m: &std::collections::BTreeMap<TermOrdKey, Term>| {
        let ok_false = matches!(
            m.get(&gc_coreform::TermOrdKey(Term::symbol(":ok")))
                .or_else(|| m.get(&gc_coreform::TermOrdKey(Term::Str(":ok".to_string())))),
            Some(Term::Bool(false))
        );
        let has_conflict = m.contains_key(&gc_coreform::TermOrdKey(Term::symbol(":conflict")))
            || m.contains_key(&gc_coreform::TermOrdKey(Term::Str(":conflict".to_string())));
        ok_false && has_conflict
    };
    let value_is_conflict = match result_value {
        Value::Data(Term::Map(m)) => map_is_conflict(m),
        Value::Data(Term::Str(s)) => match gc_coreform::parse_term(s) {
            Ok(Term::Map(m)) => map_is_conflict(&m),
            _ => false,
        },
        _ => false,
    } || match gc_coreform::parse_term(&value) {
        Ok(Term::Map(m)) => map_is_conflict(&m),
        _ => false,
    };
    if matches!(cmd, VcsCmd::Merge3 { .. } | VcsCmd::ResolveConflict { .. }) && value_is_conflict {
        ok = false;
        exit_code = 3; // conflict
    }

    let stdout = if cli.json {
        String::new()
    } else {
        match cmd {
            VcsCmd::Diff { .. } => extract_vcs_patch_hash(result_value)
                .map(|h| format!("{h}\n"))
                .unwrap_or_else(|| format!("{value}\n")),
            VcsCmd::Apply { .. } => extract_vcs_snapshot_hash(result_value)
                .map(|h| format!("{h}\n"))
                .unwrap_or_else(|| format!("{value}\n")),
            VcsCmd::Blame { .. } => extract_vcs_commit_hash(result_value)
                .map(|h| format!("{h}\n"))
                .unwrap_or_else(|| format!("{value}\n")),
            _ => format!("{value}\n"),
        }
    };

    let env = JsonEnvelope {
        ok,
        kind,
        data: Some(serde_json::json!({
            "coreform_frontend": frontend_info,
            "caps": caps.display().to_string(),
            "log": log_path.display().to_string(),
            "value": value,
            "value_format": value_format,
        })),
        error: if ok {
            None
        } else {
            Some(JsonError {
                code: "vcs/error",
                message: "vcs operation failed".to_string(),
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
