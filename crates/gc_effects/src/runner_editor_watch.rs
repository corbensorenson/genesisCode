use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use gc_coreform::{Term, TermOrdKey};

use crate::runner_io_ops::path_to_slash;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct FileStamp {
    modified_ns: u128,
    len: u64,
}

#[cfg(not(target_os = "wasi"))]
#[derive(Debug)]
struct NotifyWatchBackend {
    _watcher: notify::RecommendedWatcher,
    rx: std::sync::mpsc::Receiver<Result<notify::Event, notify::Error>>,
}

#[derive(Debug)]
enum WatchBackend {
    Polling,
    #[cfg(not(target_os = "wasi"))]
    Incremental(NotifyWatchBackend),
}

#[derive(Debug)]
pub(super) struct WatchState {
    root: String,
    globs: Vec<String>,
    logical_root: Option<String>,
    root_canon: PathBuf,
    snapshot: BTreeMap<String, FileStamp>,
    backend: WatchBackend,
}

impl WatchState {
    pub(super) fn root(&self) -> &str {
        &self.root
    }
}

pub(super) fn watch_state_new(root: &str, globs: Vec<String>) -> Result<WatchState, String> {
    let root_path = PathBuf::from(root);
    let root_canon = std::fs::canonicalize(&root_path)
        .map_err(|err| format!("watch root canonicalize failed: {err}"))?;
    if !root_canon.is_dir() {
        return Err(format!(
            "watch root is not a directory: {}",
            root_canon.display()
        ));
    }

    let logical_root = normalized_logical_root(root);
    let snapshot = scan_watch_snapshot(&root_canon, logical_root.as_deref(), &globs)?;
    let backend = make_watch_backend(&root_canon);

    Ok(WatchState {
        root: root.to_string(),
        globs,
        logical_root,
        root_canon,
        snapshot,
        backend,
    })
}

pub(super) fn watch_state_poll(watch: &mut WatchState) -> Result<Vec<Term>, String> {
    #[cfg(not(target_os = "wasi"))]
    {
        let root_canon = watch.root_canon.clone();
        let logical_root = watch.logical_root.clone();
        let globs = watch.globs.clone();
        let mut degrade_to_polling = false;

        if let WatchBackend::Incremental(backend) = &mut watch.backend {
            match poll_incremental(
                &root_canon,
                logical_root.as_deref(),
                &globs,
                &mut watch.snapshot,
                backend,
            ) {
                Ok(events) => return Ok(events),
                Err(_) => {
                    degrade_to_polling = true;
                }
            }
        }

        if degrade_to_polling {
            watch.backend = WatchBackend::Polling;
        }
    }

    let next = scan_watch_snapshot(
        &watch.root_canon,
        watch.logical_root.as_deref(),
        &watch.globs,
    )?;
    let events = diff_watch_snapshots(&watch.snapshot, &next);
    watch.snapshot = next;
    Ok(events)
}

fn normalized_logical_root(root: &str) -> Option<String> {
    let trimmed = root.trim();
    if trimmed.is_empty() || trimmed == "." {
        None
    } else {
        Some(trimmed.trim_end_matches('/').to_string())
    }
}

fn make_watch_backend(root_canon: &Path) -> WatchBackend {
    #[cfg(not(target_os = "wasi"))]
    {
        if let Ok(backend) = try_make_incremental_backend(root_canon) {
            return WatchBackend::Incremental(backend);
        }
    }
    WatchBackend::Polling
}

#[cfg(not(target_os = "wasi"))]
fn try_make_incremental_backend(root_canon: &Path) -> Result<NotifyWatchBackend, String> {
    use notify::Watcher as _;

    let (tx, rx) = std::sync::mpsc::channel::<Result<notify::Event, notify::Error>>();
    let mut watcher = notify::recommended_watcher(move |res| {
        let _ = tx.send(res);
    })
    .map_err(|e| format!("notify watcher init failed: {e}"))?;
    watcher
        .watch(root_canon, notify::RecursiveMode::Recursive)
        .map_err(|e| format!("notify watch registration failed: {e}"))?;
    Ok(NotifyWatchBackend {
        _watcher: watcher,
        rx,
    })
}

fn scan_watch_snapshot(
    root_canon: &Path,
    logical_root: Option<&str>,
    globs: &[String],
) -> Result<BTreeMap<String, FileStamp>, String> {
    let mut files = Vec::new();
    collect_files_recursive(root_canon, &mut files)
        .map_err(|err| format!("watch root scan failed: {err}"))?;
    files.sort();

    let mut snapshot = BTreeMap::new();
    for file in files {
        let Ok(rel) = file.strip_prefix(root_canon) else {
            continue;
        };
        let rel_slash = path_to_slash(rel);
        if rel_slash.is_empty() || !glob_matches_any(globs, &rel_slash) {
            continue;
        }
        let Ok(md) = std::fs::metadata(&file) else {
            continue;
        };
        if !md.is_file() {
            continue;
        }
        let logical = logical_from_rel(logical_root, &rel_slash);
        snapshot.insert(logical, file_stamp_from_metadata(&md));
    }
    Ok(snapshot)
}

