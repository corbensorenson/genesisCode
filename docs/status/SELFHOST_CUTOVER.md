# Selfhost Cutover Dashboard (v0.2)

- Artifact hash: `c28ef6e82d2afe4d341bba6e2db9bb2d237cb6014381cac517f416f1f246009c`
- Store artifact: `.genesis/store/c28ef6e82d2afe4d341bba6e2db9bb2d237cb6014381cac517f416f1f246009c`
- Selfhost toolchain artifact configured: `selfhost/toolchain.gc`
- Selfhost toolchain artifact exists: `true`

## Summary

| Metric | Value |
| --- | --- |
| Total command groups | 30 |
| Selfhost-routed command groups | 30 |
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
| `debug/*` | true | true | true |
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
