# Selfhost Cutover Dashboard (v0.2)

**Scope: command routing only.** This dashboard does not prove GenesisCode semantic implementation authority, strict Rust-authority retirement, or bootstrap fixpoint closure.
Semantic authority source: `docs/status/SELFHOST_AUTHORITY_v0.1.md`.

- Artifact hash: `2411aa23d75618ce418e439ffa65f3daf88dc85d16c91af3d2d1ab43ec7be7e8`
- Store artifact: `.genesis/store/2411aa23d75618ce418e439ffa65f3daf88dc85d16c91af3d2d1ab43ec7be7e8`
- Selfhost toolchain artifact configured: `selfhost/toolchain.gc`
- Selfhost toolchain artifact exists: `true`

## Summary

| Metric | Value |
| --- | --- |
| Total command groups | 35 |
| Selfhost-routed command groups | 35 |
| Selfhost-routed coverage | 100.00% |
| Default selfhost coverage | 100.00% |
| Fast-path default OK | true |

## Command Coverage

| Command | Fast Path | Selfhost Routed | Default Selfhost |
| --- | --- | --- | --- |
| `agent-index` | false | true | true |
| `agent-plan` | false | true | true |
| `apply-patch` | true | true | true |
| `bench/*` | false | true | true |
| `cli-schema` | false | true | true |
| `commit/*` | true | true | true |
| `debug/*` | true | true | true |
| `eval` | true | true | true |
| `explain` | true | true | true |
| `fmt` | true | true | true |
| `gc/*` | true | true | true |
| `keygen` | false | true | true |
| `mcp` | false | true | true |
| `optimize` | true | true | true |
| `pack` | true | true | true |
| `parse` | true | true | true |
| `pkg/*` | true | true | true |
| `policy/*` | false | true | true |
| `refs/*` | true | true | true |
| `registry/*` | false | true | true |
| `replay` | true | true | true |
| `run` | true | true | true |
| `selfhost-artifact` | false | true | true |
| `selfhost-dashboard` | false | true | true |
| `semantic-edit` | true | true | true |
| `session/*` | false | true | true |
| `sign` | false | true | true |
| `store/*` | true | true | true |
| `sync/*` | true | true | true |
| `test` | true | true | true |
| `transparency-verify` | false | true | true |
| `typecheck` | true | true | true |
| `vcs/*` | true | true | true |
| `verify` | false | true | true |
| `warm` | false | true | true |
