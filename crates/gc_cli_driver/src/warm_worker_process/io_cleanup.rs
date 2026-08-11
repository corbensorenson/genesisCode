use std::io;
use std::process::{Child, ChildStderr, ChildStdout, ExitStatus};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::thread;
use std::time::Duration;

use super::platform::{kill_process_tree, terminate_and_reap};

struct ActivePipeReader(Arc<AtomicUsize>);

impl ActivePipeReader {
    fn enter(active: Arc<AtomicUsize>) -> Self {
        active.fetch_add(1, Ordering::SeqCst);
        Self(active)
    }
}

impl Drop for ActivePipeReader {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::SeqCst);
    }
}

#[derive(Default)]
pub(super) struct CapturedPipe {
    pub(super) bytes: Vec<u8>,
    pub(super) observed: u64,
    pub(super) exceeded: bool,
}

type PipeReader = thread::JoinHandle<io::Result<CapturedPipe>>;

#[derive(Default)]
pub(super) struct WorkerPipes {
    cancel: Arc<AtomicBool>,
    active: Arc<AtomicUsize>,
    stdout: Option<PipeReader>,
    stderr: Option<PipeReader>,
}

pub(super) struct FinalizedWorker {
    pub(super) status: Result<ExitStatus, io::Error>,
    pub(super) stdout: CapturedPipe,
    pub(super) stderr: CapturedPipe,
    pub(super) failures: Vec<String>,
    pub(super) contained: bool,
    #[cfg(test)]
    pub(super) active_readers: usize,
}

#[cfg(test)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum WorkerCleanupFault {
    TerminationBeforeCleanup,
    ResidualVerification,
    StdoutJoin,
}

#[cfg(test)]
std::thread_local! {
    static WORKER_CLEANUP_FAULT: std::cell::Cell<Option<WorkerCleanupFault>> =
        const { std::cell::Cell::new(None) };
}

fn set_nonblocking<T: std::os::fd::AsRawFd>(pipe: &T) -> io::Result<()> {
    let fd = pipe.as_raw_fd();
    // SAFETY: fcntl only reads or updates status flags for the valid owned pipe fd.
    let flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
    if flags < 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: the descriptor remains owned by `pipe`; O_NONBLOCK changes no ownership.
    if unsafe { libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) } < 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

fn capture_pipe<R: io::Read + std::os::fd::AsRawFd>(
    mut pipe: R,
    limit: usize,
    total_observed: &AtomicU64,
    total_exceeded: &AtomicBool,
    cancelled: &AtomicBool,
) -> io::Result<CapturedPipe> {
    set_nonblocking(&pipe)?;
    let mut bytes = Vec::with_capacity(limit.min(64 * 1024));
    let mut observed = 0_u64;
    let mut chunk = [0_u8; 16 * 1024];
    loop {
        if cancelled.load(Ordering::SeqCst) {
            break;
        }
        match pipe.read(&mut chunk) {
            Ok(0) => break,
            Ok(read) => {
                observed = observed.saturating_add(read as u64);
                let total = total_observed
                    .fetch_add(read as u64, Ordering::SeqCst)
                    .saturating_add(read as u64);
                if total > limit as u64 {
                    total_exceeded.store(true, Ordering::SeqCst);
                }
                let remaining = limit.saturating_sub(bytes.len());
                bytes.extend_from_slice(&chunk[..read.min(remaining)]);
            }
            Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                thread::sleep(Duration::from_millis(1));
            }
            Err(error) => return Err(error),
        }
    }
    Ok(CapturedPipe {
        bytes,
        observed,
        exceeded: total_exceeded.load(Ordering::SeqCst),
    })
}

