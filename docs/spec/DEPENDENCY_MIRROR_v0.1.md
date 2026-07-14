<!-- genesis-doc: normative -->
# GenesisCode Dependency Mirror Contract v0.1

## Authority and scope

`genesis.dependency-mirror.json` is the sole policy authority for repository dependency acquisition and offline reconstruction. `Cargo.lock`, `tools/genesis-evidence-verifier/Cargo.lock`, and `package-lock.json` remain the package-resolution authorities. The mirror policy closes the set of those authorities, their manifests, accepted registry identities, resource bounds, offline checks, and host network-isolation backends.

The current repository has two live dependency ecosystems:

1. The root Cargo workspace and the independently locked evidence-verifier Cargo workspace.
2. The npm development workspace rooted at `package.json` and `package-lock.json`.

Pinned host tools, Rust targets, platform SDKs, Wasmtime, Lean/Lake, browsers, simulators, and physical-device helpers remain prerequisites under `genesis.prerequisites.json`. A dependency mirror MUST NOT claim to acquire or attest those prerequisites. Browser binaries are separately installed content and are outside v0.1 of this mirror contract.

## Fetch phase

`scripts/update_dependency_mirror.sh` is the only repository entry point authorized to acquire dependency bytes. It MUST:

- validate the closed policy and every authority before network access;
- reject duplicate JSON keys, floating-point numbers, path aliases, symlink authorities, unlocked Cargo packages, non-crates.io Cargo sources, npm packages without SHA-512 SRI, npm credentials, npm query strings or fragments, npm redirects, and npm URLs outside the exact declared HTTPS origin;
- invoke Cargo directly, without a shell string, using `cargo vendor --locked --versioned-dirs` over both declared workspaces;
- fetch only npm URLs present in the lockfile and verify each body against its declared SHA-512 SRI before retention;
- enforce the declared per-object, aggregate, file-count, expanded-size, and archive-size bounds;
- normalize the Cargo directory source into deterministic USTAR metadata and a fixed-metadata gzip stream;
- emit a closed generated manifest containing all authority hashes, package identities, upstream checksums or SRI values, logical tree identity, payload hashes, byte counts, and the exact offline check set;
- derive the mirror ID as SHA-256 of the canonical generated-manifest bytes; and
- install only into a new `<mirror-root>/sha256-<mirror-id>` directory, reusing an existing byte-identical mirror but never replacing one.

The fetch phase may use existing package caches, but cache contents are not evidence. Only verified bytes retained by the generated mirror manifest are authoritative. Partial staging directories are temporary and MUST be removed on failure.

## Mirror identity

The generated `manifest.json` uses `genesis/dependency-mirror-manifest-v0.1`. JSON is UTF-8, duplicate-key-free, integer-only, sorted by identity, and serialized with sorted object keys and a single trailing newline. It contains no absolute host path, timestamp, hostname, username, credential, environment value, or mutable `latest` pointer.

Every retained payload is addressed by SHA-256. npm tarballs are additionally named by their lockfile SHA-512 digest. The Cargo archive binds a logical Merkle-style tree digest over normalized relative path, entry kind, executable bit, byte length, and file SHA-256. The mirror directory name MUST equal the SHA-256 of canonical `manifest.json` bytes.

## Verification and extraction

Mirror verification is read-only and MUST recompute the mirror ID, authority hashes, every payload hash and size, package closure, and logical Cargo tree after bounded extraction. Extraction MUST reject absolute paths, `..`, path aliases, duplicate archive members, links, devices, FIFOs, sockets, unsupported metadata, excessive files, and zip/tar bombs before a build can begin.

Extracted Cargo sources are rebuildable state under `.genesis/build/dependency-mirrors/`; they are not retained evidence and may be removed by deterministic cleanup. The compressed mirror under `.genesis/dependency-mirrors/` is retained dependency state and MUST require explicit cleanup selection under R0.4.e.

## Offline phase

`scripts/test_offline_dependency_mirror.sh` MUST start from a clean materialization containing only Git-tracked and unignored source files. It MUST use empty Cargo and npm caches, a separate target directory, the installed prerequisite toolchain, and only the verified mirror payloads. The original source tree and mirror MUST remain unchanged.

The following checks are mandatory and closed by policy:

- root workspace `cargo check --workspace --all-targets --locked --offline`;
- root `cargo build -p gc_cli --locked --offline`;
- standalone verifier `cargo check --all-targets --locked --offline`;
- npm `ci --offline --ignore-scripts --no-audit --no-fund` against a temporary lockfile whose `resolved` fields point at verified mirror blobs; and
- a Node import of the pinned Playwright package.

Cargo MUST replace crates.io with the extracted directory source. npm lifecycle scripts and Playwright browser downloads MUST remain disabled. Proxy variables MUST point at a closed local endpoint as defense in depth, but proxy poisoning alone is not network isolation.

## Network denial

Every offline command runs inside a host-enforced network boundary:

- Tier-1 Darwin uses `sandbox-exec` with `deny network*`.
- Tier-1 Linux uses a new network namespace through unprivileged `unshare` or non-interactive privileged `unshare` with the invoking uid/gid restored.
- Windows currently fails closed as unsupported; it MUST NOT emit passing offline evidence until an equivalent kernel-enforced backend and CI lane exist.

Before dependency execution, the boundary MUST prove itself by denying a connection to a live loopback listener. A missing backend, inconclusive timeout, successful connection, or diagnostic other than an OS-level denial fails the profile. Cargo/npm offline flags are a second independent layer, not a substitute for this canary.

## Evidence and CI

`scripts/check_dependency_mirror_contract.sh` is the fast, read-only conformance gate. It validates policy/source closure, deterministic parsing and rendering, network-isolation selection, and adversarial fixtures without fetching packages or retaining a mirror.

Full CI prepares a mirror once with network enabled, then runs the clean offline phase with the kernel network boundary enabled. Release evidence MUST record the policy hash, generated mirror ID, authority hashes, isolation backend, canary result, commands, exit statuses, and resulting artifact hashes under the Genesis evidence profile. A local ignored mirror or an E0 run is not immutable release evidence by itself.

## Non-goals

This contract does not vendor the Rust toolchain, native SDKs, browser binaries, proof assistants, or external device images. Reproducible acquisition of those prerequisite distributions belongs to later bootstrap, release, and platform work. It also does not permit offline mode to weaken lock checks, skip the independent verifier, share a pre-populated package cache, or silently fall back to network access.
