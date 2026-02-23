# Assurance Program Backlog v0.1

Last updated: 2026-02-23

Scope:
- Explicit non-product governance controls that remain outside the core GenesisCode toolchain contract.
- Auditable ownership, evidence expectations, and closure workflow for each control.

Status semantics:
- `program-backlog`: tracked governance work item; not a toolchain capability claim.
- `closed`: governance artifact set completed and externally approved.

## Control Backlog

| Control ID | Standard/Profile | Status | Owner | Required governance artifacts | Closure workflow |
| --- | --- | --- | --- | --- | --- |
| `DO178C-A-ORG-INDEPENDENCE-PROGRAM` | RTCA DO-178C DAL A | program-backlog | program-governance | organizational independence charter, reviewer assignment matrix, conflict-of-interest attestations, release authority signoff log | produce governance artifact set, conduct independent review board approval, record signed closure bundle hash in program registry |
| `DO178C-B-AUTHORITY-SIGNOFF` | RTCA DO-178C DAL B | program-backlog | program-governance | authority signoff template, delegated authority roster, release package approval record, immutable signoff attestation | collect signoff packet, obtain delegated authority approval, attach signed packet hash and approval timestamp |
| `NASA-A-IVV-GOVERNANCE` | NASA NPR 7150.2 Class A | program-backlog | program-governance | IV&V governance plan, independent assessor roster, review cadence evidence, mission authority acceptance record | complete IV&V governance review cycle, archive signed approval outputs, register closure evidence digest |
| `NASA-B-IVV-ORG-SCOPE` | NASA NPR 7150.2 Class B | program-backlog | program-governance | organizational scope charter, IV&V boundary definition, reviewer role attestations, acceptance approval record | validate scope/role controls, obtain governing authority acceptance, publish signed closure report |
| `IEC62304-C-QMS-INTEGRATION` | IEC 62304 Class C | program-backlog | program-governance | QMS integration plan, regulated process mapping, CAPA linkage evidence, regulatory submission signoff | complete QMS integration audit, approve CAPA/process mapping artifacts, record signed closure manifest |

## Auditability Contract

- Every control closure must produce:
  - a signed closure summary (`control_id`, approver identity, timestamp, artifact hashes)
  - immutable references to governance artifacts in long-term storage
  - a change record updating status from `program-backlog` to `closed`
- Toolchain checks enforce that crosswalk entries point to this backlog for non-product governance controls.
