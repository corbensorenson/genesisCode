use super::*;
#[path = "cmd_selfhost_helpers.rs"]
mod cmd_selfhost_helpers;
use cmd_selfhost_helpers::{
    build_selfhost_cutover_rows_from_cli, percent_basis_points, percent_string_from_bps,
    write_content_addressed_artifact,
};

pub(super) fn cmd_selfhost_dashboard(
    cli: &Cli,
    markdown: Option<&Path>,
    store: Option<&Path>,
) -> Result<CmdOut, CliError> {
    let artifact = resolved_selfhost_artifact_for_frontend(cli);
    let artifact_path = artifact.as_ref().map(|p| p.display().to_string());
    let artifact_exists = artifact.as_ref().is_some_and(|p| p.is_file());
    let strict = selfhost_only_enabled(cli);
    let rows = build_selfhost_cutover_rows_from_cli()?;

    let total_commands = rows.len();
    let routed_count = rows.iter().filter(|r| r.selfhost_routed).count();
    let default_selfhost_count = rows.iter().filter(|r| r.default_selfhost).count();
    let fast_path_total = rows.iter().filter(|r| r.fast_path_required).count();
    let fast_path_default_ok = rows
        .iter()
        .filter(|r| r.fast_path_required)
        .all(|r| r.default_selfhost && r.selfhost_routed);
    let routed_bps = percent_basis_points(routed_count, total_commands);
    let default_bps = percent_basis_points(default_selfhost_count, total_commands);

    let rows_term: Vec<Term> = rows
        .iter()
        .map(|row| {
            Term::Map(
                [
                    (TermOrdKey(Term::symbol(":cmd")), Term::Str(row.cmd.clone())),
                    (
                        TermOrdKey(Term::symbol(":fast-path-required")),
                        Term::Bool(row.fast_path_required),
                    ),
                    (
                        TermOrdKey(Term::symbol(":selfhost-routed")),
                        Term::Bool(row.selfhost_routed),
                    ),
                    (
                        TermOrdKey(Term::symbol(":default-selfhost")),
                        Term::Bool(row.default_selfhost),
                    ),
                ]
                .into_iter()
                .collect(),
            )
        })
        .collect();

    let dashboard_term = Term::Map(
        [
            (
                TermOrdKey(Term::symbol(":kind")),
                Term::Str("genesis/selfhost-cutover-dashboard-v0.2".to_string()),
            ),
            (TermOrdKey(Term::symbol(":v")), Term::Int(1.into())),
            (TermOrdKey(Term::symbol(":strict")), Term::Bool(strict)),
            (
                TermOrdKey(Term::symbol(":artifact-configured")),
                Term::Bool(artifact.is_some()),
            ),
            (
                TermOrdKey(Term::symbol(":artifact-exists")),
                Term::Bool(artifact_exists),
            ),
            (
                TermOrdKey(Term::symbol(":artifact-path")),
                artifact
                    .as_ref()
                    .map(|p| Term::Str(p.display().to_string()))
                    .unwrap_or(Term::Nil),
            ),
            (
                TermOrdKey(Term::symbol(":summary")),
                Term::Map(
                    [
                        (
                            TermOrdKey(Term::symbol(":total-commands")),
                            Term::Int((total_commands as i64).into()),
                        ),
                        (
                            TermOrdKey(Term::symbol(":selfhost-routed-commands")),
                            Term::Int((routed_count as i64).into()),
                        ),
                        (
                            TermOrdKey(Term::symbol(":selfhost-default-commands")),
                            Term::Int((default_selfhost_count as i64).into()),
                        ),
                        (
                            TermOrdKey(Term::symbol(":fast-path-required-commands")),
                            Term::Int((fast_path_total as i64).into()),
                        ),
                        (
                            TermOrdKey(Term::symbol(":fast-path-default-ok")),
                            Term::Bool(fast_path_default_ok),
                        ),
                        (
                            TermOrdKey(Term::symbol(":selfhost-routed-bps")),
                            Term::Int((routed_bps as i64).into()),
                        ),
                        (
                            TermOrdKey(Term::symbol(":selfhost-default-bps")),
                            Term::Int((default_bps as i64).into()),
                        ),
                    ]
                    .into_iter()
                    .collect(),
                ),
            ),
            (
                TermOrdKey(Term::symbol(":commands")),
                Term::Vector(rows_term),
            ),
        ]
        .into_iter()
        .collect(),
    );
    let artifact_bytes = print_term(&dashboard_term);

    let store_dir = store
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from(DASHBOARD_STORE_DEFAULT_REL));
    let (artifact_hash, artifact_path_fs) =
        write_content_addressed_artifact(&store_dir, artifact_bytes.as_bytes())?;

    let markdown_path = markdown
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from(DASHBOARD_MARKDOWN_DEFAULT_REL));
    let markdown_body = {
        let mut lines = vec![
            "# Selfhost Cutover Dashboard (v0.2)".to_string(),
            "".to_string(),
            format!("- Artifact hash: `{artifact_hash}`"),
            format!("- Store artifact: `{}`", artifact_path_fs.display()),
            format!(
                "- Selfhost toolchain artifact configured: `{}`",
                artifact_path.as_deref().unwrap_or("none")
            ),
            format!("- Selfhost toolchain artifact exists: `{artifact_exists}`"),
            "".to_string(),
            "## Summary".to_string(),
            "".to_string(),
            "| Metric | Value |".to_string(),
            "| --- | --- |".to_string(),
            format!("| Total command groups | {} |", total_commands),
            format!("| Selfhost-routed command groups | {} |", routed_count),
            format!(
                "| Selfhost-routed coverage | {} |",
                percent_string_from_bps(routed_bps)
            ),
            format!(
                "| Default selfhost coverage | {} |",
                percent_string_from_bps(default_bps)
            ),
            format!("| Fast-path default OK | {} |", fast_path_default_ok),
            "".to_string(),
            "## Command Coverage".to_string(),
            "".to_string(),
            "| Command | Fast Path | Selfhost Routed | Default Selfhost |".to_string(),
            "| --- | --- | --- | --- |".to_string(),
        ];
        for row in &rows {
            lines.push(format!(
                "| `{}` | {} | {} | {} |",
                row.cmd, row.fast_path_required, row.selfhost_routed, row.default_selfhost
            ));
        }
        lines.push(String::new());
        lines.join("\n")
    };
    if let Some(parent) = markdown_path.parent()
        && !parent.as_os_str().is_empty()
    {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("create {}", parent.display()))
            .map_err(|e| cli_err(EX_IO, "io/mkdir", format!("{e}")))?;
    }
    std::fs::write(&markdown_path, markdown_body.as_bytes())
        .with_context(|| format!("write {}", markdown_path.display()))
        .map_err(|e| cli_err(EX_IO, "io/write", format!("{e}")))?;

    let env = JsonEnvelope {
        ok: true,
        kind: "genesis/selfhost-dashboard-v0.2",
        data: Some(serde_json::json!({
            "artifact_hash": artifact_hash,
            "store_artifact": artifact_path_fs.display().to_string(),
            "store_dir": store_dir.display().to_string(),
            "markdown": markdown_path.display().to_string(),
            "artifact_configured": artifact.is_some(),
            "artifact_exists": artifact_exists,
            "artifact_path": artifact_path,
            "summary": {
                "total_commands": total_commands,
                "selfhost_routed_commands": routed_count,
                "selfhost_default_commands": default_selfhost_count,
                "fast_path_required_commands": fast_path_total,
                "fast_path_default_ok": fast_path_default_ok,
                "selfhost_routed_percent": percent_string_from_bps(routed_bps),
                "selfhost_default_percent": percent_string_from_bps(default_bps),
            }
        })),
        error: None,
    };
    Ok(CmdOut {
        exit_code: EX_OK,
        stdout: if cli.json {
            String::new()
        } else {
            format!(
                "{}\n{}\n",
                artifact_path_fs.display(),
                markdown_path.display()
            )
        },
        json: json_envelope_value(env)?,
    })
}


#[path = "cmd_selfhost_artifact.rs"]
mod cmd_selfhost_artifact;

pub(super) fn cmd_selfhost_artifact(
    cli: &Cli,
    out: &Path,
    min_stage2_supported_modules: u64,
    min_stage2_validated_modules: u64,
    recover_missing_artifact: bool,
) -> Result<CmdOut, CliError> {
    cmd_selfhost_artifact::cmd_selfhost_artifact(
        cli,
        out,
        min_stage2_supported_modules,
        min_stage2_validated_modules,
        recover_missing_artifact,
    )
}
