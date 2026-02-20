# Selfhost Cutover Dashboard (v0.2)

- Artifact hash: `7a866bcec0bc8dc9f382d347a46c19242201cdf25cf9c7e171c4a274e63b6925`
- Store artifact: `.genesis/store/7a866bcec0bc8dc9f382d347a46c19242201cdf25cf9c7e171c4a274e63b6925`
- Selfhost toolchain artifact configured: `selfhost/toolchain.gc`
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
