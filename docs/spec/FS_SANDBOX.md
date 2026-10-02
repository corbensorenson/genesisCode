# Filesystem Capability Sandbox v0.2

This document is **normative** for the built-in filesystem capabilities:

- `io/fs::stat`
- `io/fs::list`
- `io/fs::mkdir`
- `io/fs::remove`
- `io/fs::rename`
- `io/fs::read`
- `io/fs::write`

These capabilities are deny-by-default and must be explicitly allowed by `caps.toml`.

## Base Directory (`base_dir`)

For `io/fs::*` operations, the capability policy may specify a `base_dir` (string path).

- When loading `caps.toml` from disk, relative `base_dir` paths are resolved relative to the directory containing the `caps.toml` file.
- At runtime, the native runner canonicalizes and opens `base_dir` as a capability directory. Stable WASI lexically normalizes the configured root and opens its directory descriptor; subsequent payload traversal is descriptor-relative.
  Failure to open the configured root is an error. Filesystem operations use that held
  directory and relative directory handles; an ambient resolved pathname is not operation authority.

If `base_dir` is not provided, the runner uses the current working directory as the base directory (this is strongly discouraged for production).

## Input Path Validation

Filesystem effect payloads are maps with op-specific required fields:

- `io/fs::{stat,list,mkdir,remove,read,write}`:
  - `:path` (string)
- `io/fs::rename`:
  - `:from` (string)
  - `:to` (string)

Validation rules:

- Input is valid UTF-8 normalized to Unicode 17 NFC.
- `.` alone names the sandbox base. Every other input is a non-empty, base-relative sequence of
  non-empty components separated by `/` on every host.
- Absolute paths, drive prefixes, backslashes, empty components, `.`, and `..` components are
  rejected before filesystem access.
- Path identity is case-sensitive and locale-independent. The runtime never folds requested case,
  even when the host filesystem does.

These language-facing rules are frozen by `docs/spec/TEXT_PATH_PROFILE_v0.1.md`. Absolute
`base_dir` values remain policy configuration and never enter a language request or response.

## Read (`io/fs::read`)

Read follows inside-root ancestor and final links, then opens through the held capability
root. Relative links are interpreted from their containing directory. Parent components in link targets are applied after preceding components have been physically resolved; lexical collapse must not change which inside-root file is selected. Directory-required suffixes and non-directory ancestors retain host filesystem semantics. Absolute inside links
are reduced to a root-relative name before the capability open, including alternate host
spellings of the configured root. An escaping link or a traversal cycle is rejected; at most
40 links are followed. The actual open remains root-relative if an ancestor changes after
resolution. Stable WASI uses safe descriptor-relative operations with no-follow ancestor and final read opens. Symlink traversal remains denied on that profile; it does not inherit native inside-link support.

## Write (`io/fs::write`)

Write payload additionally contains:

- `:data` (bytes or string): bytes are written as-is; strings are UTF-8 bytes.

Write resolves and authorizes existing ancestors before creating any missing parent.
The final entry is opened relative to a held parent directory with no final-link following;
a final symlink is rejected. `create_dirs = true` permits rooted parent creation after
preflight. The runner writes through the opened file and synchronizes it before success.
This operation does not promise atomic whole-file replacement on a mid-write I/O failure.

Package lock, pins, snapshot, patch and conflict document replacement uses a separate typed
`AtomicWriteTarget`. Preparation is read-only and grants no ambient-path conversion. Legacy absolute document destinations inside the configured root remain accepted; their parent is reduced to a root-relative name without following the final entry. Built-in effect payloads retain their relative-only rule. The
writer acquires an exclusive temporary file in the held destination parent, tries at most
1024 occupied slots, writes and synchronizes bytes, then renames that entry in the same parent.
A final destination link is replaced as an entry; its target is untouched. Replacement errors
remove the owned temporary output. A cleanup error is explicit. Crash durability of the
parent directory and recovery after abrupt termination are not established by this protocol.

## Stat (`io/fs::stat`)

Stat resolves ancestors under the held root, allows missing targets, and observes the final
entry without following its link. A dangling or outside-pointing final link has kind `symlink`.

Response envelope (data map):
- `:path` (string, path relative to `base_dir` when possible)
- `:exists` (bool)
- `:kind` (`file|dir|symlink|other|missing`)
- `:len-bytes` (int)
- `:readonly` (bool)

## List (`io/fs::list`)

List follows read-path sandbox rules, opens the directory through the capability root, and
observes each final directory entry without following its link.

