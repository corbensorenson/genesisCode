# Selfhost Cutover Dashboard (v0.2)

- Artifact hash: `7a84c40a7d885a2d985b6086255f32ad931f0d996e0892ad6f9885a99d7dfb61`
- Store artifact: `.genesis/store/7a84c40a7d885a2d985b6086255f32ad931f0d996e0892ad6f9885a99d7dfb61`
- Selfhost toolchain artifact configured: `/Users/corbensorenson/Documents/genesisCode/selfhost/toolchain.gc`
- Selfhost toolchain artifact exists: `true`

## Summary

| Metric | Value |
| --- | --- |
| Total command groups | 29 |
| Selfhost-routed command groups | 29 |
| Selfhost-routed coverage | 100.00% |
| Default selfhost coverage | 100.00% |
| Fast-path default OK | true |

## Command Coverage

| Command | Fast Path | Selfhost Routed | Default Selfhost |
| --- | --- | --- | --- |
| `agent-index` | false | true | true |
| `apply-patch` | true | true | true |
| `cli-schema` | false | true | true |
| `commit/*` | true | true | true |
| `eval` | true | true | true |
| `explain` | true | true | true |
| `fmt` | true | true | true |
| `gc/*` | true | true | true |
| `keygen` | false | true | true |
| `optimize` | true | true | true |
| `pack` | true | true | true |
| `pkg/*` | true | true | true |
| `policy/*` | false | true | true |
| `refs/*` | true | true | true |
| `registry/*` | false | true | true |
| `replay` | true | true | true |
| `run` | true | true | true |
| `selfhost-artifact` | false | true | true |
| `selfhost-dashboard` | false | true | true |
| `semantic-edit` | true | true | true |
| `sign` | false | true | true |
| `store/*` | true | true | true |
| `sync/*` | true | true | true |
| `test` | true | true | true |
| `transparency-verify` | false | true | true |
| `typecheck` | true | true | true |
| `vcs/*` | true | true | true |
| `verify` | false | true | true |
| `warm` | false | true | true |
