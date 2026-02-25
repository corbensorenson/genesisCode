# Assurance Program Backlog v0.1

Last updated: 2026-02-24

Scope:
- Explicit non-product governance controls that remain outside the core GenesisCode toolchain contract.
- Auditable ownership, evidence expectations, and closure workflow for each control.

Status semantics:
- `program-backlog`: tracked governance work item; not a toolchain capability claim.
- `closed`: governance artifact set completed and externally approved.

## Control Register

| Control ID | Standard/Profile | Status | Owner | Closure bundle | Tracking |
| --- | --- | --- | --- | --- | --- |
| `DO178C-A-ORG-INDEPENDENCE-PROGRAM` | RTCA DO-178C DAL A | closed | program-governance | `docs/program/assurance_closures/do178c-a-org-independence-program.closure.json` | `docs/program/ASSURANCE_CONTROL_CLOSURES_v0.1.md#do178c-a-org-independence-program` |
| `DO178C-B-AUTHORITY-SIGNOFF` | RTCA DO-178C DAL B | closed | program-governance | `docs/program/assurance_closures/do178c-b-authority-signoff.closure.json` | `docs/program/ASSURANCE_CONTROL_CLOSURES_v0.1.md#do178c-b-authority-signoff` |
| `NASA-A-IVV-GOVERNANCE` | NASA NPR 7150.2 Class A | closed | program-governance | `docs/program/assurance_closures/nasa-a-ivv-governance.closure.json` | `docs/program/ASSURANCE_CONTROL_CLOSURES_v0.1.md#nasa-a-ivv-governance` |
| `NASA-B-IVV-ORG-SCOPE` | NASA NPR 7150.2 Class B | closed | program-governance | `docs/program/assurance_closures/nasa-b-ivv-org-scope.closure.json` | `docs/program/ASSURANCE_CONTROL_CLOSURES_v0.1.md#nasa-b-ivv-org-scope` |
| `IEC62304-C-QMS-INTEGRATION` | IEC 62304 Class C | closed | program-governance | `docs/program/assurance_closures/iec62304-c-qms-integration.closure.json` | `docs/program/ASSURANCE_CONTROL_CLOSURES_v0.1.md#iec62304-c-qms-integration` |

## Auditability Contract

- Every control closure must produce:
  - a signed closure summary (`control_id`, approver identity, timestamp, artifact hashes)
  - immutable references to governance artifacts in long-term storage
  - a change record updating status from `program-backlog` to `closed`
- Canonical closure registry:
  - `docs/program/ASSURANCE_CONTROL_CLOSURES_v0.1.json`
  - `docs/program/ASSURANCE_CONTROL_CLOSURES_v0.1.md`
- Toolchain checks enforce that closed controls in the assurance crosswalk resolve to closure bundles with immutable references.
