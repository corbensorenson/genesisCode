# Runtime Backend Profiles v0.1

Status: normative for production/runtime backend build profile naming.

## Goal

Define explicit build profiles for `genesis` runtime backend availability so agent workflows
can reason about expected host capability execution paths.

## Profiles (`gc_cli` / `gc_cli_driver`)

- `profile-headless`
  - Backend features disabled.
  - Intended for deterministic CI/headless lanes.
- `profile-gpu`
  - Enables `gpu-device-backend` only.
  - Intended for compute-oriented runtime lanes.
- `profile-gfx`
  - Enables `gfx-desktop-backend` only.
  - Intended for desktop window/input/audio lanes.
- `profile-backend`
  - Enables both `gpu-device-backend` and `gfx-desktop-backend`.
  - Intended for full local/dev runtime capability coverage.

Default profile:
- `gc_cli` default features resolve to `profile-headless`.
- `gc_cli_driver` default features resolve to `profile-headless`.

## Build Examples

```sh
# default headless production profile
cargo build -p gc_cli

# explicit profile builds
cargo build -p gc_cli --no-default-features --features profile-headless
cargo build -p gc_cli --no-default-features --features profile-gpu
cargo build -p gc_cli --no-default-features --features profile-gfx
cargo build -p gc_cli --no-default-features --features profile-backend
```

## Verification Gate

`scripts/check_runtime_backend_feature_matrix.sh` is the normative guard for:
- `gc_effects` feature combinations
- `gc_cli` profile combinations
- `gc_cli_driver` profile/backend consistency tests
- `gcpm env` runtime backend contract mapping under each profile build
  (`gcpm_env_runtime_backend_profile_contract_is_machine_readable`).

The guard renders its report and history into a private temporary directory.
Use `scripts/update_runtime_backend_feature_matrix_report.sh` when retained E0
evidence is required; CI uses that explicit producer before downstream skill
conformance consumes the report.

## GCPM Integration

`genesis gcpm` workspace/profile descriptors can carry runtime backend contracts:
- `[defaults].runtime_backend`
- `[profiles.<name>].runtime_backend`

`gcpm env --runtime-backend <token>` provides deterministic override selection.

`gcpm run` fails closed when the resolved workspace runtime backend contract
(profile `dev` then defaults) is incompatible with the active CLI runtime backend profile.