#[cfg(not(target_os = "wasi"))]
fn poll_incremental(
    root_canon: &Path,
    logical_root: Option<&str>,
    globs: &[String],
    snapshot: &mut BTreeMap<String, FileStamp>,
    backend: &mut NotifyWatchBackend,
) -> Result<Vec<Term>, String> {
    use std::sync::mpsc::RecvTimeoutError;
    use std::time::Duration;

    let mut changed_paths = Vec::new();
    match backend.rx.recv_timeout(Duration::from_millis(3)) {
        Ok(Ok(event)) => {
            changed_paths.extend(event.paths);
        }
        Ok(Err(err)) => return Err(format!("notify watch event error: {err}")),
        Err(RecvTimeoutError::Timeout) => return Ok(Vec::new()),
        Err(RecvTimeoutError::Disconnected) => {
            return Err("notify watch channel disconnected".to_string());
        }
    }
    while let Ok(next) = backend.rx.try_recv() {
        match next {
            Ok(event) => changed_paths.extend(event.paths),
            Err(err) => return Err(format!("notify watch event error: {err}")),
        }
    }

    let candidates =
        collect_candidate_logical_paths(root_canon, logical_root, globs, snapshot, changed_paths);
    if candidates.is_empty() {
        return Ok(Vec::new());
    }
    Ok(apply_incremental_delta(
        root_canon,
        logical_root,
        snapshot,
        candidates,
    ))
}

#[cfg(not(target_os = "wasi"))]
fn collect_candidate_logical_paths(
    root_canon: &Path,
    logical_root: Option<&str>,
    globs: &[String],
    snapshot: &BTreeMap<String, FileStamp>,
    changed_paths: Vec<PathBuf>,
) -> BTreeSet<String> {
    let mut changed_files = BTreeSet::<String>::new();
    let mut changed_dirs = BTreeSet::<String>::new();

    for raw in changed_paths {
        let abs = if raw.is_absolute() {
            raw
        } else {
            root_canon.join(raw)
        };
        let Some(rel_slash) = rel_slash_under_root(root_canon, &abs) else {
            continue;
        };

        match std::fs::metadata(&abs) {
            Ok(md) if md.is_dir() => {
                changed_dirs.insert(rel_slash);
            }
            Ok(md) if md.is_file() => {
                changed_files.insert(rel_slash);
            }
            _ => {
                let logical = logical_from_rel(logical_root, &rel_slash);
                if snapshot.contains_key(&logical) {
                    changed_files.insert(rel_slash.clone());
                }
                let logical_prefix = if logical.is_empty() {
                    String::new()
                } else {
                    format!("{logical}/")
                };
                if logical_prefix.is_empty()
                    || snapshot
                        .keys()
                        .any(|path| path.starts_with(&logical_prefix))
                {
                    changed_dirs.insert(rel_slash);
                }
            }
        }
    }

    let mut candidates = BTreeSet::new();

    for rel in changed_files {
        if rel.is_empty() || !glob_matches_any(globs, &rel) {
            continue;
        }
        candidates.insert(logical_from_rel(logical_root, &rel));
    }

    for rel_dir in changed_dirs {
        let dir_abs = if rel_dir.is_empty() {
            root_canon.to_path_buf()
        } else {
            root_canon.join(&rel_dir)
        };
        let dir_logical = logical_from_rel(logical_root, &rel_dir);
        let dir_prefix = if dir_logical.is_empty() {
            String::new()
        } else {
            format!("{dir_logical}/")
        };
        if dir_prefix.is_empty() {
            candidates.extend(snapshot.keys().cloned());
        } else {
            for path in snapshot.keys() {
                if path.starts_with(&dir_prefix) {
                    candidates.insert(path.clone());
                }
            }
        }

        if dir_abs.is_dir() {
            let mut files = Vec::new();
            if collect_files_recursive(&dir_abs, &mut files).is_ok() {
                files.sort();
                for file in files {
                    let Some(rel_file) = rel_slash_under_root(root_canon, &file) else {
                        continue;
                    };
                    if rel_file.is_empty() || !glob_matches_any(globs, &rel_file) {
                        continue;
                    }
                    candidates.insert(logical_from_rel(logical_root, &rel_file));
                }
            }
        }
    }

    candidates
}