Response envelope:
- vector of entry maps, deterministically sorted by canonical term order
- each entry map contains `:name`, `:path`, `:kind`, `:len-bytes`

Names and paths are strict UTF-8 normalized to NFC and use `/`. A non-UTF-8 entry returns trusted
sealed `core/path-encoding-error`. If distinct host names normalize to the same response identity,
the operation returns trusted sealed `core/path-collision-error`; no lossy replacement or silent
merge is allowed.

## Mkdir (`io/fs::mkdir`)

Payload fields:
- `:path` (string)
- optional `:parents` (bool, default `true`)

When `:parents` is true, parent directories are created recursively.

## Remove (`io/fs::remove`)

Payload fields:
- `:path` (string)
- optional `:recursive` (bool, default `false`)

Behavior:
- files/symlinks are unlinked as final entries without dereferencing the link, including
  dangling, directory and outside-pointing links; link targets retain their contents
- directories require `:recursive true` for recursive removal
- missing paths, including missing ancestors, are treated as deterministic no-op success
- `.` cannot be removed or used as either rename entry; it names the capability root, not a child

## Rename (`io/fs::rename`)

Payload fields:
- `:from` (string)
- `:to` (string)
- optional `:overwrite` (bool, default `false`)

Behavior:
- both ancestor paths are resolved under the held root before destination parent creation
- final entries are not dereferenced; rename moves/replaces the entries themselves
- if `create_dirs = true`, destination parent directories may be created after preflight
- overwrite uses one host rename, without pre-removing the destination
- identical paths and same-inode aliases inherit the host's atomic rename behavior; successful
  no-op replacement preserves that inode and its contents
- file/directory mismatches, occupied directory replacement, missing source and cross-device
  errors return failure without a destructive copy/delete fallback
- no-overwrite uses atomic no-replace rename on Linux/Android and Apple hosts; destination
  existence returns the existing policy error, including an occupied final link
- hosts lacking the atomic no-replace primitive return explicit `Unsupported` before creating
  parents; Windows and other hosts are not silently given a check-then-rename fallback. Stable
  WASI supports atomic overwrite but rejects no-overwrite before parent creation

## Remaining Scope And Qualification

The built-in operations and typed document writer use a held capability directory. Replacing
an ancestor name with an escaping link cannot turn a later relative operation into ambient
outside-root I/O. Once a parent directory is opened, operations refer to that directory object;
renaming its visible name does not change the handle's authority.

Legacy pathname-returning adapters remain for package/module reads, GPK streaming, pins reads/locks,
quarantine/store integration and external process APIs. Their native preflight now authorizes
before rooted parent creation, but a returned `PathBuf` still has a check/open race. They are
transitional adapters, not equivalent to the capability operations. F02 remains open until all
consumers have the agreed boundary and independent acceptance.

Rooted parent creation and recursive removal can make partial progress before a later I/O
error. Stable-WASI recursive removal admits at most 256 directory levels and returns an explicit error before descending further; this bounds host call-stack use. The depth error does not promise rollback of entries already removed. Atomic rename preserves entries on host rejection, but cancellation/crash recovery,
cross-device controls, complete denied-operation rollback, hostile coequal writers, and host
qualification require their separate evidence. The current native controls establish local
behavior on their named host; source compilation is not WASI/Windows runtime qualification.
This specification does not promote F02-F04 or supply an OS process sandbox.

Stable-WASI filesystem effects and typed document replacements use the existing pinned
`rustix` dependency and stable Rust descriptor ownership. `cap-std`/`cap-fs-ext` remain
native-only because their WASI filesystem-time dependency requires an unstable Rust API.
WASI `:readonly` remains false, matching stable Rust metadata because Preview1 filestat has no Unix permission bits; it never grants write authority.
Each ancestor is opened separately with `NOFOLLOW`; read-only preparation examines existing
ancestors before any creation, and parent creation reopens each new entry without following
links. Stat, list metadata, unlink and atomic overwrite inspect or operate on final entries
without dereferencing links. Document replacement retains the same bounded exclusive
acquisition, write/synchronization, same-parent rename and failure-cleanup protocol.

Actual WASI controls must execute the compiled target, exercise admitted and denied operations,
and independently compare host entry identities/content. Native emulation and target compilation
alone are insufficient. These local controls restore the tested operations but do not qualify
all hosts or close F02-F04. The WASI CLI's bootstrap/path-based transport and remaining legacy
consumers stay separately governed. No-overwrite rename remains explicitly unsupported on this
profile until an atomic primitive and its evidence exist.
