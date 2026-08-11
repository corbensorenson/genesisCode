#[cfg(not(target_os = "wasi"))]
use super::BridgeError;
#[cfg(not(target_os = "wasi"))]
use crate::runner_process_control::{terminate_and_reap, terminate_descendants};
#[cfg(not(target_os = "wasi"))]
use std::io::Write as _;
#[cfg(not(target_os = "wasi"))]
use std::process::ChildStdin;

#[cfg(all(test, not(target_os = "wasi")))]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum SpawnBridgeCleanupFault {
    TerminationBeforeCleanup,
    Reap,
    ResidualVerification,
    WriterJoin,
}

#[cfg(all(test, not(target_os = "wasi")))]
std::thread_local! {
    static SPAWN_BRIDGE_CLEANUP_FAULT: std::cell::Cell<Option<SpawnBridgeCleanupFault>> =
        const { std::cell::Cell::new(None) };
}

#[cfg(not(target_os = "wasi"))]
static ACTIVE_BRIDGE_IO_PUMPS: std::sync::atomic::AtomicUsize =
    std::sync::atomic::AtomicUsize::new(0);

#[cfg(not(target_os = "wasi"))]
pub(super) struct ActiveBridgeIoPump;

#[cfg(not(target_os = "wasi"))]
impl ActiveBridgeIoPump {
    pub(super) fn enter() -> Self {
        ACTIVE_BRIDGE_IO_PUMPS.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        Self
    }
}

#[cfg(not(target_os = "wasi"))]
impl Drop for ActiveBridgeIoPump {
    fn drop(&mut self) {
        ACTIVE_BRIDGE_IO_PUMPS.fetch_sub(1, std::sync::atomic::Ordering::SeqCst);
    }
}

#[cfg(not(target_os = "wasi"))]
type BridgeWriterPump = std::thread::JoinHandle<std::io::Result<()>>;

#[cfg(not(target_os = "wasi"))]
type BridgeReaderPump = std::thread::JoinHandle<std::io::Result<Vec<u8>>>;

#[cfg(not(target_os = "wasi"))]
#[derive(Default)]
pub(super) struct SpawnBridgePumps {
    cancel: std::sync::Arc<std::sync::atomic::AtomicBool>,
    pub(super) writer: Option<BridgeWriterPump>,
    pub(super) stdout: Option<BridgeReaderPump>,
    pub(super) stderr: Option<BridgeReaderPump>,
}

#[cfg(not(target_os = "wasi"))]
#[derive(Default)]
pub(super) struct SpawnBridgePumpResults {
    pub(super) write: Option<std::io::Result<()>>,
    pub(super) stdout: Option<std::io::Result<Vec<u8>>>,
    pub(super) stderr: Option<std::io::Result<Vec<u8>>>,
    pub(super) join_failures: Vec<&'static str>,
}

#[cfg(not(target_os = "wasi"))]
impl SpawnBridgePumps {
    pub(super) fn cancellation_handle(&self) -> std::sync::Arc<std::sync::atomic::AtomicBool> {
        std::sync::Arc::clone(&self.cancel)
    }

    fn cancel(&self) {
        self.cancel.store(true, std::sync::atomic::Ordering::SeqCst);
    }

    pub(super) fn join_all(mut self) -> SpawnBridgePumpResults {
        let mut results = SpawnBridgePumpResults::default();
        if let Some(writer) = self.writer.take() {
            match writer.join() {
                Ok(result) => results.write = Some(result),
                Err(_) => results.join_failures.push("stdin pump join failed"),
            }
        }
        if let Some(stdout) = self.stdout.take() {
            match stdout.join() {
                Ok(result) => results.stdout = Some(result),
                Err(_) => results.join_failures.push("stdout pump join failed"),
            }
        }
        if let Some(stderr) = self.stderr.take() {
            match stderr.join() {
                Ok(result) => results.stderr = Some(result),
                Err(_) => results.join_failures.push("stderr pump join failed"),
            }
        }
        #[cfg(test)]
        if SPAWN_BRIDGE_CLEANUP_FAULT
            .get()
            .is_some_and(|fault| fault == SpawnBridgeCleanupFault::WriterJoin)
        {
            results
                .join_failures
                .push("injected stdin pump join failure");
        }
        results
    }
}

#[cfg(all(not(target_os = "wasi"), unix))]
fn set_bridge_pipe_nonblocking<T: std::os::fd::AsRawFd>(pipe: &T) -> std::io::Result<()> {
    let fd = pipe.as_raw_fd();
    // SAFETY: fcntl only reads or updates status flags for the valid owned pipe fd.
    let flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
    if flags < 0 {
        return Err(std::io::Error::last_os_error());
    }
    // SAFETY: the descriptor remains owned by `pipe`; O_NONBLOCK changes no ownership.
    if unsafe { libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) } < 0 {
        return Err(std::io::Error::last_os_error());
    }
    Ok(())
}

#[cfg(all(not(target_os = "wasi"), unix))]
pub(super) fn write_bridge_pipe(
    mut stdin: ChildStdin,
    payload: &[u8],
    cancel: &std::sync::atomic::AtomicBool,
) -> std::io::Result<()> {
    set_bridge_pipe_nonblocking(&stdin)?;
    let mut offset = 0;
    while offset < payload.len() {
        if cancel.load(std::sync::atomic::Ordering::SeqCst) {
            return Ok(());
        }
        match stdin.write(&payload[offset..]) {
            Ok(0) => {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::WriteZero,
                    "bridge stdin closed before payload completed",
                ));
            }
            Ok(written) => offset += written,
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => {}
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                std::thread::sleep(std::time::Duration::from_millis(1));
            }
            Err(error) => return Err(error),
        }
    }
    Ok(())
}

