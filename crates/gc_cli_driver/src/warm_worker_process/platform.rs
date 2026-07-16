use std::collections::HashSet;
#[cfg(target_os = "linux")]
use std::fs;
use std::process::Command;

use crate::warm_worker::WorkerJob;

#[cfg(unix)]
pub(super) fn configure_process(command: &mut Command, job: &WorkerJob) {
    use std::os::unix::process::CommandExt;

    command.process_group(0);
    let cpu_seconds = u64::try_from(job.limits.max_cpu.as_millis())
        .unwrap_or(u64::MAX)
        .div_ceil(1000)
        .max(1);
    let file_bytes = job.limits.max_disk_bytes;
    // SAFETY: this hook performs only async-signal-safe setrlimit calls before exec.
    unsafe {
        command.pre_exec(move || {
            let set = |resource, value: u64| -> std::io::Result<()> {
                let limit = libc::rlimit {
                    rlim_cur: value as libc::rlim_t,
                    rlim_max: value as libc::rlim_t,
                };
                if libc::setrlimit(resource, &limit) == 0 {
                    Ok(())
                } else {
                    Err(std::io::Error::last_os_error())
                }
            };
            set(libc::RLIMIT_CPU, cpu_seconds)?;
            // max_heap_bytes is aggregate resident memory for the complete process
            // tree. RLIMIT_AS measures per-process virtual address space and can
            // reject work below that contract before the audited tree monitor sees it.
            set(libc::RLIMIT_FSIZE, file_bytes)?;
            Ok(())
        });
    }
}

#[cfg(not(unix))]
pub(super) fn configure_process(_command: &mut Command, _job: &WorkerJob) {}

#[cfg(unix)]
pub(super) fn kill_process_tree(process_id: u32) -> std::io::Result<()> {
    let process_id = i32::try_from(process_id).map_err(|_| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "invalid worker process id",
        )
    })?;
    let mut first_error = None;
    let root = process_id as u32;
    let initial = process_tree_ids(root);
    for member in &initial {
        let Ok(member) = i32::try_from(*member) else {
            continue;
        };
        if unsafe { libc::kill(member, libc::SIGSTOP) } != 0 {
            let error = std::io::Error::last_os_error();
            if error.raw_os_error() != Some(libc::ESRCH) && first_error.is_none() {
                first_error = Some(error);
            }
        }
    }
    let mut members = process_tree_ids(root).into_iter().collect::<HashSet<_>>();
    members.extend(initial);
    for member in &members {
        if let Ok(member) = i32::try_from(*member) {
            let _ = unsafe { libc::kill(member, libc::SIGSTOP) };
        }
    }
    members.extend(process_tree_ids(root));
    let mut descendants = members
        .into_iter()
        .filter(|pid| *pid != root)
        .collect::<Vec<_>>();
    descendants.reverse();
    for descendant in descendants {
        let Ok(descendant) = i32::try_from(descendant) else {
            continue;
        };
        if unsafe { libc::kill(descendant, libc::SIGKILL) } != 0 {
            let error = std::io::Error::last_os_error();
            if error.raw_os_error() != Some(libc::ESRCH) && first_error.is_none() {
                first_error = Some(error);
            }
        }
    }
    // The leader retains its own group; this also catches descendants that did not regroup.
    if unsafe { libc::kill(-process_id, libc::SIGKILL) } != 0 {
        let error = std::io::Error::last_os_error();
        if error.raw_os_error() != Some(libc::ESRCH) && first_error.is_none() {
            first_error = Some(error);
        }
    }
    if unsafe { libc::kill(process_id, libc::SIGKILL) } != 0 {
        let error = std::io::Error::last_os_error();
        if error.raw_os_error() != Some(libc::ESRCH) && first_error.is_none() {
            first_error = Some(error);
        }
    }
    match first_error {
        Some(error) => Err(error),
        None => Ok(()),
    }
}

#[cfg(not(unix))]
pub(super) fn kill_process_tree(_process_id: u32) -> std::io::Result<()> {
    Ok(())
}

