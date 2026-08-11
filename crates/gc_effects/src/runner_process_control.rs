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

#[cfg(all(not(target_os = "wasi"), target_os = "macos"))]
fn process_group_has_live_members(process_id: u32) -> std::io::Result<Option<bool>> {
    const PROC_PGRP_ONLY: u32 = 2;
    const INITIAL_PID_CAPACITY: usize = 32;
    const MAX_PID_CAPACITY: usize = 16_384;

    let process_group = i32::try_from(process_id).map_err(|_| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "child process ID exceeds the platform process-group range",
        )
    })?;
    let mut capacity = INITIAL_PID_CAPACITY;
    loop {
        let mut pids = vec![0_i32; capacity];
        let buffer_bytes = i32::try_from(std::mem::size_of_val(pids.as_slice())).map_err(|_| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "process-group inspection buffer exceeds the platform range",
            )
        })?;
        let written_bytes = unsafe {
            libc::proc_listpids(
                PROC_PGRP_ONLY,
                process_id,
                pids.as_mut_ptr().cast(),
                buffer_bytes,
            )
        };
        if written_bytes < 0 {
            return Err(std::io::Error::last_os_error());
        }
        let written_bytes = usize::try_from(written_bytes).map_err(|_| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "process-group inspection returned a negative byte count",
            )
        })?;
        if written_bytes % std::mem::size_of::<libc::pid_t>() != 0 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "process-group inspection returned a partial PID",
            ));
        }
        let count = written_bytes / std::mem::size_of::<libc::pid_t>();
        if count >= capacity {
            if capacity >= MAX_PID_CAPACITY {
                return Err(std::io::Error::other(
                    "process group exceeds the bounded inspection capacity",
                ));
            }
            capacity = capacity.saturating_mul(2).min(MAX_PID_CAPACITY);
            continue;
        }

        for pid in pids.into_iter().take(count).filter(|pid| *pid > 0) {
            let mut info = std::mem::MaybeUninit::<libc::proc_bsdinfo>::zeroed();
            let expected_bytes = i32::try_from(std::mem::size_of::<libc::proc_bsdinfo>())
                .map_err(|_| std::io::Error::other("proc_bsdinfo size exceeds i32"))?;
            let actual_bytes = unsafe {
                libc::proc_pidinfo(
                    pid,
                    libc::PROC_PIDTBSDINFO,
                    0,
                    info.as_mut_ptr().cast(),
                    expected_bytes,
                )
            };
            if actual_bytes == expected_bytes {
                let info = unsafe { info.assume_init() };
                if info.pbi_pgid == process_id && info.pbi_status != libc::SZOMB {
                    return Ok(Some(true));
                }
                continue;
            }

            // The process list and detail query are not atomic. An exited PID is
            // harmless. macOS reports ESRCH for a zombie returned by proc_listpids
            // even while kill(pid, 0) still sees its process-table entry.
            let detail_error = std::io::Error::last_os_error();
            if detail_error.raw_os_error() == Some(libc::ESRCH) {
                continue;
            }
            let probe = unsafe { libc::kill(pid, 0) };
            if probe == 0 {
                return Err(std::io::Error::other(format!(
                    "could not inspect addressable process {pid} in group {process_group}"
                )));
            }
            let error = std::io::Error::last_os_error();
            if error.raw_os_error() != Some(libc::ESRCH) {
                return Err(error);
            }
        }
        return Ok(Some(false));
    }
}

#[cfg(all(not(target_os = "wasi"), target_os = "linux"))]
fn parse_linux_process_stat(source: &str) -> Option<(u8, u32)> {
    let command_end = source.rfind(')')?;
    let mut fields = source.get(command_end + 1..)?.split_whitespace();
    let state = fields.next()?.as_bytes().first().copied()?;
    let _parent = fields.next()?;
    let process_group = fields.next()?.parse::<u32>().ok()?;
    Some((state, process_group))
}

#[cfg(all(not(target_os = "wasi"), target_os = "linux"))]
fn process_group_has_live_members(process_id: u32) -> std::io::Result<Option<bool>> {
    const MAX_PROC_ENTRIES: usize = 65_536;
    const MAX_INSPECTION_TIME: std::time::Duration = std::time::Duration::from_millis(100);

    let started = std::time::Instant::now();
    let mut inspected = 0_usize;
    let mut saw_group_member = false;
    for entry in std::fs::read_dir("/proc")? {
        inspected = inspected.saturating_add(1);
        if inspected > MAX_PROC_ENTRIES || started.elapsed() > MAX_INSPECTION_TIME {
            return Err(std::io::Error::new(
                std::io::ErrorKind::TimedOut,
                "process-group inspection exceeded its deterministic bound",
            ));
        }
        let entry = entry?;
        let Some(pid) = entry
            .file_name()
            .to_str()
            .and_then(|name| name.parse::<u32>().ok())
        else {
            continue;
        };
        let stat = match std::fs::read_to_string(entry.path().join("stat")) {
            Ok(stat) => stat,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(error) if error.kind() == std::io::ErrorKind::PermissionDenied => continue,
            Err(error) => return Err(error),
        };
        let Some((state, process_group)) = parse_linux_process_stat(&stat) else {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("could not parse /proc/{pid}/stat while inspecting bridge group"),
            ));
        };
        if process_group == process_id {
            saw_group_member = true;
            if !matches!(state, b'Z' | b'X' | b'x') {
                return Ok(Some(true));
            }
        }
    }
    // A hidden /proc mount must not be interpreted as proof that the group is
    // empty. Fall back to bounded kill(2) disappearance unless a member was
    // observed and every observed member was non-executing.
    Ok(saw_group_member.then_some(false))
}

