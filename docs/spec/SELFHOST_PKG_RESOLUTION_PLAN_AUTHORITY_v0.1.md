# Self-hosted package resolution plan authority v0.1

Status: normative partial authority contract for `R4.2.e`.

## Scope

The artifact-loaded `core/pkg::resolution-plan-authority` binding is the exclusive production
semantic authority for selector normalization and classification, declared-versus-inferred
strategy agreement, tag-policy admission, semver selection-policy normalization, and the
existing-lock update decision used by `core/pkg-low::lock`, `core/pkg-low::update`, strict lock
validation, and install-time dependency hydration.

Rust constructs the typed request, evaluates the authority under bounds, contradiction-checks the
closed result, parses semver syntax, compares versions, observes refs, and performs store, registry,
network, and commit mechanisms. Those mechanisms may execute a returned plan but MUST NOT silently
reclassify a selector, infer a strategy, normalize a tag policy, or decide update admission in
production.

## Bootstrap and limits

The plan and fingerprint bindings share one artifact-loaded `EvalCtx`; adding planning therefore
does not add a second toolchain bootstrap. Production uses `SelfhostBootstrapMode::ArtifactOnly`
and fails closed with a sealed `core/pkg/authority-error` if the artifact, binding, evaluation, or
result contract is unavailable. The shared package-resolution authority context is bounded to
20,000,000 steps, 40,000,000 allocation units, 4 MiB bytes and strings, and 65,536 map or vector
entries. These finite limits accommodate a complete admitted lock model and workflow while still
failing closed before unbounded host work.

## Request

Every request is exactly:

```text
{
  :has-existing <bool>
  :kind "genesis/pkg-resolution-plan-request-v0.1"
  :op :plan
  :selector <string>
  :strategy <:pinned|:track-ref|:tag-policy>
  :tag-policy <string-or-nil>
  :update-policy <:manual|:auto>
  :v 1
}
```

Open maps, wrong types, kinds, operations, versions, strategies, or update policies are rejected.

## Selector plan

The authority trims ASCII boundary whitespace and admits exactly these selector forms:

| Input | Normalized kind/value | Inferred strategy |
|---|---|---|
| `commit:<hex64>` or bare `<hex64>` | `:commit`, case-preserved hex64 | `:pinned` |
| `snapshot:<hex64>` | `:snapshot`, case-preserved hex64 | `:pinned` |
| `ref:refs/...` or `refs/...` | `:ref`, normalized ref | `:track-ref` |
| `ref:refs/tags/...` or `refs/tags/...` | `:ref`, normalized ref | `:tag-policy` |
| `semver:<non-empty-range>` | `:semver-range`, trimmed range | `:tag-policy` |

Hex admission is exactly 64 case-insensitive ASCII hexadecimal characters. Ref selectors outside
`refs/...`, empty semver ranges, malformed hashes, and unknown forms are rejected. The declared
strategy must equal the inferred strategy. Tag-policy strategies require a string tag policy;
other strategies forbid one. For semver, `highest`, `latest`, and legacy `exact` normalize to
`:highest`, while `lowest` normalizes to `:lowest`; all other values are rejected. Semver grammar
and version comparison remain host mechanisms.

## Update decision

`:should-resolve` is true when no lock exists. With an existing lock it is true only for `:auto`
requirements whose inferred strategy is `:track-ref` or `:tag-policy`; manual and pinned existing
locks are retained. The dispatcher consumes this bit directly and does not reproduce the rule.

## Result

Every result has exactly these fields:

```text
[:code :kind :message :ok :request-h :selector-kind :selector-value
 :semver-policy :should-resolve :v]
```

`:kind` is `genesis/pkg-resolution-plan-result-v0.1`, `:v` is 1, and `:request-h` is the canonical
term hash of the complete request. Success has nil code/message, a closed selector kind, a typed
normalized value, semver policy iff the selector is semver, and a Boolean update decision.
Rejection has a closed semantic class (`core/pkg/bad-selector`, `core/pkg/strategy-mismatch`,
`core/pkg/tag-policy-required`, `core/pkg/tag-policy-forbidden`, or
`core/pkg/semver-policy-unsupported`), a message, and nil plan fields. The host maps that class to
the pre-existing route diagnostic (`core/pkg/bad-selector` during resolution; coherence classes
become `core/pkg/lock-invariant` during strict validation) without re-evaluating the decision. Rust rejects open, mistyped,
request-unbound, malformed, or contradictory results before package state changes.

## Compatibility oracle

The former Rust parser, strategy inference, semver-policy normalization, and update planner are
reachable only under tests or the explicit `parity-oracle` feature. They support differential
compatibility checks but have no production fallback authority. Generic typed lock parsing in
`gc_pkg`, semver mechanics, ref observation, graph resolution, registry transport, and artifact
validation remain residual production Rust and keep `SD-PACKAGE-RESOLUTION` at H0.

## Nonclaims

This contract does not claim complete graph solving, registry or transport authority, generic lock
codec authority, H2 package resolution, `R4.2.e` or SH-C closure, bootstrap fixpoint, workspace
authority, or release qualification.
