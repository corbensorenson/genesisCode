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
effect log. The authority also owns normalization and typed error state for the
private per-operation `allow_programs` rule consumed by process launch
enforcement; program matching remains a bounded host enforcement mechanism.
It also owns the private database policy consumed by SQL and KV dispatch:
`db_target_allow`, `allow_query_classes`, `max_result_bytes`, `max_row_count`,
and `max_value_bytes`. Matching and resource measurement remain bounded host
enforcement mechanisms.
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
`genesis/effect-policy-authority-request-v0.5`, version `5`, the operation string,
the complete ordered baseline allow vector, a positive host
`:platform-max-bytes` observation equal to the target `usize` maximum, and either
`nil` or an exact override map containing `:allow`, `:base-dir`, `:create-dirs`,
`:timeout-ms`, `:log-inline-max-bytes`, `:max-bytes`, `:process-programs`, and
`:database-policy`. The nested database map has exactly `:target-allow`,
`:query-classes`, `:max-result-bytes`, `:max-row-count`, and `:max-value-bytes`.
The base directory is
`nil` or the exact configured string. Missing optional fields use `nil`. A TOML
integer is transported exactly for `:max-bytes`; a present non-integer is
transported as the closed `:invalid-type` observation so GenesisCode, rather than
Rust, decides its effect-use error state. Missing `allow_programs` is transported
as `nil`, a non-array as `:invalid-type`, and array entries as their exact strings
or the closed `:invalid-entry` observation. Database allowlists use the same
exact string or closed invalid observation transport; database bounds use exact
integers or `:invalid-type`. No omitted or additional field is accepted. A
policy may expose at most 4,096 unique candidate operations.

Before those per-operation requests, the inventory authority receives a closed
four-field `genesis/effect-policy-inventory-request-v0.1` map containing version
`1`, the complete baseline vector, and the complete ordered vector of override
operation names. It validates string membership and returns the strictly ordered,
duplicate-free union in a closed
`genesis/effect-policy-inventory-result-v0.1` map bound to the request hash. The
host rejects malformed, oversized, duplicate, unsorted, substituted, or
oracle-contradicting inventory results and uses only the validated GenesisCode
inventory to drive per-operation composition.

The authority returns a closed ten-field
`genesis/effect-policy-authority-result-v0.5` map containing the exact operation,
boolean admission decision, selected `:base-dir`, canonical capability map when
admitted or `nil` when denied, private `:max-bytes-policy` and
`:process-program-policy`, private `:database-policy`, lowercase canonical
request hash, and version `5`.
For an admitted operation, the private byte policy is
an exact `{:limit ... :status ...}` map. Its status is exactly `:absent`,
`:invalid-type`, `:nonpositive`, `:platform-overflow`, or `:valid`; only `:valid`
carries a positive integer limit that fits `:platform-max-bytes`, and every other
status carries `nil`. The process-program policy is an exact
`{:programs ... :status ...}` map whose status is `:absent`, `:invalid-type`,
`:invalid-entry`, `:empty`, or `:valid`; only `:valid` carries a nonempty vector
of whitespace-trimmed, nonempty strings. Order and duplicates remain observable
and are preserved. The database result is an exact five-field map. Its two
allowlists use exact `{:status ... :values ...}` states with `:absent`,
`:invalid-type`, `:invalid-entry`, `:empty`, or `:valid`; its three bounds use
the closed positive-limit state above. Only valid lists carry nonempty trimmed
strings, and only valid bounds carry positive platform-sized integers. Denied
operations must carry no base directory, capability, byte policy,
process-program policy, or database policy. Malformed requests return sealed errors.
The host rejects
unknown fields, identity drift, request-hash substitution, invalid path types,
denied non-nil state, admitted non-map capabilities or private policies,
noncanonical false/zero/negative/overflowing controls, contradictory status/limit
pairs, noncanonical or contradictory process-program or database states, operation
substitution inside the capability, and any result that
contradicts its retained compatibility oracle. After validation, the host
installs the GenesisCode-selected base directory, create-directories flag,
timeout, per-operation log limit, closed max-byte state, closed normalized
process-program state, and closed database allowlist/bound states into enforcement;
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
`allow_programs` likewise retains effect-use timing and exact errors: missing,
non-array, non-string-entry, and empty-after-trimming configurations are installed
as typed states and rejected only when `sys/process::exec` or `spawn` is used.
Database policy retains the same effect-use timing and exact errors. Missing,
ill-typed, non-string, empty-after-trimming, nonpositive, and overflowing states
are installed and rejected only by the SQL or KV operation that consumes them.

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
against the capability-file base. The private max-byte, process-program, and database states
likewise remain out of the descriptor. Every production generic or bridge byte-limit consumer checks
the installed authority state before the raw compatibility field; raw fallback is
reachable only for policies constructed without the self-host authority by
explicit compatibility and test routes.
Process launch dispatch follows the same rule: it consumes the installed
GenesisCode process-program state before the raw compatibility field, then the
host enforces exact, `*`, or suffix-`*` matching without selecting the rule.
Database dispatch likewise consumes installed GenesisCode allowlists and bounds
before raw compatibility fields. Rust validates URL shape, performs the selected
matching rule, injects authorized bounds, and executes the bridge.
Host code retains payload measurement and enforcement mechanisms, filesystem path resolution,
accounting mechanisms, cancellation, effect execution, and replay mechanisms.
`CapsPolicy::from_toml_str`, `CapsPolicy::empty`, and the independent legacy
composition oracle remain reachable for tests, host mechanisms, and this partial
transition, and therefore are not evidence of H2.

## Residual Decisions And Nonclaims

The machine profile lists the complete residual boundary. It includes TOML syntax
and type decoding; global store remote transport, TLS, and authentication policy;
operation-specific network, crypto, FFI, plugin, model,
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
Process policy configuration is no longer residual: GenesisCode owns the complete
`allow_programs` domain, normalization, and error state for both launch operations;
payload decoding, wildcard matching, process creation, lifecycle control, and
hard cancellation remain host enforcement/execution mechanisms.
Database policy configuration is no longer residual: GenesisCode owns target and
query-class allowlist normalization plus SQL/KV result, row, and value bound
states. URL parsing, matching, bridge transport, database execution, and
measurement remain host enforcement/execution mechanisms.

This contract does not promote `SD-EFFECT-POLICY`, close R4.2.d or SH-C, establish
H2/H3/H4, authorize release, or authorize GenesisBench, Genesis Foundry,
GenesisChallenge, or Genesis Model work. It is an independently checked partial
production shadow that narrows the next migration frontier without weakening the
current host enforcement boundary.
