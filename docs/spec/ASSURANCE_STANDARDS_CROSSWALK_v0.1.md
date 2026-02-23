# Assurance Standards Crosswalk v0.1

Normative objective-level crosswalk between assurance profile-pack outputs and
regulated standards posture.

Canonical schema:
- `docs/spec/ASSURANCE_STANDARDS_CROSSWALK_v0.1.json`

Input sources:
- `policies/assurance/profile_packs.toml`
- `docs/spec/ASSURANCE_PROFILE_PACKS_v0.1.md`
- `docs/spec/ASSURANCE_ARTIFACTS_v0.1.md`

## Profile-Pack Evidence Outputs

The following deterministic outputs are treated as crosswalk evidence package
members for regulated profiles:

- `assurance_pack.gc`
- `requirements_trace.gc`
- `tool_qualification.gc`
- `coverage/*.gc`
- `object_equivalence.gc`
- `independent_verifier/*.gc`
- `bundle_manifest.gc`

These outputs are emitted via:
- `genesis gcpm assurance-pack --assurance-profile <profile> --bundle-dir <dir>`

## Objective Matrix (Toolchain Posture)

| Standard | Classification | `--assurance-profile` | Objective ID | Toolchain status | Primary evidence paths |
| --- | --- | --- | --- | --- | --- |
| RTCA DO-178C | DAL A | `do178c-dal-a` | `DO178C-A-TRACE-BIDIRECTIONAL` | covered-by-toolchain | `docs/spec/ASSURANCE_ARTIFACTS_v0.1.md`, `docs/spec/ASSURANCE_PROFILE_PACKS_v0.1.md` |
| RTCA DO-178C | DAL A | `do178c-dal-a` | `DO178C-A-COVERAGE-MCDC` | covered-by-toolchain | `policies/assurance/profile_packs.toml`, `docs/spec/ASSURANCE_ARTIFACTS_v0.1.md` |
| RTCA DO-178C | DAL A | `do178c-dal-a` | `DO178C-A-TOOL-QUAL` | covered-by-toolchain | `docs/spec/ASSURANCE_ARTIFACTS_v0.1.md` |
| RTCA DO-178C | DAL A | `do178c-dal-a` | `DO178C-A-OBJECT-EQUIVALENCE` | covered-by-toolchain | `docs/spec/ASSURANCE_ARTIFACTS_v0.1.md`, `policies/assurance/profile_packs.toml` |
| RTCA DO-178C | DAL A | `do178c-dal-a` | `DO178C-A-INDEPENDENCE` | covered-by-toolchain | `docs/spec/ASSURANCE_ARTIFACTS_v0.1.md`, `policies/assurance/profile_packs.toml` |
| RTCA DO-178C | DAL B | `do178c-dal-b` | `DO178C-B-TRACE-BIDIRECTIONAL` | covered-by-toolchain | `docs/spec/ASSURANCE_ARTIFACTS_v0.1.md` |
| RTCA DO-178C | DAL B | `do178c-dal-b` | `DO178C-B-COVERAGE-DECISION` | covered-by-toolchain | `policies/assurance/profile_packs.toml` |
| RTCA DO-178C | DAL B | `do178c-dal-b` | `DO178C-B-TOOL-QUAL` | covered-by-toolchain | `docs/spec/ASSURANCE_ARTIFACTS_v0.1.md` |
| RTCA DO-178C | DAL B | `do178c-dal-b` | `DO178C-B-OBJECT-EQUIVALENCE` | covered-by-toolchain | `docs/spec/ASSURANCE_ARTIFACTS_v0.1.md`, `policies/assurance/profile_packs.toml` |
| RTCA DO-178C | DAL B | `do178c-dal-b` | `DO178C-B-INDEPENDENCE` | covered-by-toolchain | `docs/spec/ASSURANCE_ARTIFACTS_v0.1.md` |
| NASA NPR 7150.2 | Class A | `nasa-class-a` | `NASA-A-TRACEABILITY` | covered-by-toolchain | `docs/spec/ASSURANCE_ARTIFACTS_v0.1.md` |
| NASA NPR 7150.2 | Class A | `nasa-class-a` | `NASA-A-COVERAGE-MCDC` | covered-by-toolchain | `policies/assurance/profile_packs.toml` |
| NASA NPR 7150.2 | Class A | `nasa-class-a` | `NASA-A-TOOL-QUAL` | covered-by-toolchain | `docs/spec/ASSURANCE_ARTIFACTS_v0.1.md` |
| NASA NPR 7150.2 | Class A | `nasa-class-a` | `NASA-A-OBJECT-EQUIVALENCE` | covered-by-toolchain | `docs/spec/ASSURANCE_ARTIFACTS_v0.1.md`, `policies/assurance/profile_packs.toml` |
| NASA NPR 7150.2 | Class A | `nasa-class-a` | `NASA-A-IVV-INDEPENDENCE` | covered-by-toolchain | `docs/spec/ASSURANCE_ARTIFACTS_v0.1.md` |
| NASA NPR 7150.2 | Class B | `nasa-class-b` | `NASA-B-TRACEABILITY` | covered-by-toolchain | `docs/spec/ASSURANCE_ARTIFACTS_v0.1.md` |
| NASA NPR 7150.2 | Class B | `nasa-class-b` | `NASA-B-COVERAGE-DECISION` | covered-by-toolchain | `policies/assurance/profile_packs.toml` |
| NASA NPR 7150.2 | Class B | `nasa-class-b` | `NASA-B-TOOL-QUAL` | covered-by-toolchain | `docs/spec/ASSURANCE_ARTIFACTS_v0.1.md` |
| NASA NPR 7150.2 | Class B | `nasa-class-b` | `NASA-B-OBJECT-EQUIVALENCE` | covered-by-toolchain | `docs/spec/ASSURANCE_ARTIFACTS_v0.1.md`, `policies/assurance/profile_packs.toml` |
| NASA NPR 7150.2 | Class B | `nasa-class-b` | `NASA-B-INDEPENDENCE` | covered-by-toolchain | `docs/spec/ASSURANCE_ARTIFACTS_v0.1.md` |
| IEC 62304 | Class C | `iec62304-class-c` | `IEC62304-C-TRACEABILITY` | covered-by-toolchain | `docs/spec/ASSURANCE_ARTIFACTS_v0.1.md` |
| IEC 62304 | Class C | `iec62304-class-c` | `IEC62304-C-RISK-COVERAGE` | covered-by-toolchain | `policies/assurance/profile_packs.toml`, `docs/spec/ASSURANCE_ARTIFACTS_v0.1.md` |
| IEC 62304 | Class C | `iec62304-class-c` | `IEC62304-C-TOOL-QUAL` | covered-by-toolchain | `docs/spec/ASSURANCE_ARTIFACTS_v0.1.md` |
| IEC 62304 | Class C | `iec62304-class-c` | `IEC62304-C-OBJECT-EQUIVALENCE` | covered-by-toolchain | `docs/spec/ASSURANCE_ARTIFACTS_v0.1.md`, `policies/assurance/profile_packs.toml` |
| IEC 62304 | Class C | `iec62304-class-c` | `IEC62304-C-DEVICE-QMS` | external | `docs/spec/ASSURANCE_PROFILE_PACKS_v0.1.md` |

