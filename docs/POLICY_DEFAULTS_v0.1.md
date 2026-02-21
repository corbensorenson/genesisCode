# Policy Defaults v0.1

Default ref protection rules, required obligations, and enforcement strategy.

## 1.0 Policy model overview

A policy is a content-addressed artifact defining:

- which refs are protected
- which obligations are required to advance each ref class
- which evidence kinds are required
- optional signature/attestation requirements
- optional signer-role requirements and role-separation constraints

Enforcement occurs at:

- `refs set`
- `pkg publish`
- optionally `pkg install` (verification strictness)

## 2.0 Ref classes (normative defaults)

- Development branches: `refs/**/heads/*` except main
- Mainline branches: `refs/**/heads/main`
- Release tags: `refs/**/tags/*`
- Frozen refs: `refs/frozen/*` (cannot advance by default)

## 3.0 Default obligations by ref class

### 3.1 Dev branches

Required obligations:

- `core/obligation::unit-tests`
- `core/obligation::capabilities-declared`

Signatures: not required.

### 3.2 Mainline

Required obligations:

- `core/obligation::unit-tests`
- `core/obligation::replayable-tests`
- `core/obligation::capabilities-declared`
- `core/obligation::determinism` (for declared-pure packages/modules)

Signatures: optional.

### 3.3 Release tags

Required obligations:

- all mainline obligations
- `core/obligation::no-unknown-deps`
- `core/obligation::signed-provenance` (default ON)

Recommended required evidence kinds for regulated release profiles:

- `:requirements-trace`
- `:tool-qualification`

Signatures: required by default.

Role separation (default for protected release profiles):

- required attestation roles: `:reviewer`, `:verifier`
- minimum per-role signatures: `:reviewer >= 1`, `:verifier >= 1`
- independence pair: `(:reviewer, :verifier)` must be signed by distinct keys

### 3.4 Frozen refs

Cannot be advanced.

## 4.0 Determinism rules

If `:caps` is empty for a package/module, enforce no effects (no sealed EFFECT observed).

## 5.0 Install-time verification strictness

Policy defines `install.verify = off|basic|strict`.

- basic: verify presence of obligations/evidence hashes
- strict: verify integrity, replay logs, optionally re-run tests

## 6.0 Policy artifact schema (recommended)

Example TOML:

```toml
version = 1
name = "policy:default-v0.1"

[refs]
frozen_prefixes = ["refs/frozen/"]

[classes.dev]
patterns = ["refs/**/heads/*"]
exclude = ["refs/**/heads/main"]
required_obligations = ["core/obligation::unit-tests", "core/obligation::capabilities-declared"]
require_signatures = false

[classes.main]
patterns = ["refs/**/heads/main"]
required_obligations = [
  "core/obligation::unit-tests",
  "core/obligation::replayable-tests",
  "core/obligation::capabilities-declared",
  "core/obligation::determinism"
]
require_signatures = false

[classes.tags]
patterns = ["refs/**/tags/*"]
required_obligations = [
  "core/obligation::unit-tests",
  "core/obligation::replayable-tests",
  "core/obligation::capabilities-declared",
  "core/obligation::determinism",
  "core/obligation::no-unknown-deps",
  "core/obligation::signed-provenance"
]
required_evidence_kinds = [":requirements-trace", ":tool-qualification"]
require_signatures = true
required_attestation_roles = [":reviewer", ":verifier"]

[classes.tags.role_min_signatures]
":reviewer" = 1
":verifier" = 1

[[classes.tags.independent_role_pairs]]
left = ":reviewer"
right = ":verifier"

[install]
verify = "basic"
```

## 7.0 Enforcement points

### 7.1 `refs set`

- determine ref class
- verify commit obligations and evidence presence
- verify attestations if required
- enforce role requirements (`required_attestation_roles`, `role_min_signatures`)
- enforce separation-of-duty (`independent_role_pairs`)
- only then advance ref

### 7.2 `pkg publish`

Same checks as `refs set`, plus push required artifacts.

### 7.4 Role-aware attestation artifact fields

` :vcs/attestation` may include an optional `:role` field (`symbol|string`) such as
`:reviewer` or `:verifier`. Policies evaluate role gates only on cryptographically valid
attestations.

### 7.3 Optional `pkg install`

Verify according to `install.verify`.