impl WorkerPipes {
    fn spawn_reader<R: io::Read + std::os::fd::AsRawFd + Send + 'static>(
        &self,
        name: &str,
        pipe: R,
        limit: usize,
        total_observed: Arc<AtomicU64>,
        total_exceeded: Arc<AtomicBool>,
    ) -> io::Result<PipeReader> {
        let cancelled = Arc::clone(&self.cancel);
        let active = Arc::clone(&self.active);
        thread::Builder::new()
            .name(name.to_string())
            .spawn(move || {
                let _active = ActivePipeReader::enter(active);
                capture_pipe(pipe, limit, &total_observed, &total_exceeded, &cancelled)
            })
    }

    pub(super) fn capture_stdout(
        &mut self,
        pipe: ChildStdout,
        limit: usize,
        total_observed: Arc<AtomicU64>,
        total_exceeded: Arc<AtomicBool>,
    ) -> io::Result<()> {
        self.stdout = Some(self.spawn_reader(
            "genesis-warm-stdout",
            pipe,
            limit,
            total_observed,
            total_exceeded,
        )?);
        Ok(())
    }

    pub(super) fn capture_stderr(
        &mut self,
        pipe: ChildStderr,
        limit: usize,
        total_observed: Arc<AtomicU64>,
        total_exceeded: Arc<AtomicBool>,
    ) -> io::Result<()> {
        self.stderr = Some(self.spawn_reader(
            "genesis-warm-stderr",
            pipe,
            limit,
            total_observed,
            total_exceeded,
        )?);
        Ok(())
    }

    fn cancel(&self) {
        self.cancel.store(true, Ordering::SeqCst);
    }

    fn join_all(mut self) -> (CapturedPipe, CapturedPipe, Vec<String>, usize) {
        let mut failures = Vec::new();
        let stdout = join_reader("stdout", self.stdout.take(), &mut failures);
        let stderr = join_reader("stderr", self.stderr.take(), &mut failures);
        #[cfg(test)]
        if WORKER_CLEANUP_FAULT
            .get()
            .is_some_and(|fault| fault == WorkerCleanupFault::StdoutJoin)
        {
            failures.push("injected stdout pipe join failure".to_string());
        }
        let active = self.active.load(Ordering::SeqCst);
        if active != 0 {
            failures.push(format!(
                "{active} worker pipe reader(s) remained active after join"
            ));
        }
        (stdout, stderr, failures, active)
    }
}

fn join_reader(name: &str, reader: Option<PipeReader>, failures: &mut Vec<String>) -> CapturedPipe {
    let Some(reader) = reader else {
        return CapturedPipe::default();
    };
    match reader.join() {
        Ok(Ok(captured)) => captured,
        Ok(Err(error)) => {
            failures.push(format!("{name} pipe read failed: {error}"));
            CapturedPipe::default()
        }
        Err(_) => {
            failures.push(format!("{name} pipe join failed"));
            CapturedPipe::default()
        }
    }
}

fn terminate_worker(child: &mut Child) -> io::Result<ExitStatus> {
    #[cfg(test)]
    if WORKER_CLEANUP_FAULT
        .get()
        .is_some_and(|fault| fault == WorkerCleanupFault::TerminationBeforeCleanup)
    {
        return Err(io::Error::other(
            "injected worker termination failure before cleanup",
        ));
    }
    terminate_and_reap(child)
}

fn terminate_residual(process_id: u32) -> io::Result<()> {
    let result = kill_process_tree(process_id);
    #[cfg(test)]
    if result.is_ok()
        && WORKER_CLEANUP_FAULT
            .get()
            .is_some_and(|fault| fault == WorkerCleanupFault::ResidualVerification)
    {
        return Err(io::Error::other(
            "injected worker residual verification failure",
        ));
    }
    result
}

pub(super) fn finalize_worker(
    child: &mut Child,
    pipes: WorkerPipes,
    mut status: Option<Result<ExitStatus, io::Error>>,
    termination_required: bool,
) -> FinalizedWorker {
    let process_id = child.id();
    let mut failures = Vec::new();
    let mut contained = true;
    if termination_required {
        match terminate_worker(child) {
            Ok(terminated) => {
                if status.is_none() {
                    status = Some(Ok(terminated));
                }
            }
            Err(error) => {
                failures.push(format!("process termination/reap failed: {error}"));
                pipes.cancel();
                let fallback = match child.try_wait() {
                    Ok(Some(exited)) => {
                        if status.is_none() {
                            status = Some(Ok(exited));
                        }
                        terminate_residual(process_id)
                    }
                    Ok(None) | Err(_) => terminate_and_reap(child).map(|exited| {
                        if status.is_none() {
                            status = Some(Ok(exited));
                        }
                    }),
                };
                if let Err(error) = fallback {
                    contained = false;
                    failures.push(format!("fallback process termination/reap failed: {error}"));
                }
            }
        }
    } else if let Err(error) = terminate_residual(process_id) {
        contained = false;
        failures.push(format!("residual process-tree cleanup failed: {error}"));
    }
    let (stdout, stderr, pipe_failures, active_readers) = pipes.join_all();
    #[cfg(not(test))]
    let _ = active_readers;
    failures.extend(pipe_failures);
    let status = status.unwrap_or_else(|| {
        Err(io::Error::other(
            "isolated worker finalizer produced no exit status",
        ))
    });
    FinalizedWorker {
        status,
        stdout,
        stderr,
        failures,
        contained,
        #[cfg(test)]
        active_readers,
    }
}

