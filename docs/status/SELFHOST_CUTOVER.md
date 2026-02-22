# Selfhost Cutover Dashboard (v0.2)

- Artifact hash: `e4aa103537e2cd161bf6bde9097cc7db1d22b2e2b79dd3689caafd4116f0bec1`
- Store artifact: `.genesis/store/e4aa103537e2cd161bf6bde9097cc7db1d22b2e2b79dd3689caafd4116f0bec1`
- Selfhost toolchain artifact configured: `/Users/corbensorenson/Documents/genesisCode/selfhost/toolchain.gc`
- Selfhost toolchain artifact exists: `true`

## Summary

| Metric | Value |
| --- | --- |
| Total command groups | 25 |
| Selfhost-routed command groups | 25 |
| Selfhost-routed coverage | 100.00% |
| Default selfhost coverage | 100.00% |
| Fast-path default OK | true |

## Command Coverage

| Command | Fast Path | Selfhost Routed | Default Selfhost |
| --- | --- | --- | --- |
| `fmt` | true | true | true |
| `eval` | true | true | true |
| `explain` | true | true | true |
| `run` | true | true | true |
| `replay` | true | true | true |
| `test` | true | true | true |
| `pack` | true | true | true |
| `selfhost-artifact` | false | true | true |
| `selfhost-dashboard` | false | true | true |
| `agent-index` | false | true | true |
| `keygen` | false | true | true |
| `sign` | false | true | true |
| `transparency-verify` | false | true | true |
| `typecheck` | true | true | true |
| `optimize` | true | true | true |
| `apply-patch` | true | true | true |
| `semantic-edit` | true | true | true |
| `verify` | false | true | true |
| `store/*` | true | true | true |
| `refs/*` | true | true | true |
| `pkg/*` | true | true | true |
| `policy/*` | false | true | true |
| `sync/*` | true | true | true |
| `gc/*` | true | true | true |
| `vcs/*` | true | true | true |
