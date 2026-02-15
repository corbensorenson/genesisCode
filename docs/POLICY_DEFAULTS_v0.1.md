# Policy Defaults v0.1

Default ref protection rules, obligations, and enforcement strategy for GenesisGraph/GenesisPkg.

## 1.0 Policy model overview

A policy is an artifact (content-addressed) that defines:

- which refs are protected
- which obligations are required to advance each ref class
- what evidence kinds are required and how strict verification is
- optional signature/attestation requirements

Policy enforcement occurs at:

- `refs set`
- `pkg publish`
- optionally `pkg install` (verification strictness)

## 2.0 Ref classes (normative defaults)

Refnames are classified by prefix/pattern:

### 2.1 Development branches

- `refs/heads/*`
- `refs/pkgs/*/heads/*`
- `refs/contracts/*/heads/*`

### 2.2 Release tags

- `refs/tags/*`
- `refs/pkgs/*/tags/*`
- `refs/contracts/*/tags/*`

### 2.3 Protected mainline

- `refs/heads/main`
- `refs/pkgs/*/heads/main`

### 2.4 Frozen / immutable refs (optional)

- `refs/frozen/*`

Default: advancing frozen refs is disallowed.

## 3.0 Default obligations by ref class

Obligations are qualified symbols; evidence artifacts are hashes stored in the content store.

### 3.1 Dev branches (`refs/**/heads/*` except main)

Required obligations:

- `core/obligation::unit-tests`
- `core/obligation::capabilities-declared`

Evidence required:

- unit test report evidence
- capability audit evidence (can be derived from effect logs)

Signatures:

- not required by default

### 3.2 Mainline branches (`refs/**/heads/main`)

Required obligations:

- `core/obligation::unit-tests`
- `core/obligation::replayable-tests`
- `core/obligation::capabilities-declared`
- `core/obligation::determinism` (for declared-pure modules/packages)

Evidence required:

- unit test logs
- effect logs from test runs (for replay)
- replay verification evidence
- capability audit evidence

Signatures:

- optional by default (recommended for shared registries)

### 3.3 Release tags (`refs/**/tags/*`)

Required obligations:

- all mainline obligations, plus:
- `core/obligation::no-unknown-deps` (deps pinned to hashes)
- `core/obligation::signed-provenance` (default ON for tags)
- optional: `core/obligation::resource-budgets` (if configured)

Evidence required:

- mainline evidence
- dependency lock evidence (`genesis.lock` hash or equivalent)
- signature/attestation artifacts

### 3.4 Frozen refs (`refs/frozen/*`)

Rule: cannot be advanced by default policy.

## 4.0 Determinism rules (policy semantics)

### 4.1 Declared pure packages/modules

If `:caps` is empty at package/module level:

- require `core/obligation::determinism`
- evaluation/tests must not emit sealed EFFECT requests

### 4.2 Declared effectful packages/modules

If `:caps` is non-empty:

- require `core/obligation::capabilities-declared`
- require `core/obligation::replayable-tests` where policy demands it (main/tags)

## 5.0 Install-time verification strictness

Policies define install verification strictness:

- `install.verify = off|basic|strict`

Defaults:

- dev workspace: `basic`
- CI: `strict`
- releases: `strict`

Basic checks:

- commit exists
- obligations list contains required symbols
- evidence hashes are present in store/bundle

Strict checks:

- verify evidence integrity
- replay effect logs
- optionally re-run tests in sandbox runner

## 6.0 Policy artifact schema (recommended)

Policies are stored as artifacts (CoreForm, TOML, or JSON). Example TOML:

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
require_signatures = true

[install]
verify = "basic"
```

## 7.0 Enforcement points (must implement)

### 7.1 `refs set`

- load policy and determine ref class by patterns
- verify commit object:
  - obligations include all required ones
  - evidence hashes are present (and verifiable in strict mode)
  - if signatures required: verify attestation/signature
- only then mutate the ref

### 7.2 `pkg publish`

- same gating as `refs set`, but includes:
  - push required artifacts to remote
  - advance remote ref only if policy satisfied

### 7.3 Optional `pkg install` enforcement

- verify installed packages according to policy install.strictness

## 8.0 Minimal policy implementation order (MVP)

1. pattern matching for ref classes
2. required obligations presence check
3. required evidence presence check
4. determinism check based on `:caps` empty
5. signatures/attestations (integrate with existing signing work)

