> Deprecated Top-Level Doc: Use `docs/DEPRECATION_MAP_v0.1.md` for canonical replacements.

# Garbage Collection Rules v0.1

Safe pruning for the local Genesis store using reachability closures.

Scope: local artifact store `.genesis/store/`
Goal: prevent unbounded growth while preserving correctness and reproducibility.

GC is outside the kernel. Deletions are effects and must be logged.

## 1) Roots

Hard roots (must keep):

- all local refs (their head commit hashes)
- all locked commits/snapshots from all `genesis.lock` files
- pinned hashes and pinned refs (`.genesis/pins.toml`)

Soft roots (policy-controlled):

- recent dangling commits within TTL
- caches

## 2) Mark-and-sweep

1. Build root set
2. Compute `Live = Closure(Roots, policy)` using the same reachability rules as export/publish
3. Sweep: delete store objects not in Live (or quarantine)

## 3) Pins

Recommended file: `./.genesis/pins.toml`

```toml
version = 1

[pins]
keep = ["h:..."]
keep_refs = ["refs/tags/v1.0.0"]
```

Pins are treated as hard roots.

## 4) Policies

Default profiles:

- dev: keep recent history and required evidence for main/tags
- ci: keep only what lock + main/tags require
- release-archive: keep full history and all evidence for tags

## 5) Commands

- `genesis gc plan` print what would be deleted
- `genesis gc run` execute GC
- `genesis gc pin` add pins
- `genesis gc unpin` remove pins

## 6) Required tests

- lock safety
- mainline replay safety
- dev branch pruning
- tag archival
- quarantine TTL
