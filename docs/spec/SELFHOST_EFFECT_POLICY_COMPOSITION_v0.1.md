# Self-host Effect Policy Composition v0.1

Status: normative partial R4.2.d contract; no H2 claim.

`core/effects::policy-authority` is the GenesisCode producer for the first closed
effect-policy composition slice: baseline operation admission, per-operation
`allow` precedence, per-operation base-directory selection, and selection of the
canonical generic enforcement controls `:create-dirs`, `:timeout-ms`, and
`:log-inline-max-bytes`. The same path-free map, including exact `:op`, is the
capability descriptor recorded in the effect log. The authority also owns the
private per-operation `max_bytes` decision consumed by filesystem, store, media,
FFI, and bridge enforcement. That decision is deliberately separate from the
capability descriptor and therefore never adds the configured byte limit to an
effect log.
`core/effects::policy-inventory-authority` owns deterministic union,
deduplication, and ordering of baseline and per-operation candidate names.
`core/effects::resource-policy-authority` owns global log/store byte budgets,
log/store/refs configured-or-default location selection, runtime and task
resource limits, and selection of an explicit task worker default from the
configured value or the host's bounded available-worker observation.

The Rust host still parses TOML, independently reconstructs the legacy candidate
inventory, per-operation results, and log/refs/runtime/store/task resource
policy, and rejects every contradiction.
That live oracle is a required safety mechanism for this partial checkpoint and
prevents `SD-EFFECT-POLICY` from reaching H2. Removing it before all residual
decisions are GenesisCode-owned and independently verified is forbidden.

## Closed Protocol

Each request is a closed six-field map with kind
`genesis/effect-policy-authority-request-v0.3`, version `3`, the operation string,
the complete ordered baseline allow vector, a positive host
`:platform-max-bytes` observation equal to the target `usize` maximum, and either
`nil` or an exact override map containing `:allow`, `:base-dir`, `:create-dirs`,
`:timeout-ms`, `:log-inline-max-bytes`, and `:max-bytes`. The base directory is
`nil` or the exact configured string. Missing optional fields use `nil`. A TOML
integer is transported exactly for `:max-bytes`; a present non-integer is
transported as the closed `:invalid-type` observation so GenesisCode, rather than
Rust, decides its effect-use error state. No omitted or additional field is
accepted. A policy may expose at most 4,096 unique candidate operations.

Before those per-operation requests, the inventory authority receives a closed
four-field `genesis/effect-policy-inventory-request-v0.1` map containing version
`1`, the complete baseline vector, and the complete ordered vector of override
operation names. It validates string membership and returns the strictly ordered,
duplicate-free union in a closed
`genesis/effect-policy-inventory-result-v0.1` map bound to the request hash. The
host rejects malformed, oversized, duplicate, unsorted, substituted, or
oracle-contradicting inventory results and uses only the validated GenesisCode
inventory to drive per-operation composition.

The authority returns a closed eight-field
`genesis/effect-policy-authority-result-v0.3` map containing the exact operation,
boolean admission decision, selected `:base-dir`, canonical capability map when
admitted or `nil` when denied, private `:max-bytes-policy`, lowercase canonical
request hash, and version `3`. For an admitted operation, the private policy is
an exact `{:limit ... :status ...}` map. Its status is exactly `:absent`,
`:invalid-type`, `:nonpositive`, `:platform-overflow`, or `:valid`; only `:valid`
carries a positive integer limit that fits `:platform-max-bytes`, and every other
status carries `nil`. Denied operations must carry no base directory, capability,
or max-byte policy. Malformed requests return sealed errors. The host rejects
unknown fields, identity drift, request-hash substitution, invalid path types,
denied non-nil state, admitted non-map capabilities or byte policies,
noncanonical false/zero/negative/overflowing controls, contradictory status/limit
pairs, operation substitution inside the capability, and any result that
contradicts its retained compatibility oracle. After validation, the host
installs the GenesisCode-selected base directory, create-directories flag,
timeout, per-operation log limit, and closed max-byte state into enforcement;
its separately parsed values are used only by the compatibility oracle.