#[cfg(unix)]
pub(super) fn cpu_millis_snapshot() -> Option<u64> {
    let mut usage = std::mem::MaybeUninit::<libc::rusage>::uninit();
    // SAFETY: getrusage initializes the provided rusage value on success.
    if unsafe { libc::getrusage(libc::RUSAGE_CHILDREN, usage.as_mut_ptr()) } != 0 {
        return None;
    }
    // SAFETY: success above guarantees initialization.
    let usage = unsafe { usage.assume_init() };
    let user = (usage.ru_utime.tv_sec as u64)
        .saturating_mul(1000)
        .saturating_add((usage.ru_utime.tv_usec as u64) / 1000);
    let system = (usage.ru_stime.tv_sec as u64)
        .saturating_mul(1000)
        .saturating_add((usage.ru_stime.tv_usec as u64) / 1000);
    Some(user.saturating_add(system))
}

#[cfg(target_os = "macos")]
#[repr(C)]
struct ProcTaskInfo {
    virtual_size: u64,
    resident_size: u64,
    total_user: u64,
    total_system: u64,
    threads_user: u64,
    threads_system: u64,
    policy: i32,
    faults: i32,
    pageins: i32,
    cow_faults: i32,
    messages_sent: i32,
    messages_received: i32,
    syscalls_mach: i32,
    syscalls_unix: i32,
    context_switches: i32,
    thread_count: i32,
    running_thread_count: i32,
    priority: i32,
}

#[cfg(target_os = "macos")]
#[link(name = "proc")]
unsafe extern "C" {
    fn proc_pidinfo(
        pid: i32,
        flavor: i32,
        arg: u64,
        buffer: *mut std::ffi::c_void,
        buffer_size: i32,
    ) -> i32;
    fn proc_listchildpids(pid: i32, buffer: *mut std::ffi::c_void, buffer_size: i32) -> i32;
}

#[cfg(target_os = "macos")]
fn direct_child_ids(process_id: u32) -> Option<Vec<u32>> {
    let mut ids = vec![0_i32; 256];
    let size = i32::try_from(ids.len().checked_mul(std::mem::size_of::<i32>())?).ok()?;
    let pid = i32::try_from(process_id).ok()?;
    let written = unsafe { proc_listchildpids(pid, ids.as_mut_ptr().cast(), size) };
    if written < 0 {
        return None;
    }
    let count = usize::try_from(written).ok()?.min(ids.len());
    Some(
        ids.into_iter()
            .take(count)
            .filter_map(|pid| u32::try_from(pid).ok())
            .filter(|pid| *pid != 0)
            .collect(),
    )
}

#[cfg(target_os = "macos")]
fn process_tree_ids(process_id: u32) -> Vec<u32> {
    const MAX_TRACKED_PROCESSES: usize = 4096;
    let mut seen = HashSet::from([process_id]);
    let mut pending = vec![process_id];
    while let Some(parent) = pending.pop() {
        let Some(children) = direct_child_ids(parent) else {
            continue;
        };
        for child in children {
            if seen.len() >= MAX_TRACKED_PROCESSES {
                break;
            }
            if seen.insert(child) {
                pending.push(child);
            }
        }
    }
    seen.into_iter().collect()
}

#[cfg(target_os = "macos")]
fn process_info(process_id: u32) -> Option<ProcTaskInfo> {
    const PROC_PIDTASKINFO: i32 = 4;
    let pid = i32::try_from(process_id).ok()?;
    let mut info = std::mem::MaybeUninit::<ProcTaskInfo>::uninit();
    let size = i32::try_from(std::mem::size_of::<ProcTaskInfo>()).ok()?;
    // SAFETY: proc_pidinfo writes at most `size` bytes into the valid buffer.
    let written = unsafe { proc_pidinfo(pid, PROC_PIDTASKINFO, 0, info.as_mut_ptr().cast(), size) };
    if written != size {
        return None;
    }
    // SAFETY: an exact-size successful result initialized the complete structure.
    Some(unsafe { info.assume_init() })
}