#[cfg(test)]
pub(super) fn with_worker_cleanup_fault_for_tests<T>(
    fault: WorkerCleanupFault,
    operation: impl FnOnce() -> T,
) -> T {
    struct ResetFault(Option<WorkerCleanupFault>);

    impl Drop for ResetFault {
        fn drop(&mut self) {
            WORKER_CLEANUP_FAULT.set(self.0);
        }
    }

    let previous = WORKER_CLEANUP_FAULT.replace(Some(fault));
    let _reset = ResetFault(previous);
    operation()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::process::CommandExt as _;
    use std::process::{Command, Stdio};
    use std::time::Instant;

    fn spawn_worker(command_source: &str) -> (Child, WorkerPipes) {
        let mut command = Command::new("/bin/sh");
        command.args(["-c", command_source]);
        command.process_group(0);
        command.stdout(Stdio::piped());
        command.stderr(Stdio::piped());
        let mut child = command.spawn().expect("spawn worker");
        let observed = Arc::new(AtomicU64::new(0));
        let exceeded = Arc::new(AtomicBool::new(false));
        let mut pipes = WorkerPipes::default();
        pipes
            .capture_stdout(
                child.stdout.take().expect("stdout"),
                1024,
                Arc::clone(&observed),
                Arc::clone(&exceeded),
            )
            .expect("start stdout reader");
        pipes
            .capture_stderr(
                child.stderr.take().expect("stderr"),
                1024,
                observed,
                exceeded,
            )
            .expect("start stderr reader");
        (child, pipes)
    }

    #[test]
    fn failed_initial_termination_cancels_pipes_before_fallback_reap() {
        let (mut child, pipes) = spawn_worker("while :; do sleep 1; done");

        let started = Instant::now();
        let finalized = with_worker_cleanup_fault_for_tests(
            WorkerCleanupFault::TerminationBeforeCleanup,
            || finalize_worker(&mut child, pipes, None, true),
        );
        assert!(
            finalized
                .failures
                .iter()
                .any(|failure| failure.contains("injected worker termination failure"))
        );
        assert!(finalized.contained);
        assert!(finalized.status.is_ok());
        assert!(
            child.try_wait().expect("inspect worker").is_some(),
            "worker leader survived fallback cleanup"
        );
        assert_eq!(finalized.active_readers, 0);
        assert!(started.elapsed() < Duration::from_secs(1));
    }

    #[test]
    fn residual_and_pipe_join_failures_are_not_discarded() {
        for (fault, expected, contained) in [
            (
                WorkerCleanupFault::ResidualVerification,
                "injected worker residual verification failure",
                false,
            ),
            (
                WorkerCleanupFault::StdoutJoin,
                "injected stdout pipe join failure",
                true,
            ),
        ] {
            let (mut child, pipes) = spawn_worker("exit 0");
            let status = child.wait().expect("wait for short-lived worker");
            let finalized = with_worker_cleanup_fault_for_tests(fault, || {
                finalize_worker(&mut child, pipes, Some(Ok(status)), false)
            });
            assert!(
                finalized
                    .failures
                    .iter()
                    .any(|failure| failure.contains(expected)),
                "missing {fault:?} failure: {:?}",
                finalized.failures
            );
            assert_eq!(finalized.contained, contained);
            assert_eq!(finalized.active_readers, 0);
        }
    }
}
