#[cfg(not(target_os = "wasi"))]
use std::process::{Child, Command, ExitStatus};

#[cfg(all(not(target_os = "wasi"), unix))]
pub(crate) fn configure_killable_process(command: &mut Command) {
    use std::os::unix::process::CommandExt;

    command.process_group(0);
}

#[cfg(all(not(target_os = "wasi"), unix))]
pub(crate) fn hard_process_tree_termination_supported() -> bool {
    true
}

#[cfg(all(not(target_os = "wasi"), not(unix)))]
pub(crate) fn configure_killable_process(_command: &mut Command) {}

#[cfg(all(not(target_os = "wasi"), not(unix)))]
pub(crate) fn hard_process_tree_termination_supported() -> bool {
    false
}

#[cfg(all(not(target_os = "wasi"), unix))]
fn kill_process_group(process_id: u32) -> std::io::Result<()> {
    let process_id = i32::try_from(process_id).map_err(|_| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "child process ID exceeds the platform process-group range",
        )
    })?;
    // The child is placed in a new process group before exec, so a negative PID
    // targets only that bridge tree rather than the Genesis process group.
    let result = unsafe { libc::kill(-process_id, libc::SIGKILL) };
    if result == 0 {
        return Ok(());
    }
    let error = std::io::Error::last_os_error();
    if error.raw_os_error() == Some(libc::ESRCH) {
        Ok(())
    } else {
        Err(error)
    }
}

#[cfg(all(not(target_os = "wasi"), not(unix)))]
fn kill_process_group(_process_id: u32) -> std::io::Result<()> {
    Ok(())
}

#[cfg(not(target_os = "wasi"))]
pub(crate) fn terminate_descendants(process_id: u32) -> std::io::Result<()> {
    kill_process_group(process_id)
}

#[cfg(all(not(target_os = "wasi"), unix))]
pub(crate) fn terminate_and_reap(child: &mut Child) -> std::io::Result<ExitStatus> {
    let group_result = kill_process_group(child.id());
    if group_result.is_err() {
        let _ = child.kill();
    }
    let status = child.wait()?;
    group_result.map(|()| status)
}

#[cfg(all(not(target_os = "wasi"), not(unix)))]
pub(crate) fn terminate_and_reap(child: &mut Child) -> std::io::Result<ExitStatus> {
    child.kill()?;
    child.wait()
}
