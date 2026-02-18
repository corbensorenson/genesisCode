use std::path::{Path, PathBuf};

use gc_pkg::WorkspaceConfig;

pub(crate) enum WorkspaceTaskAction {
    Test { pkg: PathBuf },
    Pack { pkg: PathBuf },
    Typecheck { pkg: PathBuf },
}

pub(crate) fn resolve_workspace_task(
    workspace_file: &Path,
    task_name: &str,
) -> Result<WorkspaceTaskAction, String> {
    let ws = WorkspaceConfig::load(workspace_file).map_err(|e| e.to_string())?;
    let task = ws.tasks.get(task_name).ok_or_else(|| {
        format!(
            "task `{task_name}` not found in {}",
            workspace_file.display()
        )
    })?;

    let cmd = task.cmd.trim();
    match cmd {
        "test" => Ok(WorkspaceTaskAction::Test {
            pkg: resolve_pkg_path(workspace_file, task),
        }),
        "pack" => Ok(WorkspaceTaskAction::Pack {
            pkg: resolve_pkg_path(workspace_file, task),
        }),
        "typecheck" => Ok(WorkspaceTaskAction::Typecheck {
            pkg: resolve_pkg_path(workspace_file, task),
        }),
        other => Err(format!(
            "unsupported task cmd `{other}` for task `{task_name}`; supported: test|pack|typecheck"
        )),
    }
}

fn resolve_pkg_path(workspace_file: &Path, task: &gc_pkg::WorkspaceTask) -> PathBuf {
    let raw = task
        .pkg
        .as_deref()
        .or(task.file.as_deref())
        .unwrap_or("package.toml");
    let candidate = PathBuf::from(raw);
    if candidate.is_absolute() {
        return candidate;
    }
    workspace_file
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .join(candidate)
}