#[cfg(all(not(target_os = "wasi"), not(unix)))]
pub(super) fn write_bridge_pipe(
    mut stdin: ChildStdin,
    payload: &[u8],
    _cancel: &std::sync::atomic::AtomicBool,
) -> std::io::Result<()> {
    stdin.write_all(payload)
}

#[cfg(all(not(target_os = "wasi"), unix))]
pub(super) fn read_bridge_pipe<R: std::io::Read + std::os::fd::AsRawFd>(
    mut pipe: R,
    cancel: &std::sync::atomic::AtomicBool,
) -> std::io::Result<Vec<u8>> {
    set_bridge_pipe_nonblocking(&pipe)?;
    let mut bytes = Vec::new();
    let mut chunk = [0u8; 8192];
    loop {
        if cancel.load(std::sync::atomic::Ordering::SeqCst) {
            return Ok(bytes);
        }
        match pipe.read(&mut chunk) {
            Ok(0) => return Ok(bytes),
            Ok(read) => bytes.extend_from_slice(&chunk[..read]),
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => {}
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                std::thread::sleep(std::time::Duration::from_millis(1));
            }
            Err(error) => return Err(error),
        }
    }
}

#[cfg(all(not(target_os = "wasi"), not(unix)))]
pub(super) fn read_bridge_pipe<R: std::io::Read>(
    mut pipe: R,
    _cancel: &std::sync::atomic::AtomicBool,
) -> std::io::Result<Vec<u8>> {
    let mut bytes = Vec::new();
    pipe.read_to_end(&mut bytes)?;
    Ok(bytes)
}

#[cfg(not(target_os = "wasi"))]
pub(super) fn spawn_bridge_reap_error(
    family: &str,
    context: &str,
    failures: &[String],
    prior: Option<&BridgeError>,
) -> BridgeError {
    let details = failures.join("; ");
    let prior = prior
        .map(|error| format!("; prior error: {}: {}", error.code, error.message))
        .unwrap_or_default();
    BridgeError {
        code: format!("{family}/bridge-reap"),
        message: format!("{context}: {details}{prior}"),
    }
}

#[cfg(not(target_os = "wasi"))]
fn terminate_spawn_bridge(
    child: &mut std::process::Child,
) -> std::io::Result<std::process::ExitStatus> {
    #[cfg(test)]
    if SPAWN_BRIDGE_CLEANUP_FAULT
        .get()
        .is_some_and(|fault| fault == SpawnBridgeCleanupFault::TerminationBeforeCleanup)
    {
        return Err(std::io::Error::other(
            "injected spawn bridge termination failure before cleanup",
        ));
    }
    let result = terminate_and_reap(child);
    #[cfg(test)]
    if result.is_ok()
        && SPAWN_BRIDGE_CLEANUP_FAULT
            .get()
            .is_some_and(|fault| fault == SpawnBridgeCleanupFault::Reap)
    {
        return Err(std::io::Error::other("injected spawn bridge reap failure"));
    }
    result
}

#[cfg(not(target_os = "wasi"))]
pub(super) fn terminate_spawn_bridge_descendants(process_id: u32) -> std::io::Result<()> {
    let result = terminate_descendants(process_id);
    #[cfg(test)]
    if result.is_ok()
        && SPAWN_BRIDGE_CLEANUP_FAULT
            .get()
            .is_some_and(|fault| fault == SpawnBridgeCleanupFault::ResidualVerification)
    {
        return Err(std::io::Error::other(
            "injected spawn bridge residual verification failure",
        ));
    }
    result
}

#[cfg(all(test, not(target_os = "wasi")))]
pub(super) fn with_spawn_bridge_cleanup_fault_for_tests<T>(
    fault: SpawnBridgeCleanupFault,
    operation: impl FnOnce() -> T,
) -> T {
    struct ResetFault(Option<SpawnBridgeCleanupFault>);

    impl Drop for ResetFault {
        fn drop(&mut self) {
            SPAWN_BRIDGE_CLEANUP_FAULT.set(self.0);
        }
    }

    let previous = SPAWN_BRIDGE_CLEANUP_FAULT.replace(Some(fault));
    let _reset = ResetFault(previous);
    operation()
}

#[cfg(not(target_os = "wasi"))]
pub(super) fn cleanup_failed_spawn_bridge(
    family: &str,
    child: &mut std::process::Child,
    pumps: SpawnBridgePumps,
    prior: BridgeError,
) -> BridgeError {
    let mut failures = Vec::new();
    let process_id = child.id();
    let termination_failed = if let Err(error) = terminate_spawn_bridge(child) {
        failures.push(format!("process termination/reap failed: {error}"));
        true
    } else {
        false
    };
    if termination_failed {
        pumps.cancel();
    }
    let joined = pumps.join_all();
    failures.extend(joined.join_failures.into_iter().map(str::to_string));
    if termination_failed {
        let fallback = match child.try_wait() {
            Ok(Some(_)) => terminate_spawn_bridge_descendants(process_id),
            Ok(None) | Err(_) => terminate_and_reap(child).map(|_| ()),
        };
        if let Err(error) = fallback {
            failures.push(format!("fallback process termination/reap failed: {error}"));
        }
    }
    if failures.is_empty() {
        prior
    } else {
        spawn_bridge_reap_error(
            family,
            "spawn-per-operation bridge cleanup failed",
            &failures,
            Some(&prior),
        )
    }
}

#[cfg(all(test, not(target_os = "wasi")))]
pub(super) fn active_bridge_io_pumps_for_tests() -> usize {
    ACTIVE_BRIDGE_IO_PUMPS.load(std::sync::atomic::Ordering::SeqCst)
}
