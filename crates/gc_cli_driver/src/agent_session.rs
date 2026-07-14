#[path = "agent_session/apply.rs"]
mod apply;
#[path = "agent_session/model.rs"]
mod model;
#[path = "agent_session/storage.rs"]
mod storage;

use std::fs;
use std::path::{Path, PathBuf};

use serde_json::{Value, json};

use super::*;
use apply::{acquire_apply_lock, apply_snapshot};
use model::{
    PatchRecord, SESSION_SCHEMA, SessionRecord, SessionStatus, VerificationRecord,
    WorkspaceSnapshot,
};
use storage::{
    capture_snapshot, load_snapshot, load_state, materialize_snapshot, package_manifest_name,
    resolve_paths, save_state, snapshot_contains,
};

fn session_error(code: &'static str, message: impl Into<String>, session: &str) -> CliError {
    cli_err_with_context(
        EX_IO,
        code,
        message,
        json!({"operation": "agent-session", "session": session}),
    )
}

fn output(cli: &Cli, kind: &'static str, exit_code: u8, data: Value) -> Result<CmdOut, CliError> {
    let summary = data
        .get("current_snapshot")
        .or_else(|| data.get("base_snapshot"))
        .and_then(Value::as_str)
        .unwrap_or(kind);
    let error = (exit_code != EX_OK).then(|| JsonError {
        code: "session/obligations-failed",
        message: "transaction snapshot did not satisfy every package obligation".to_string(),
        context: Some(json!({"operation": "agent-session/verify"})),
    });
    Ok(CmdOut {
        exit_code,
        stdout: if cli.json {
            String::new()
        } else {
            format!("{summary}\n")
        },
        json: json_envelope_value(JsonEnvelope {
            ok: exit_code == EX_OK,
            kind,
            data: Some(data),
            error,
        })?,
    })
}

fn state_data(state: &SessionRecord) -> Value {
    json!({
        "schema": state.schema,
        "session": state.session,
        "package_manifest": state.package_manifest,
        "base_snapshot": state.base_snapshot,
        "current_snapshot": state.current_snapshot,
        "status": state.status,
        "patch_count": state.patches.len(),
        "patches": state.patches,
        "verification": state.verification,
    })
}

fn require_open(state: &SessionRecord) -> Result<(), CliError> {
    if state.status == SessionStatus::Open {
        Ok(())
    } else {
        Err(session_error(
            "session/not-open",
            "transaction is already closed",
            &state.session,
        ))
    }
}