#[cfg(target_os = "macos")]
pub(super) fn process_tree_usage(process_id: u32) -> Option<(u64, u64, u64)> {
    let ids = process_tree_ids(process_id);
    let mut resident = 0_u64;
    let mut cpu_nanos = 0_u64;
    for id in &ids {
        let Some(info) = process_info(*id) else {
            continue;
        };
        resident = resident.saturating_add(info.resident_size);
        cpu_nanos = cpu_nanos
            .saturating_add(info.total_user)
            .saturating_add(info.total_system);
    }
    Some((ids.len() as u64, resident, cpu_nanos / 1_000_000))
}

#[cfg(target_os = "linux")]
fn process_tree_ids(process_id: u32) -> Vec<u32> {
    let mut children = std::collections::HashMap::<u32, Vec<u32>>::new();
    let Ok(entries) = fs::read_dir("/proc") else {
        return vec![process_id];
    };
    for entry in entries.flatten() {
        let Ok(pid) = entry.file_name().to_string_lossy().parse::<u32>() else {
            continue;
        };
        let Ok(stat) = fs::read_to_string(entry.path().join("stat")) else {
            continue;
        };
        let Some(end) = stat.rfind(')') else {
            continue;
        };
        let Some(parent) = stat
            .get(end + 2..)
            .and_then(|suffix| suffix.split_whitespace().nth(1))
            .and_then(|field| field.parse::<u32>().ok())
        else {
            continue;
        };
        children.entry(parent).or_default().push(pid);
    }
    let mut seen = HashSet::from([process_id]);
    let mut pending = vec![process_id];
    while let Some(parent) = pending.pop() {
        for child in children.remove(&parent).unwrap_or_default() {
            if seen.len() >= 4096 {
                break;
            }
            if seen.insert(child) {
                pending.push(child);
            }
        }
    }
    seen.into_iter().collect()
}

#[cfg(target_os = "linux")]
pub(super) fn process_tree_usage(process_id: u32) -> Option<(u64, u64, u64)> {
    let ticks = unsafe { libc::sysconf(libc::_SC_CLK_TCK) };
    let page_size = unsafe { libc::sysconf(libc::_SC_PAGESIZE) };
    if ticks <= 0 || page_size <= 0 {
        return None;
    }
    let ids = process_tree_ids(process_id)
        .into_iter()
        .collect::<HashSet<_>>();
    let mut count = 0_u64;
    let mut resident = 0_u64;
    let mut cpu_ticks = 0_u64;
    for entry in fs::read_dir("/proc").ok()?.flatten() {
        if entry.file_name().to_string_lossy().parse::<u32>().is_err() {
            continue;
        }
        let Ok(stat) = fs::read_to_string(entry.path().join("stat")) else {
            continue;
        };
        let Some(end) = stat.rfind(')') else {
            continue;
        };
        let Some(suffix) = stat.get(end + 2..) else {
            continue;
        };
        let fields = suffix.split_whitespace().collect::<Vec<_>>();
        let Ok(pid) = entry.file_name().to_string_lossy().parse::<u32>() else {
            continue;
        };
        if !ids.contains(&pid) {
            continue;
        }
        let Some(user) = fields.get(11).and_then(|field| field.parse::<u64>().ok()) else {
            continue;
        };
        let Some(system) = fields.get(12).and_then(|field| field.parse::<u64>().ok()) else {
            continue;
        };
        let Some(pages) = fields.get(21).and_then(|field| field.parse::<u64>().ok()) else {
            continue;
        };
        count = count.saturating_add(1);
        cpu_ticks = cpu_ticks.saturating_add(user).saturating_add(system);
        resident = resident.saturating_add(pages.saturating_mul(page_size as u64));
    }
    Some((
        count,
        resident,
        cpu_ticks.saturating_mul(1000) / ticks as u64,
    ))
}

#[cfg(not(unix))]
pub(super) fn cpu_millis_snapshot() -> Option<u64> {
    None
}
