# Assurance Profile Packs v0.1

This document defines standards-oriented assurance profile pack mappings for
deterministic `gcpm assurance-pack` delivery lanes.

Canonical template source:
- `policies/assurance/profile_packs.toml`

Related artifact contracts:
- `docs/spec/ASSURANCE_ARTIFACTS_v0.1.md`
- `docs/spec/ASSURANCE_STANDARDS_CROSSWALK_v0.1.md`
- `docs/spec/ASSURANCE_STANDARDS_CROSSWALK_v0.1.json`
- `docs/spec/REGISTRY_POLICY.md`

Consolidated source:
- legacy top-level policy defaults guidance (redirected through the deprecation map)

## Crosswalk Matrix

| Standard family | Classification | `--assurance-profile` | Required trace | Required qualification | Minimum coverage profile | Object equivalence evidence | Independence attestations | Independent verifier runs |
|---|---|---|---|---|---|---|---|---|
| Custom/internal | custom | `custom` | yes | yes | `none` | no | no | no |
| RTCA DO-178C | DAL A | `do178c-dal-a` | yes | yes | `mcdc` | yes | yes | yes |
| RTCA DO-178C | DAL B | `do178c-dal-b` | yes | yes | `decision` | yes | yes | yes |
| NASA NPR 7150.2 | Class A | `nasa-class-a` | yes | yes | `mcdc` | yes | yes | yes |
| NASA NPR 7150.2 | Class B | `nasa-class-b` | yes | yes | `decision` | yes | yes | yes |
| IEC 62304 | Class C | `iec62304-class-c` | yes | yes | `symbol` | yes | no | yes |

## Deterministic Export Contract

Profile packs are executed through a stable deterministic flow:

1. Emit requirements trace evidence:
   - `genesis gcpm trace --pkg <package.toml> --requirements <requirements.gc> --snapshot <hex64> [--commit <hex64>] [--policy <hex64>]`
2. Emit tool qualification evidence:
   - `genesis gcpm qualify --profile <name> --snapshot <hex64> --requirement <id>... --test-artifact <id=run-manifest-hex64>... --tool <name=path>... [--commit <hex64>] [--policy <hex64>]`
3. Emit assurance pack evidence + optional reproducible bundle mirror:
   - `genesis gcpm assurance-pack --pkg <package.toml> --assurance-profile <profile> --snapshot <hex64> [--commit <hex64>] [--policy <hex64>] [--trace <path-or-hash>] [--qualification <path-or-hash>] [--coverage <path-or-hash> ...] [--object-equivalence <path-or-hash>] [--independence-attestation <left:right@attestor> ...] [--independent-verifier-run <path-or-hash> ...] [--bundle-dir <dir>]`

When `--bundle-dir` is supplied, output is reproducible and must contain:
- `assurance_pack.gc`
- `requirements_trace.gc`
- `tool_qualification.gc`
- `coverage/*.gc`
- `object_equivalence.gc`
- `independent_verifier/*.gc`
- `bundle_manifest.gc`

## Scope Notes

- This mapping is an engineering assurance crosswalk for deterministic
  toolchain posture and evidence export semantics.
- Formal certification authority approval remains external to the language
  runtime and must be executed by the target program's governance process.
- Objective-level posture (including unresolved/non-claim controls) is
  normatively tracked in:
  - `docs/spec/ASSURANCE_STANDARDS_CROSSWALK_v0.1.md`
  - `docs/spec/ASSURANCE_STANDARDS_CROSSWALK_v0.1.json`