#[cfg(all(
    not(target_os = "wasi"),
    unix,
    not(any(target_os = "linux", target_os = "macos"))
))]
fn process_group_has_live_members(_process_id: u32) -> std::io::Result<Option<bool>> {
    Ok(None)
}

#[cfg(not(target_os = "wasi"))]
pub(crate) fn signal_process_tree(process_id: u32) -> std::io::Result<()> {
    kill_process_group(process_id)
}

#[cfg(all(not(target_os = "wasi"), unix))]
fn kill_process_group_until_gone(process_id: u32) -> std::io::Result<()> {
    const MAX_REAP_WAIT: std::time::Duration = std::time::Duration::from_millis(250);
    let process_id = i32::try_from(process_id).map_err(|_| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "child process ID exceeds the platform process-group range",
        )
    })?;
    let started = std::time::Instant::now();
    let deadline = started + MAX_REAP_WAIT;
    let mut last_error;
    let mut sleep_ms = 1_u64;
    loop {
        let result = unsafe { libc::kill(-process_id, libc::SIGKILL) };
        if result == 0 {
            last_error = None;
        } else {
            let error = std::io::Error::last_os_error();
            match error.raw_os_error() {
                Some(libc::ESRCH) => return Ok(()),
                Some(libc::EPERM) => {
                    // macOS may report EPERM while only reparented zombies remain.
                    last_error = Some(error);
                }
                _ => return Err(error),
            }
        }
        if process_group_has_live_members(u32::try_from(process_id).map_err(|_| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "process-group ID cannot be represented as u32",
            )
        })?)?
            == Some(false)
        {
            return Ok(());
        }
        if std::time::Instant::now() >= deadline {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(sleep_ms));
        sleep_ms = (sleep_ms.saturating_mul(2)).min(10);
    }
    Err(last_error.unwrap_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::TimedOut,
            format!(
                "bridge process group remained after {}ms bounded termination",
                started.elapsed().as_millis()
            ),
        )
    }))
}

#[cfg(all(not(target_os = "wasi"), not(unix)))]
fn kill_process_group(_process_id: u32) -> std::io::Result<()> {
    Ok(())
}

#[cfg(all(not(target_os = "wasi"), not(unix)))]
fn kill_process_group_until_gone(process_id: u32) -> std::io::Result<()> {
    kill_process_group(process_id)
}

#[cfg(not(target_os = "wasi"))]
pub(crate) fn terminate_descendants(process_id: u32) -> std::io::Result<()> {
    kill_process_group_until_gone(process_id)
}

#[cfg(all(not(target_os = "wasi"), unix))]
pub(crate) fn terminate_and_reap(child: &mut Child) -> std::io::Result<ExitStatus> {
    let group_result = kill_process_group(child.id());
    if let Err(group_error) = &group_result
        && let Err(child_error) = child.kill()
    {
        return Err(std::io::Error::new(
            child_error.kind(),
            format!(
                "process-group signal failed ({group_error}); leader kill failed ({child_error})"
            ),
        ));
    }
    let status = child.wait()?;
    // A bridge can fork between the first group signal and leader termination.
    // Sweep the now-quiescent group again before pipe readers are joined.
    let residual_result = kill_process_group_until_gone(child.id());
    if let Err(group_error) = &group_result
        && let Err(residual_error) = &residual_result
    {
        return Err(std::io::Error::new(
            residual_error.kind(),
            format!(
                "initial process-group signal failed ({group_error}); final quiescence failed ({residual_error})"
            ),
        ));
    }
    residual_result.map(|()| status)
}

#[cfg(all(not(target_os = "wasi"), not(unix)))]
pub(crate) fn terminate_and_reap(child: &mut Child) -> std::io::Result<ExitStatus> {
    child.kill()?;
    child.wait()
}

#[cfg(all(test, any(target_os = "linux", target_os = "macos")))]
mod tests {
    use super::*;

    #[test]
    fn zombie_only_process_group_is_execution_quiescent() {
        let mut command = Command::new("/bin/sh");
        command.args(["-c", "exit 0"]);
        configure_killable_process(&mut command);
        let mut child = command
            .spawn()
            .expect("spawn short-lived process-group leader");
        let process_id = child.id();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
        loop {
            if process_group_has_live_members(process_id).expect("inspect process group")
                == Some(false)
            {
                break;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "process-group leader did not become a zombie within the test bound"
            );
            std::thread::sleep(std::time::Duration::from_millis(1));
        }

        let started = std::time::Instant::now();
        kill_process_group_until_gone(process_id)
            .expect("zombie-only process group must be execution-quiescent");
        assert!(
            started.elapsed() < std::time::Duration::from_millis(100),
            "zombie-only quiescence detection exceeded its local bound"
        );
        child.wait().expect("reap process-group leader");
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn linux_stat_parser_handles_spaces_and_parentheses_in_command_names() {
        assert_eq!(
            parse_linux_process_stat("123 (bridge worker (one)) Z 99 123 123 0"),
            Some((b'Z', 123))
        );
        assert_eq!(
            parse_linux_process_stat("124 (bridge worker) S 99 123 123 0"),
            Some((b'S', 123))
        );
        assert_eq!(parse_linux_process_stat("malformed"), None);
    }
}