#[cfg(not(target_os = "wasi"))]
fn apply_incremental_delta(
    root_canon: &Path,
    logical_root: Option<&str>,
    snapshot: &mut BTreeMap<String, FileStamp>,
    candidates: BTreeSet<String>,
) -> Vec<Term> {
    let mut events = Vec::new();

    for logical in candidates {
        let old = snapshot.get(&logical).copied();
        let abs = abs_path_for_logical(root_canon, logical_root, &logical);
        let new = std::fs::metadata(&abs)
            .ok()
            .filter(|md| md.is_file())
            .map(|md| file_stamp_from_metadata(&md));
        match (old, new) {
            (None, Some(new_stamp)) => {
                snapshot.insert(logical.clone(), new_stamp);
                events.push(watch_event(":create", &logical, new_stamp.modified_ns));
            }
            (Some(old_stamp), Some(new_stamp)) if old_stamp != new_stamp => {
                snapshot.insert(logical.clone(), new_stamp);
                events.push(watch_event(":modify", &logical, new_stamp.modified_ns));
            }
            (Some(old_stamp), None) => {
                snapshot.remove(&logical);
                events.push(watch_event(":delete", &logical, old_stamp.modified_ns));
            }
            _ => {}
        }
    }

    events
}

fn rel_slash_under_root(root_canon: &Path, abs: &Path) -> Option<String> {
    let rel = abs.strip_prefix(root_canon).ok()?;
    Some(path_to_slash(rel))
}

fn logical_from_rel(logical_root: Option<&str>, rel_slash: &str) -> String {
    if rel_slash.is_empty() {
        return logical_root.unwrap_or_default().to_string();
    }
    match logical_root {
        Some(prefix) => format!("{prefix}/{rel_slash}"),
        None => rel_slash.to_string(),
    }
}

fn abs_path_for_logical(root_canon: &Path, logical_root: Option<&str>, logical: &str) -> PathBuf {
    let rel = match logical_root {
        Some(prefix) if !prefix.is_empty() => logical
            .strip_prefix(prefix)
            .and_then(|s| s.strip_prefix('/'))
            .unwrap_or(logical),
        _ => logical,
    };
    root_canon.join(rel)
}

fn collect_files_recursive(root: &Path, out: &mut Vec<PathBuf>) -> std::io::Result<()> {
    let mut entries = std::fs::read_dir(root)?.collect::<Result<Vec<_>, _>>()?;
    entries.sort_by_key(|e| e.file_name());
    for entry in entries {
        let ft = entry.file_type()?;
        let p = entry.path();
        if ft.is_dir() {
            collect_files_recursive(&p, out)?;
        } else if ft.is_file() {
            out.push(p);
        }
    }
    Ok(())
}

fn diff_watch_snapshots(
    old: &BTreeMap<String, FileStamp>,
    new: &BTreeMap<String, FileStamp>,
) -> Vec<Term> {
    let mut events = Vec::new();
    for (path, new_stamp) in new {
        match old.get(path) {
            None => events.push(watch_event(":create", path, new_stamp.modified_ns)),
            Some(old_stamp) if old_stamp != new_stamp => {
                events.push(watch_event(":modify", path, new_stamp.modified_ns))
            }
            _ => {}
        }
    }
    for (path, old_stamp) in old {
        if !new.contains_key(path) {
            events.push(watch_event(":delete", path, old_stamp.modified_ns));
        }
    }
    events
}

fn watch_event(kind: &str, path: &str, stamp_ns: u128) -> Term {
    map_term(vec![
        (":kind", Term::symbol(kind)),
        (":path", Term::Str(path.to_string())),
        (":stamp", Term::Int((u128_to_i64(stamp_ns)).into())),
    ])
}

fn file_stamp_from_metadata(md: &std::fs::Metadata) -> FileStamp {
    let modified_ns = md
        .modified()
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    FileStamp {
        modified_ns,
        len: md.len(),
    }
}

fn u128_to_i64(x: u128) -> i64 {
    if x > i64::MAX as u128 {
        i64::MAX
    } else {
        x as i64
    }
}

fn glob_matches_any(globs: &[String], rel_slash: &str) -> bool {
    if globs.is_empty() {
        return true;
    }
    globs.iter().any(|g| wildcard_match(g, rel_slash))
}

fn wildcard_match(pattern: &str, text: &str) -> bool {
    if pattern == "*" {
        return true;
    }
    if !pattern.contains('*') {
        return pattern == text;
    }
    let parts = pattern.split('*').collect::<Vec<_>>();
    let starts_anchored = !pattern.starts_with('*');
    let ends_anchored = !pattern.ends_with('*');
    let mut idx = 0usize;

    for (i, part) in parts.iter().enumerate() {
        if part.is_empty() {
            continue;
        }
        if i == 0 && starts_anchored {
            if !text[idx..].starts_with(part) {
                return false;
            }
            idx += part.len();
            continue;
        }
        let Some(found) = text[idx..].find(part) else {
            return false;
        };
        idx += found + part.len();
    }
    if ends_anchored && let Some(last) = parts.last() {
        return text.ends_with(last);
    }
    true
}

fn map_term(items: Vec<(&str, Term)>) -> Term {
    let mut map = BTreeMap::new();
    for (k, v) in items {
        map.insert(TermOrdKey(Term::symbol(k)), v);
    }
    Term::Map(map)
}
