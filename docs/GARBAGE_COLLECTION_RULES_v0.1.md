# Garbage Collection Rules v0.1

Safe pruning for the local Genesis store using reachability closures.

Goal: prevent unbounded growth while preserving correctness, reproducibility, and policy guarantees.

GC is mark-and-sweep over a content-addressed store where "live" objects are exactly those
reachable from an explicit root set (refs, locks, pins), using the same reachability closure rules
as export/publish.

GC runs outside the kernel as effectful operations (file deletions) and must be logged/auditable.

## 0) Principles

- Never break reproducibility: GC must not delete artifacts needed to run/replay installed packages.
- Explicit retention beats guessing: preserve only what is reachable from explicit roots + pins.
- Policy-aware retention: evidence/history retention is policy-controlled.
- Quarantine is recommended during early development to reduce accidental loss.

## 1) Root sets (normative defaults)

### 1.1 Hard roots

Hard roots define the minimum live set:

- all local refs (all `refs/*` heads)
- all workspace locks (`genesis.lock`):
  - all `locked.*.commit` and `locked.*.snapshot`
  - optional root workspace snapshot/commit if used
- explicit pins (`.genesis/pins.toml` or equivalent)

### 1.2 Soft roots (optional)

Soft roots preserve convenience history (dangling WIP commits, caches) via TTL and policy knobs.

## 2) Pins

Recommended file: `./.genesis/pins.toml`

```toml
version = 1

[pins]
keep = ["h:AAAA...", "h:BBBB..."]
keep_refs = ["refs/tags/v1.2.0", "refs/heads/main"]
keep_evidence_for = ["refs/tags/v1.2.0"]
```

Pins are treated as hard roots.

## 3) GC policy profiles (defaults)

### 3.1 `gc:dev`

- keep history for main/tags; dev branch history limited by depth/time
- keep required evidence for main/tags; old dev evidence can be dropped
- keep dangling commits younger than TTL (e.g. 7 days)

### 3.2 `gc:ci`

- keep only what lock + main + tags require
- drop dangling work
- keep required evidence to satisfy policy verification and replay

### 3.3 `gc:release-archive`

- keep full history + all evidence for tags/releases
- minimal for dev branches

## 4) Mark-and-sweep algorithm (normative)

Inputs:

- store index of all hashes (and sizes)
- ref database
- workspace lock files
- pins
- GC policy

Steps:

1. build root set (refs + locks + pins + optional soft roots)
2. mark live objects by computing reachability closure (policy-driven):
   - commits: patch/result/evidence/attestations/parents (depth/time capped by policy)
   - snapshots: members/defs/modules/proto/overrides/exports_hash/deps/prov (policy-driven)
   - evidence: inputs/outputs/data (policy-driven)
3. sweep: delete objects not in live set
4. optional quarantine: move dead objects to `.genesis/quarantine/` then purge after TTL

Safety requirements:

- avoid corrupting running processes (lock store or require exclusive access)
- deletion must be best-effort but must not leave store inconsistent

## 5) Evidence retention knobs (important)

Policy flags:

- `gc.keep_evidence = none|required|all`
- `gc.keep_replay_logs = true/false`
- `gc.keep_unit_test_logs = true/false`

Recommended defaults:

- dev: `required` for main/tags, drop for old dev
- CI: `required` only for lock + main/tags
- release archive: `all` for tags

## 6) Dependency and provenance retention

- dependencies retained if referenced by locks, protected refs, or pins
- provenance retained per policy (e.g. signatures for tags), but avoid retaining unbounded ancestry

## 7) CLI additions

### 7.1 `genesis gc plan`

```text
genesis gc plan [--policy <gc-policy>] [--json]
```

Print what would be deleted (counts, largest objects, estimated reclaimed bytes).

### 7.2 `genesis gc run`

```text
genesis gc run [--policy <gc-policy>] [--quarantine] [--log <path>] [--json]
```

Execute GC; emit effect log; report reclaimed bytes.

### 7.3 `genesis gc pin` / `unpin`

```text
genesis gc pin <hash-or-ref> [--evidence all|required|none]
genesis gc unpin <hash-or-ref>
```

## 8) Required tests

- lock safety: locked deps remain available after GC
- mainline replay safety: evidence needed for replay remains
- dev pruning: old dangling dev commits removed/quarantined
- tag archival: tag closure remains (incl attestations)
- quarantine TTL purge