## Unresolved Controls (Explicit Non-Claims)

These controls are intentionally tracked as unresolved and external to the core
GenesisCode toolchain contract:

| Control ID | Standard/Profile | Status | Owner | Tracking |
| --- | --- | --- | --- | --- |
| `DO178C-A-ORG-INDEPENDENCE-PROGRAM` | RTCA DO-178C DAL A | open | program-governance | `docs/spec/ASSURANCE_STANDARDS_CROSSWALK_v0.1.md#unresolved-controls-explicit-non-claims` |
| `DO178C-B-AUTHORITY-SIGNOFF` | RTCA DO-178C DAL B | open | program-governance | `docs/spec/ASSURANCE_STANDARDS_CROSSWALK_v0.1.md#unresolved-controls-explicit-non-claims` |
| `NASA-A-IVV-GOVERNANCE` | NASA NPR 7150.2 Class A | open | program-governance | `docs/spec/ASSURANCE_STANDARDS_CROSSWALK_v0.1.md#unresolved-controls-explicit-non-claims` |
| `NASA-B-IVV-ORG-SCOPE` | NASA NPR 7150.2 Class B | open | program-governance | `docs/spec/ASSURANCE_STANDARDS_CROSSWALK_v0.1.md#unresolved-controls-explicit-non-claims` |
| `IEC62304-C-QMS-INTEGRATION` | IEC 62304 Class C | open | program-governance | `docs/spec/ASSURANCE_STANDARDS_CROSSWALK_v0.1.md#unresolved-controls-explicit-non-claims` |

## Not a Certification Claim

- This crosswalk is an engineering-readiness mapping only.
- Formal certification approval and regulator/program sign-off remain external.
- Authority-governed controls are intentionally represented as unresolved where
  the language/toolchain cannot make the claim itself.