The resource authority receives a closed eight-field
`genesis/effect-resource-policy-request-v0.3` map. It contains version `3`, the
positive host observation `:available-workers`, and exact `:log`, `:refs`,
`:runtime`, `:store`, and `:task` maps. Missing optional TOML fields are
represented by `nil`. Runtime and task limits must be nonnegative integers, and
a configured `:default-workers` must be positive. Global `:inline-max-bytes`,
`:max-artifact-bytes-per-run`, and `:max-run-bytes` accept the legacy integer
domain and are normalized by GenesisCode so only positive limits survive; zero
and negative values become `nil`. Location inputs are `nil` or strings.

The closed `genesis/effect-resource-policy-result-v0.3` result is bound to the
complete request hash, preserves the validated limits, replaces a missing task
default with `:available-workers`, defaults store and refs locations to
`.genesis/store` and `.genesis/refs.gc`, and defaults the log store only when the
normalized inline spill threshold is present. Explicit locations always win. The
host strictly decodes every field into `u64`, platform `usize`, or a UTF-8 path;
rejects invalid result domains and overflow; compares the complete result with
its independently parsed compatibility oracle; installs the validated
GenesisCode log, refs, runtime, store, and task values; and only then resolves
relative paths against the capability file's parent directory. Filesystem path
resolution and use remain host mechanisms rather than policy-selection authority.

Per-operation `allow` has legacy precedence: an override with `allow = false`
denies the operation; an override with true or no explicit `allow` admits it;
without an override, baseline membership decides admission. Capability timeouts
are nonnegative, inline limits are emitted only when positive, and
`:create-dirs` is emitted only when true. `max_bytes` retains legacy observable
timing: the capability file loads, then an invalid type, nonpositive integer, or
platform overflow produces the exact prior error when an affected effect is
used. The existing strict TOML parser remains responsible for syntax and integer
representation; it does not choose the max-byte policy state.

## Production Boundary

Every file-backed production CLI capability policy is loaded through
`load_caps_policy`, which resolves the selected artifact-only self-host frontend
and invokes `CapsPolicy::load_with_selfhost_authority`. The Rust route exists only
under `parity-harness`; production builds fail closed if a Rust frontend is somehow
selected. Obligation preflight uses the same self-host route while preserving its
already-normative missing-policy observation behavior.

The effect runner uses the validated GenesisCode capability descriptor in log
entries and installs all decoded generic operation and resource controls for host
enforcement. The selected base directory remains separate from that descriptor so
logs do not gain path material; Rust installs it before resolving relative paths
against the capability-file base. The private max-byte state likewise remains out
of the descriptor. Every production generic or bridge byte-limit consumer checks
the installed authority state before the raw compatibility field; raw fallback is
reachable only for policies constructed without the self-host authority by
explicit compatibility and test routes.
Host code retains payload measurement and enforcement mechanisms, filesystem path resolution,
accounting mechanisms, cancellation, effect execution, and replay mechanisms.
`CapsPolicy::from_toml_str`, `CapsPolicy::empty`, and the independent legacy
composition oracle remain reachable for tests, host mechanisms, and this partial
transition, and therefore are not evidence of H2.

## Residual Decisions And Nonclaims

The machine profile lists the complete residual boundary. It includes TOML syntax
and type decoding; global store remote transport, TLS, and authentication policy;
operation-specific network, process, database, crypto, FFI, plugin, model,
graphics, and device constraints; secret and path resolution; effect execution
and cancellation; strict replay; and removal of the compatibility oracle.
Filesystem policy configuration is no longer a residual decision: admission,
base-directory selection, directory-creation selection, and byte-limit state are
GenesisCode-produced. Filesystem path joining, canonicalization, symlink defense,
sandbox enforcement, actual reads/writes, byte measurement, and error transport
remain bounded host mechanisms under `path-and-secret-resolution` and
`effect-execution-and-hard-cancellation`; this is not a claim that filesystem
execution moved into the pure kernel. Policy aliases are governed separately by
the policy-alias authority and are not part of this profile's residual inventory.

This contract does not promote `SD-EFFECT-POLICY`, close R4.2.d or SH-C, establish
H2/H3/H4, authorize release, or authorize GenesisBench, Genesis Foundry,
GenesisChallenge, or Genesis Model work. It is an independently checked partial
production shadow that narrows the next migration frontier without weakening the
current host enforcement boundary.
