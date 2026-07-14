# Versioning Policy v0.1

Normative policy for GenesisCode crate, CLI, generated-artifact, and release-train versions.

## Current Release Train

- Current workspace package version: `0.2.0`.
- Workspace crates inherit the version from `[workspace.package].version` in `Cargo.toml`.
- Workspace crates inherit `publish = false`; crates.io publication is disabled until the release/registry roadmap explicitly opens that boundary.
- CLI `--version`, selfhost `:generated-by` metadata, and release-smoke checks derive from the Cargo package version.
- Spec documents keep their own document versions (`v0.1`, `v0.2`, etc.) and do not imply the crate release version.
- Serialized-format, hash-domain, binary-magic, and artifact identities are independently governed by `genesis.version-surfaces.json`; they must not be inferred from the crate release.

## Version Sources

| Surface | Source of truth | Gate |
|---|---|---|
| Rust workspace crate version | `Cargo.toml` `[workspace.package].version` | `scripts/check_versioning_release_hygiene.sh` |
| Crate manifest inheritance | `crates/*/Cargo.toml` `version.workspace = true` | `scripts/check_versioning_release_hygiene.sh` |
| Crate publication boundary | `crates/*/Cargo.toml` `publish.workspace = true` + root `publish = false` | `scripts/check_versioning_release_hygiene.sh` |
| Generated selfhost artifact producer version | `selfhost/toolchain.gc` `:generated-by "genesis <version>"` | `scripts/check_versioning_release_hygiene.sh` |
| Release facts | Canonical inputs selected by `policies/release_notes_v0.1.json` | `scripts/check_release_notes.sh` |
| Human release notes | Generated block in `CHANGELOG.md` | `scripts/check_release_notes.sh` |
| Release smoke contract | `docs/spec/RELEASE_SMOKE_v0.1.md` | `scripts/check_release_smoke.sh` |
| Format/hash/artifact compatibility | `genesis.version-surfaces.json` | `scripts/check_version_surfaces.sh` |

## Semver Contract Before v1.0

GenesisCode remains pre-1.0. Public CLI, file formats, package-lock formats, host ABI schemas, and selfhost artifacts may change when the change is captured in specs, changelog, and migration notes.

Pre-1.0 compatibility rules:

- Patch releases may fix bugs, harden gates, and improve diagnostics without changing source semantics.
- Minor releases may change language/tooling semantics when docs, goldens, and migration notes are updated in the same change.
- Any format or replay-log incompatibility must be explicit in the relevant spec and changelog entry.
- No crates.io publication is release-ready until `scripts/check_release_smoke.sh` passes.

## Bump Procedure

1. Update `[workspace.package].version` in root `Cargo.toml`.
2. Keep all workspace crates on `version.workspace = true`.
3. Regenerate `selfhost/toolchain.gc` with the new package version.
4. Keep `publish = false` unless a release owner is intentionally opening the public registry boundary.
5. Run `scripts/update_release_notes.sh` to regenerate the machine-readable release facts and the bounded `CHANGELOG.md` block; add a dated release heading only when cutting a release.
6. Reconcile every format/hash/artifact identity and migration record in `genesis.version-surfaces.json`.
7. Run:
   - `bash scripts/check_version_surfaces.sh`
   - `bash scripts/check_release_notes.sh`
   - `bash scripts/check_versioning_release_hygiene.sh`
   - `bash scripts/check_release_smoke.sh`
   - `bash scripts/check_green_front_door.sh`
8. Do not mark the release complete while the worktree has uncommitted release files.
9. Render E3/E4 evidence with `scripts/update_evidence_release_asset.sh`, publish the content-addressed files through create-new release and mirror channels, and perform the descriptor's offline verification with an out-of-band trust-policy digest. A local candidate is not release authority.

## Generated Evidence Rule

Generated reports under `.genesis/perf/*.json` and `.genesis/perf/*.jsonl` are runtime evidence, not source files. They must remain untracked unless explicitly allowlisted in `policies/generated_artifact_allowlist.txt` for a release evidence exception.
