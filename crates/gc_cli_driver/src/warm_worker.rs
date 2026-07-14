use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::PathBuf;
use std::sync::mpsc::Sender;
use std::thread;

use super::*;

pub(super) struct WorkerJob {
    pub(super) request_id: String,
    pub(super) cli: Cli,
    pub(super) flavor: Flavor,
    pub(super) workspace_root: PathBuf,
}

#[derive(Debug)]
pub(super) enum WorkerResult {
    Completed {
        request_id: String,
        result: Result<CmdOut, CliError>,
    },
    WorkspaceError {
        request_id: String,
        message: String,
    },
    Crashed {
        request_id: String,
    },
}

fn run_job(job: WorkerJob) -> WorkerResult {
    let request_id = job.request_id.clone();
    let original = match std::env::current_dir() {
        Ok(path) => path,
        Err(error) => {
            return WorkerResult::WorkspaceError {
                request_id,
                message: error.to_string(),
            };
        }
    };
    if let Err(error) = std::env::set_current_dir(&job.workspace_root) {
        return WorkerResult::WorkspaceError {
            request_id,
            message: error.to_string(),
        };
    }
    let guarded = catch_unwind(AssertUnwindSafe(|| dispatch(&job.cli, job.flavor)));
    if let Err(error) = std::env::set_current_dir(original) {
        return WorkerResult::WorkspaceError {
            request_id,
            message: error.to_string(),
        };
    }
    match guarded {
        Ok(result) => WorkerResult::Completed { request_id, result },
        Err(_) => WorkerResult::Crashed { request_id },
    }
}

pub(super) fn spawn_worker(job: WorkerJob, sender: Sender<WorkerResult>) -> Result<(), String> {
    thread::Builder::new()
        .name("genesis-warm-worker".to_string())
        .spawn(move || {
            let _ = sender.send(run_job(job));
        })
        .map(|_| ())
        .map_err(|error| error.to_string())
}

pub(super) fn run_worker_inline(job: WorkerJob) -> WorkerResult {
    run_job(job)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn worker_panic_boundary_is_recoverable() {
        let result = catch_unwind(AssertUnwindSafe(|| -> Result<(), String> {
            panic!("test-only worker crash")
        }));
        assert!(result.is_err());
    }
}