fn snapshot_caps_path(
    live_root: &Path,
    snapshot_root: &Path,
    snapshot: &WorkspaceSnapshot,
    caps: Option<&Path>,
    session: &str,
) -> Result<Option<PathBuf>, CliError> {
    let Some(caps) = caps else {
        return Ok(None);
    };
    let canonical = caps.canonicalize().map_err(|error| {
        session_error(
            "session/caps-unavailable",
            format!("capability policy is unavailable: {error}"),
            session,
        )
    })?;
    let relative = canonical.strip_prefix(live_root).map_err(|_| {
        session_error(
            "session/caps-outside-snapshot",
            "capability policy must be part of the captured package snapshot",
            session,
        )
    })?;
    let relative = relative
        .components()
        .map(|part| part.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/");
    if !snapshot_contains(snapshot, &relative) {
        return Err(session_error(
            "session/caps-outside-snapshot",
            "capability policy was not captured by the package snapshot",
            session,
        ));
    }
    Ok(Some(snapshot_root.join(relative)))
}

fn patch_error(error: gc_patches::PatchError, session: &str) -> CliError {
    let (code, message) = match error {
        gc_patches::PatchError::Parse(_) | gc_patches::PatchError::Validate(_) => (
            "session/patch-invalid",
            "semantic patch syntax or validation failed",
        ),
        gc_patches::PatchError::Io(_) => (
            "session/patch-io",
            "semantic patch processing encountered an I/O error",
        ),
        gc_patches::PatchError::Obligations(_) => (
            "session/patch-obligation",
            "semantic patch obligations could not be evaluated",
        ),
    };
    session_error(code, message, session)
}

fn session_obligation_error(error: gc_obligations::ObligationError, session: &str) -> CliError {
    let boundary = match error {
        gc_obligations::ObligationError::Manifest(_) => "manifest",
        gc_obligations::ObligationError::Module(_) => "module",
        gc_obligations::ObligationError::Test(_) => "test",
        gc_obligations::ObligationError::Typecheck(_) => "typecheck",
        gc_obligations::ObligationError::Opt(_) => "optimizer",
        gc_obligations::ObligationError::Store(_) => "artifact store",
        gc_obligations::ObligationError::Io(_) => "I/O",
    };
    session_error(
        "session/obligation-error",
        format!("transaction obligations failed at the {boundary} boundary"),
        session,
    )
}

fn begin(cli: &Cli, pkg: &Path, session: &str) -> Result<CmdOut, CliError> {
    let paths = resolve_paths(pkg, session)?;
    if paths.state_path.exists() {
        return Err(session_error(
            "session/already-exists",
            "transaction ID already exists",
            session,
        ));
    }
    let package_manifest = package_manifest_name(pkg)?;
    fs::create_dir_all(&paths.transaction_root).map_err(|error| {
        session_error(
            "session/workspace-write",
            format!("cannot create transaction: {error}"),
            session,
        )
    })?;
    let snapshot = capture_snapshot(&paths, &paths.live_root, &package_manifest, session)?;
    materialize_snapshot(&paths, &snapshot, &paths.workspace_root, session)?;
    let state = SessionRecord {
        schema: SESSION_SCHEMA.to_string(),
        session: session.to_string(),
        package_manifest,
        base_snapshot: snapshot.identity.clone(),
        current_snapshot: snapshot.identity,
        status: SessionStatus::Open,
        patches: Vec::new(),
        verification: None,
    };
    save_state(&paths, &state)?;
    output(
        cli,
        "genesis/agent-session-begin-v0.1",
        EX_OK,
        state_data(&state),
    )
}

fn status(cli: &Cli, pkg: &Path, session: &str) -> Result<CmdOut, CliError> {
    let paths = resolve_paths(pkg, session)?;
    let state = load_state(&paths, session)?;
    load_snapshot(&paths, &state.base_snapshot, session)?;
    load_snapshot(&paths, &state.current_snapshot, session)?;
    output(
        cli,
        "genesis/agent-session-status-v0.1",
        EX_OK,
        state_data(&state),
    )
}

fn stage(
    cli: &Cli,
    pkg: &Path,
    session: &str,
    patch: &Path,
    caps: Option<&Path>,
) -> Result<CmdOut, CliError> {
    let paths = resolve_paths(pkg, session)?;
    let mut state = load_state(&paths, session)?;
    require_open(&state)?;
    let before = load_snapshot(&paths, &state.current_snapshot, session)?;
    let patch_bytes = fs::read(patch).map_err(|error| {
        session_error(
            "session/patch-unavailable",
            format!("cannot read semantic patch: {error}"),
            session,
        )
    })?;
    let mut patch_hasher = blake3::Hasher::new();
    patch_hasher.update(b"GCv0.2\0agent-session-patch-v0.1\0");
    patch_hasher.update(&patch_bytes);
    let patch_identity = patch_hasher.finalize().to_hex().to_string();
    let candidate = paths.transaction_root.join("candidate");
    materialize_snapshot(&paths, &before, &candidate, session)?;
    let candidate_patch = paths
        .transaction_root
        .join("patches")
        .join(format!("{patch_identity}.gc"));
    if let Some(parent) = candidate_patch.parent() {
        fs::create_dir_all(parent).map_err(|error| {
            session_error(
                "session/patch-store",
                format!("cannot create patch store: {error}"),
                session,
            )
        })?;
    }
    fs::write(&candidate_patch, &patch_bytes).map_err(|error| {
        session_error(
            "session/patch-store",
            format!("cannot persist semantic patch: {error}"),
            session,
        )
    })?;
    let caps = snapshot_caps_path(&paths.live_root, &candidate, &before, caps, session)?;
    let candidate_pkg = candidate.join(&state.package_manifest);
    let frontend = resolved_coreform_frontend(cli)?;
    let result = gc_patches::apply_patch_with_step_limit_and_frontend(
        &candidate_patch,
        &candidate_pkg,
        caps.as_deref(),
        resolved_step_limit(cli),
        resolved_mem_limits(cli),
        frontend,
    )
    .map_err(|error| patch_error(error, session))?;
    let after = capture_snapshot(&paths, &candidate, &state.package_manifest, session)?;
    if paths.workspace_root.exists() {
        fs::remove_dir_all(&paths.workspace_root).map_err(|error| {
            session_error(
                "session/workspace-write",
                format!("cannot replace transaction workspace: {error}"),
                session,
            )
        })?;
    }
    fs::rename(&candidate, &paths.workspace_root).map_err(|error| {
        session_error(
            "session/workspace-write",
            format!("cannot activate transaction snapshot: {error}"),
            session,
        )
    })?;
    let acceptance = result.acceptance_artifact.clone();
    state.current_snapshot = after.identity.clone();
    state.patches.push(PatchRecord {
        patch: patch_identity.clone(),
        before_snapshot: before.identity,
        after_snapshot: after.identity,
        obligations_ok: result.ok,
        acceptance: acceptance.clone(),
    });
    state.verification = acceptance.map(|acceptance| VerificationRecord {
        snapshot: state.current_snapshot.clone(),
        acceptance,
        obligations_ok: result.ok,
    });
    save_state(&paths, &state)?;
    output(
        cli,
        "genesis/agent-session-stage-v0.1",
        if result.ok { EX_OK } else { EX_OBLIGATIONS },
        json!({
            "session": session,
            "patch": patch_identity,
            "current_snapshot": state.current_snapshot,
            "obligations_ok": result.ok,
            "acceptance": result.acceptance_artifact,
            "patch_artifact": result.patch_artifact,
            "report_artifact": result.report_artifact,
            "package_artifact": result.package_artifact,
        }),
    )
}

fn test_session(
    cli: &Cli,
    pkg: &Path,
    session: &str,
    caps: Option<&Path>,
) -> Result<CmdOut, CliError> {
    let paths = resolve_paths(pkg, session)?;
    let mut state = load_state(&paths, session)?;
    require_open(&state)?;
    let current = load_snapshot(&paths, &state.current_snapshot, session)?;
    let observed = capture_snapshot(
        &paths,
        &paths.workspace_root,
        &state.package_manifest,
        session,
    )?;
    if observed.identity != current.identity {
        return Err(session_error(
            "session/workspace-tampered",
            "transaction workspace differs from its content-addressed snapshot",
            session,
        ));
    }
    let caps = snapshot_caps_path(
        &paths.live_root,
        &paths.workspace_root,
        &current,
        caps,
        session,
    )?;
    let frontend = resolved_coreform_frontend(cli)?;
    let result = gc_obligations::test_package_with_step_limit_and_frontend(
        &paths.workspace_root.join(&state.package_manifest),
        caps.as_deref(),
        resolved_step_limit(cli),
        resolved_mem_limits(cli),
        frontend,
    )
    .map_err(|error| session_obligation_error(error, session))?;
    state.verification = Some(VerificationRecord {
        snapshot: state.current_snapshot.clone(),
        acceptance: result.acceptance_artifact.clone(),
        obligations_ok: result.ok,
    });
    save_state(&paths, &state)?;
    output(
        cli,
        "genesis/agent-session-test-v0.1",
        if result.ok { EX_OK } else { EX_OBLIGATIONS },
        json!({
            "session": session,
            "current_snapshot": state.current_snapshot,
            "obligations_ok": result.ok,
            "acceptance": result.acceptance_artifact,
            "obligation_count": result.obligation_results.len(),
        }),
    )
}

fn rollback_to_base(
    paths: &storage::SessionPaths,
    current: &WorkspaceSnapshot,
    base: &WorkspaceSnapshot,
    package_manifest: &str,
    session: &str,
) -> Result<(), CliError> {
    apply_snapshot(paths, current, base, session).map_err(|_| {
        session_error(
            "session/rollback-failed",
            "transaction rollback could not restore the captured base",
            session,
        )
    })?;
    let restored =
        capture_snapshot(paths, &paths.live_root, package_manifest, session).map_err(|_| {
            session_error(
                "session/rollback-failed",
                "transaction rollback result could not be verified",
                session,
            )
        })?;
    if restored.identity != base.identity {
        return Err(session_error(
            "session/rollback-failed",
            "transaction rollback did not reproduce the captured base",
            session,
        ));
    }
    Ok(())
}

fn apply(cli: &Cli, pkg: &Path, session: &str) -> Result<CmdOut, CliError> {
    let paths = resolve_paths(pkg, session)?;
    let _lock = acquire_apply_lock(&paths, session)?;
    let mut state = load_state(&paths, session)?;
    require_open(&state)?;
    let verification = state.verification.as_ref().ok_or_else(|| {
        session_error(
            "session/unverified",
            "current transaction snapshot has not passed obligations",
            session,
        )
    })?;
    if !verification.obligations_ok || verification.snapshot != state.current_snapshot {
        return Err(session_error(
            "session/unverified",
            "verification does not authorize the current transaction snapshot",
            session,
        ));
    }
    let base = load_snapshot(&paths, &state.base_snapshot, session)?;
    let current = load_snapshot(&paths, &state.current_snapshot, session)?;
    let isolated = capture_snapshot(
        &paths,
        &paths.workspace_root,
        &state.package_manifest,
        session,
    )?;
    if isolated.identity != current.identity {
        return Err(session_error(
            "session/workspace-tampered",
            "transaction workspace differs from its verified snapshot",
            session,
        ));
    }
    let live = capture_snapshot(&paths, &paths.live_root, &state.package_manifest, session)?;
    if live.identity != base.identity {
        return Err(session_error(
            "session/stale-base",
            "live package changed after the transaction began",
            session,
        ));
    }
    apply_snapshot(&paths, &base, &current, session)?;
    let applied = match capture_snapshot(&paths, &paths.live_root, &state.package_manifest, session)
    {
        Ok(applied) => applied,
        Err(error) => {
            rollback_to_base(&paths, &current, &base, &state.package_manifest, session)?;
            return Err(error);
        }
    };
    if applied.identity != current.identity {
        rollback_to_base(&paths, &current, &base, &state.package_manifest, session)?;
        return Err(session_error(
            "session/apply-mismatch",
            "live package does not match the verified transaction snapshot after apply",
            session,
        ));
    }
    state.status = SessionStatus::Applied;
    if let Err(error) = save_state(&paths, &state) {
        rollback_to_base(&paths, &current, &base, &state.package_manifest, session)?;
        return Err(error);
    }
    output(
        cli,
        "genesis/agent-session-apply-v0.1",
        EX_OK,
        state_data(&state),
    )
}

fn abort(cli: &Cli, pkg: &Path, session: &str) -> Result<CmdOut, CliError> {
    let paths = resolve_paths(pkg, session)?;
    let mut state = load_state(&paths, session)?;
    require_open(&state)?;
    state.status = SessionStatus::Aborted;
    save_state(&paths, &state)?;
    output(
        cli,
        "genesis/agent-session-abort-v0.1",
        EX_OK,
        state_data(&state),
    )
}

pub(super) fn cmd_agent_session(cli: &Cli, command: &AgentSessionCmd) -> Result<CmdOut, CliError> {
    match command {
        AgentSessionCmd::Begin { pkg, session } => begin(cli, pkg, session),
        AgentSessionCmd::Status { pkg, session } => status(cli, pkg, session),
        AgentSessionCmd::Stage {
            pkg,
            session,
            patch,
            caps,
        } => stage(cli, pkg, session, patch, caps.as_deref()),
        AgentSessionCmd::Test { pkg, session, caps } => {
            test_session(cli, pkg, session, caps.as_deref())
        }
        AgentSessionCmd::Apply { pkg, session } => apply(cli, pkg, session),
        AgentSessionCmd::Abort { pkg, session } => abort(cli, pkg, session),
    }
}
