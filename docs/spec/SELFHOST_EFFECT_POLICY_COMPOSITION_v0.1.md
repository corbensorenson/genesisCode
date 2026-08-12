# Self-host Effect Policy Composition v0.1

Status: normative partial R4.2.d contract; no H2 claim.

`core/effects::policy-authority` is the GenesisCode producer for the first closed
effect-policy composition slice: baseline operation admission, per-operation
`allow` precedence, and the canonical log capability descriptor fields `:op`,
`:create-dirs`, `:timeout-ms`, and `:log-inline-max-bytes`.

The Rust host still parses TOML, constructs the candidate operation inventory,
independently reconstructs the legacy result, and rejects every contradiction.
That live oracle is a required safety mechanism for this partial checkpoint and
prevents `SD-EFFECT-POLICY` from reaching H2. Removing it before all residual
decisions are GenesisCode-owned and independently verified is forbidden.

## Closed Protocol

Each request is a closed five-field map with kind
`genesis/effect-policy-authority-request-v0.1`, version `1`, the operation string,
the complete ordered baseline allow vector, and either `nil` or an exact override
map containing `:allow`, `:create-dirs`, `:timeout-ms`, and
`:log-inline-max-bytes`. Optional fields use `nil`; no omitted or additional field
is accepted. A policy may expose at most 4,096 unique candidate operations.

The authority returns a closed six-field
`genesis/effect-policy-authority-result-v0.1` map containing the exact operation,
boolean admission decision, canonical capability map when admitted or `nil` when
denied, lowercase canonical request hash, and version `1`. Malformed requests
return sealed errors. The host rejects unknown fields, identity drift, request-hash
substitution, denied non-nil capabilities, admitted non-map capabilities, and any
result that contradicts its retained compatibility oracle.

Per-operation `allow` has legacy precedence: an override with `allow = false`
denies the operation; an override with true or no explicit `allow` admits it;
without an override, baseline membership decides admission. Capability timeouts
are nonnegative, inline limits are emitted only when positive, and
`:create-dirs` is emitted only when true. The existing strict TOML parser remains
responsible for rejecting invalid host-side numeric values before invocation.

## Production Boundary

Every file-backed production CLI capability policy is loaded through
`load_caps_policy`, which resolves the selected artifact-only self-host frontend
and invokes `CapsPolicy::load_with_selfhost_authority`. The Rust route exists only
under `parity-harness`; production builds fail closed if a Rust frontend is somehow
selected. Obligation preflight uses the same self-host route while preserving its
already-normative missing-policy observation behavior.

The effect runner uses the validated GenesisCode capability descriptor in log
entries. Host code retains payload enforcement, filesystem path resolution,
resource accounting, cancellation, effect execution, and replay mechanisms.
`CapsPolicy::from_toml_str`, `CapsPolicy::empty`, and the independent legacy
composition oracle remain reachable for tests, host mechanisms, and this partial
transition, and therefore are not evidence of H2.

## Residual Decisions And Nonclaims

The machine profile lists the complete residual boundary. It includes TOML syntax
and type decoding; candidate-inventory construction; global runtime, task, log,
store, and refs policy; operation-specific filesystem, network, process, database,
crypto, FFI, plugin, model, graphics, and device constraints; secret and path
resolution; effect execution and cancellation; strict replay; policy aliasing;
and removal of the compatibility oracle.

This contract does not promote `SD-EFFECT-POLICY`, close R4.2.d or SH-C, establish
H2/H3/H4, authorize release, or authorize GenesisBench, Genesis Foundry,
GenesisChallenge, or Genesis Model work. It is an independently checked partial
production shadow that narrows the next migration frontier without weakening the
current host enforcement boundary.
