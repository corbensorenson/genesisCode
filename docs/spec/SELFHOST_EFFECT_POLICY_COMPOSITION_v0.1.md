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
The authority also owns the complete per-operation network policy shared by
`io/net::*`, `core/sync::*`, package publication, and store remote access:
independent `url_allow` and `remote_allow` normalization, `allow_http`,
`wasi_network_profile`, listener host and port allowlists, and
`max_request_bytes`. URL/authority parsing, matching, target-specific WASI
backend availability, transport, and byte enforcement remain host mechanisms.
The authority also owns the complete per-operation crypto policy consumed by
`core/crypto::{hash,sign,verify,kdf,aead-seal,aead-open}`: algorithm and key-ID
allowlist normalization plus twelve operation-specific byte-limit states.
Algorithm names are trimmed and ASCII-lowercased; key IDs are trimmed without
case folding. Algorithm/key matching, key custody, cryptographic execution,
payload measurement, and output enforcement remain bounded host mechanisms.
The authority also owns the plugin, command, and optional schema-ID allowlist
states consumed by `host/plugin::command` and `editor/plugin::command`.
It additionally owns whether a configured process bridge requires an executable
digest pin and canonicalizes the optional SHA-256 value into a closed typed
state. It also owns command and fixed-argument selection, compatibility argument
filtering, transport-mode selection and invalid-mode classification, and
configured WASI-profile activation. It owns whether an explicit bridge profile
is active from the command, inline WASI response, WASI response file, or WASI
profile fields; every first-party-versus-bridge branch consumes that closed fact.
For `gpu/compute::*` and `gfx/gpu::*`, it also owns canonical GPU backend
selection and device-error fallback policy, including explicit-over-host-default
precedence. Device operation applicability, discovery and execution, fallback
response decoration, and resource lifecycle remain host mechanisms.
For `gfx/window::*`, `gfx/input::*`, and `gfx/audio::*`, it owns first-party
profile selection, including primary-over-alias precedence, production runtime
fallback, and the compile-target production default supplied as an explicit host
observation. Window, input, audio, rendering, adapter, and resource lifecycle
execution remain host mechanisms.
For `gfx/xr::*`, it owns backend classification from `xr_backend`, primary/alias
runtime-profile fallback, the GenesisCode-produced bridge-active decision, and
all haptics and advanced-feature policy states. This includes haptics input and
amplitude/duration bounds; hand-tracking, hit-test, and spatial-mesh gates and
bounds; anchor-space and layer-type allowlists; and anchor, mesh, layer, and
opacity bounds. WebXR bridge availability and execution, replay envelopes,
payload handling, matching and bound enforcement, adapters, and resource
lifecycle remain host mechanisms.
Allowlist matching, command path resolution,
executable hashing and digest comparison, schema validation, bridge transport
execution, cancellation, and model-provider lifecycle remain host mechanisms.
The authority also owns the ABI-ID, library, symbol, and optional schema-ID
allowlist states plus the buffer and call-payload positive-bound states shared by
`host/ffi::call`, `host/ffi::buffer-pin`, and `host/ffi::buffer-unpin`.
Allowlist matching, signed-policy provenance validation, bridge command
resolution, executable hashing and digest comparison, schema implementation,
payload measurement, bridge execution, cancellation, transport, provider
lifecycle, and replay remain bounded host mechanisms.
`core/effects::policy-inventory-authority` owns deterministic union,
deduplication, and ordering of baseline and per-operation candidate names.
`core/effects::resource-policy-authority` owns global log/store byte budgets,
log/store/refs configured-or-default location selection, global store remote
target/allowlist/HTTP states, runtime and task resource limits, and selection of
an explicit task worker default from the configured value or the host's bounded
available-worker observation.

The Rust host still parses TOML, independently reconstructs the legacy candidate
inventory, per-operation results, and log/refs/runtime/store/task resource
policy, and rejects every contradiction.
That live oracle is a required safety mechanism for this partial checkpoint and
prevents `SD-EFFECT-POLICY` from reaching H2. Removing it before all residual
decisions are GenesisCode-owned and independently verified is forbidden.

## Closed Protocol

Each request is a closed nine-field map with kind
`genesis/effect-policy-authority-request-v0.19`, version `19`, the operation string,
the complete ordered baseline allow vector, a positive host
`:platform-max-bytes` observation equal to the target `usize` maximum, and either
`nil` or an exact override map containing `:allow`, `:base-dir`, `:create-dirs`,
`:timeout-ms`, `:log-inline-max-bytes`, `:max-bytes`, `:process-programs`,
`:database-policy`, `:network-policy`, `:crypto-policy`, `:plugin-policy`,
`:ffi-policy`, and `:bridge-identity-policy`.
The top-level `:gfx-policy` observation is an exact five-field map containing
`:production-default`, `:profile`, `:profile-alias`, `:runtime-profile`, and
`:runtime-profile-alias`. The production default is exactly `:headless`,
`:interactive`, `:desktop`, or `:browser`; configured fields carry their exact
string, `nil`, or `:invalid-type`. GenesisCode preserves present-primary-over-alias
precedence even when the primary has the wrong type, trims and ASCII-lowercases
recognized strings, maps `production` and `prod` explicit profiles to the bound
compile-target default, maps unsupported explicit profiles to `:headless`, and
uses the production default for an absent or malformed explicit profile only
when the selected runtime profile is `production`, `prod`, or `release`.
The top-level `:gpu-policy` observation is an exact three-field map containing
`:backend`, `:fallback-override`, and `:fallback-default`. The first two carry
the exact configured string, `nil`, or `:invalid-type`; the default carries the
exact host-observed `GENESIS_GPU_BACKEND_POLICY_DEFAULT` string or `nil`.
GenesisCode trims and ASCII-lowercases these observations, recognizes only the
three documented backend values, preserves explicit-over-default fallback
precedence, and maps unsupported values to the compatibility defaults. The
request hash binds the ambient default observation used for the decision, so the
runtime consumer never rereads the environment.
The top-level `:xr-policy` observation is an exact 18-slot vector. In order, its
slots are `allow_anchor_spaces`, `allow_hand_tracking`,
`allow_haptics_inputs`, `allow_hit_test`, `allow_layer_types`,
`allow_spatial_mesh`, `xr_backend`, `max_anchors`, `max_hand_joints`,
`max_haptics_amplitude`, `max_haptics_duration_ms`, `max_hit_results`,
`max_layer_opacity`, `max_layers`, `max_mesh_vertices`, `max_meshes`,
`runtime_profile`, and `host_runtime_profile`. Lists carry exact strings or the
closed `:invalid-entry` observation; booleans, strings, and integers carry their
exact value, `nil`, or `:invalid-type`. The fixed positional transport avoids
repeated map-allocation cost while its exact length and per-slot domains remain
closed and request-hash bound. GenesisCode preserves
present-primary-over-alias runtime precedence, trims and ASCII-lowercases
strings, canonicalizes documented first-party and WebXR aliases, and returns an
exact backend/invalid-value pair. Production aliases select WebXR only when the
GenesisCode-composed bridge policy is active; otherwise they return the closed
`:production-requires-bridge` state. Unsupported strings retain only their
canonical normalized token in the closed invalid state. It trims all XR
allowlists, removes empty entries, preserves order and duplicates, and
ASCII-lowercases only anchor-space and layer-type values. It classifies optional
booleans and positive integer limits without making an unrelated malformed field
reject policy loading; the consuming XR operation receives the stable policy
error. `max_haptics_amplitude` additionally classifies values above 1000 as
`:out-of-range`.
The nested database map has exactly `:target-allow`, `:query-classes`,
`:max-result-bytes`, `:max-row-count`, and `:max-value-bytes`. The base directory is
`nil` or the exact configured string. Missing optional fields use `nil`. A TOML
integer is transported exactly for `:max-bytes`; a present non-integer is
transported as the closed `:invalid-type` observation so GenesisCode, rather than
Rust, decides its effect-use error state. Missing `allow_programs` is transported
as `nil`, a non-array as `:invalid-type`, and array entries as their exact strings
or the closed `:invalid-entry` observation. Database allowlists use the same
exact string or closed invalid observation transport; database bounds use exact
integers or `:invalid-type`. The nested network map has exactly `:url-allow`,
`:remote-allow`, `:allow-http`, `:wasi-network-profile`, `:bind-hosts`,
`:bind-ports`, and `:max-request-bytes`. String allowlists use the same exact
transport as database lists; optional boolean/string fields and the positive
limit use closed invalid observations; bind-port entries are exact integers,
exact strings, or `:invalid-entry`. No omitted or additional field is accepted. A
policy may expose at most 4,096 unique candidate operations.
The nested crypto map has exactly `:algorithms`, `:key-ids`,
`:max-aad-bytes`, `:max-ciphertext-bytes`, `:max-context-bytes`,
`:max-info-bytes`, `:max-input-bytes`, `:max-message-bytes`,
`:max-nonce-bytes`, `:max-output-bytes`, `:max-plaintext-bytes`,
`:max-salt-bytes`, `:max-signature-bytes`, and `:max-tag-bytes`. Its two
allowlists use exact string or closed invalid observation transport; every bound
uses an exact integer or `:invalid-type`.
The nested plugin map has exactly `:plugins`, `:commands`, and `:schema-ids`;
all three use exact string or closed invalid observation transport.
The nested FFI map has exactly `:abi-ids`, `:libraries`, `:symbols`,
`:schema-ids`, `:max-buffer-bytes`, `:max-call-payload-bytes`,
`:signed-policy-required`, `:policy-artifact-h`, `:policy-signature-h`,
`:policy-key-id`, and `:evidence-mode`. Its four allowlists use exact string or
closed invalid observation transport; both bounds use an exact integer or
`:invalid-type`; each optional metadata string uses the exact string, `nil`, or
`:invalid-type`. Rust transports `:signed-policy-required` as its exact boolean,
uses `false` only when the key is absent, and transports a present non-boolean as
`:invalid-type`. GenesisCode owns the resulting fail-closed admission decision;
malformed opt-in cannot silently disable signed-policy enforcement.
The nested bridge identity map has exactly `:allowlist`, `:args`, `:command`,
`:digest`, `:transport`, `:wasi-profile`, `:wasi-response`, and
`:wasi-response-file`. The allowlist and argument vector
use exact string or closed invalid-observation transport; command, digest, and
transport use exact optional-string transport; the WASI flag uses exact
optional-boolean transport. GenesisCode preserves allowlist order
and duplicates, trims every entry, preserves an empty array as a valid deny-all
list, and distinguishes a wrong container, a non-string entry, and an empty
trimmed entry. For compatibility, GenesisCode preserves the command and string
arguments byte-for-byte, maps a malformed argument container to an empty vector,
and skips non-string argument entries. It defaults absent, malformed, blank, and
explicit `spawn-per-op` transport values to spawn-per-operation, selects
`persistent-stdio` exactly after trimming, and preserves any other trimmed token
as an invalid decision. It enables the configured WASI profile only for a literal
true boolean. GenesisCode separately trims the command to decide whether a pin
is required for a non-WASI `host/plugin::*`, `host/ffi::*`, or `editor/*` process
bridge, accepts only an optional exact `sha256:` or `SHA256:` prefix, validates
64 hexadecimal digits, and emits the digest in lowercase.

Before those per-operation requests, the inventory authority receives a closed
four-field `genesis/effect-policy-inventory-request-v0.1` map containing version
`1`, the complete baseline vector, and the complete ordered vector of override
operation names. It validates string membership and returns the strictly ordered,
duplicate-free union in a closed
`genesis/effect-policy-inventory-result-v0.1` map bound to the request hash. The
host rejects malformed, oversized, duplicate, unsorted, substituted, or
oracle-contradicting inventory results and uses only the validated GenesisCode
inventory to drive per-operation composition.

The authority returns a closed eighteen-field
`genesis/effect-policy-authority-result-v0.19` map containing the exact operation,
boolean admission decision, selected `:base-dir`, canonical capability map when
admitted or `nil` when denied, private `:max-bytes-policy` and
`:process-program-policy`, private `:database-policy`, private `:network-policy`,
private `:crypto-policy`, private `:plugin-policy`, private `:ffi-policy`, private
`:bridge-identity-policy`, private `:gfx-policy`, private `:gpu-policy`, private
`:xr-policy`, lowercase canonical request hash, and version `19`.
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
process-program policy, database policy, network policy, crypto policy, plugin
policy, FFI policy, GFX policy, GPU policy, XR policy, or bridge identity policy. The network result preserves independent URL and remote list states,
closed optional boolean/string
states, a closed bind-port state (`:absent`, `:invalid-type`, `:invalid-entry`,
`:out-of-range`, `:empty`, or `:valid`), and a closed request-byte bound. Only a
valid bind-port state carries an exact wildcard boolean and ordered in-range port
vector. The crypto result is an exact fourteen-field map. Its two allowlists use
the same closed list state; only algorithm values are ASCII-lowercased. Its
twelve bounds use the closed positive-limit state above. The plugin result is an
exact three-field map whose values use the closed list state above. Malformed
requests return sealed errors. The FFI result is an exact seven-field map whose
four allowlists use the closed list state above and whose two bounds use the
closed positive-limit state above. Its `:signed-policy` field is an exact
five-field map containing `:status`, `:policy-artifact-h`,
`:policy-signature-h`, `:policy-key-id`, and `:evidence-mode`. Status is exactly
`:disabled`, `:invalid-required-type`, `:missing-artifact-h`,
`:empty-artifact-h`, `:invalid-artifact-h`, `:missing-signature-h`,
`:empty-signature-h`, `:invalid-signature-h`, `:missing-key-id`, `:empty-key-id`,
`:missing-evidence-mode`, `:empty-evidence-mode`, `:invalid-evidence-mode`, or
`:valid`. Only `:valid` carries two 64-hex strings, a nonempty trimmed key ID, and
the exact evidence mode `deterministic`; every other status carries four `nil`
metadata values. The host independently rejects contradictory status/value pairs.
The bridge identity result is an exact
`{:active ... :allowlist ... :args ... :command ... :digest ... :pin-required ...
:transport ... :wasi-profile ...}` map. Its allowlist is an exact
`{:status ... :values ...}` state with status `:absent`, `:invalid-type`,
`:invalid-entry`, `:empty-entry`, or `:valid`; only `:valid` carries an ordered
vector of canonical nonempty strings, and that vector may be empty to preserve
deny-all semantics. Its digest is an exact `{:status ... :value ...}` state with
status `:absent`, `:invalid-type`, `:empty`, `:invalid-digest`, or `:valid`; only
`:valid` carries a lowercase 64-hex value. The pin flag is an exact boolean and
cannot be true for an operation outside the three admitted bridge families.
Arguments are an exact string vector, command is `nil` or an exact string, and
WASI profile and activation are booleans. Activation is true exactly when the
configured command, inline WASI response, or WASI response-file string is
nonempty after trimming, or the configured WASI profile is literal true.
Transport is an exact `{:status ... :value ...}` state: `:spawn-per-op` and
`:persistent-stdio` carry `nil`, while only `:invalid` carries a canonical
nonempty token that is neither supported mode.
The GPU result is an exact `{:backend ... :fallback ...}` map. Backend is exactly
`:first-party-runtime`, `:device-runtime`, or `:device-runtime-full`; fallback is
exactly `:allow-fallback` or `:require-device`.
The GFX result is exactly one of `:headless`, `:interactive`, `:desktop`, or
`:browser` for an admitted operation and `nil` for a denied operation.
The XR result is an exact 17-field map containing `:backend`, `:invalid-value`,
`:allow-haptics-inputs`, `:max-haptics-amplitude`,
`:max-haptics-duration-ms`, `:allow-hand-tracking`, `:max-hand-joints`,
`:allow-hit-test`, `:max-hit-results`, `:allow-spatial-mesh`, `:max-meshes`,
`:max-mesh-vertices`, `:allow-anchor-spaces`, `:max-anchors`,
`:allow-layer-types`, `:max-layers`, and `:max-layer-opacity`. Backend is
`:first-party-runtime`, `:webxr-device`, `:production-requires-bridge`, or
`:invalid`; only `:invalid` carries a nonempty canonical string. Optional boolean
decisions are `nil`, `:invalid-type`, or an exact boolean. Positive limits are
`nil`, `:invalid-type`, `:nonpositive`, or a positive integer;
`:max-haptics-amplitude` may additionally be `:out-of-range`. List decisions are
`nil`, `:invalid-type`, `:invalid-entry`, `:empty`, or a nonempty canonical
string vector. The host rejects every other symbol, padded or empty valid list
entry, non-lowercase anchor/layer value, nonpositive valid integer, field
substitution, or denied non-nil XR result.
The host rejects unknown fields,
identity drift, request-hash substitution, invalid path types,
denied non-nil state, admitted non-map capabilities or private policies,
noncanonical false/zero/negative/overflowing controls, contradictory status/limit
pairs, noncanonical or contradictory process-program, database, network, crypto,
plugin, FFI, GFX, GPU, XR, or bridge identity states, operation substitution inside the
capability, and any result that contradicts its retained compatibility oracle.
After validation, the host
installs the GenesisCode-selected base directory, create-directories flag,
timeout, per-operation log limit, closed max-byte state, closed normalized
process-program state, closed database allowlist/bound states, closed network
allowlist/option/bind/bound states, closed crypto allowlist/bound states, and
closed plugin and FFI allowlist/bound/signed-metadata states plus the closed GFX
profile, GPU backend/fallback decision, complete XR device policy, and bridge activation, command allowlist, command,
arguments, transport, WASI profile, digest
requirement, and canonical digest into enforcement; its separately parsed values
are used only by the compatibility oracle.

The resource authority receives a closed eight-field
`genesis/effect-resource-policy-request-v0.4` map. It contains version `4`, the
positive host observation `:available-workers`, and exact `:log`, `:refs`,
`:runtime`, `:store`, and `:task` maps. Missing optional TOML fields are
represented by `nil`. Runtime and task limits must be nonnegative integers, and
a configured `:default-workers` must be positive. Global `:inline-max-bytes`,
`:max-artifact-bytes-per-run`, and `:max-run-bytes` accept the legacy integer
domain and are normalized by GenesisCode so only positive limits survive; zero
and negative values become `nil`. Location inputs are `nil` or strings. The
store map additionally contains an exact `:remote-policy` input with
`:remote`, `:remote-allow`, and `:allow-http`; present wrong types and non-string
list entries are transported as closed invalid observations rather than silently
coerced by Rust.

The closed `genesis/effect-resource-policy-result-v0.4` result is bound to the
complete request hash, preserves the validated limits, replaces a missing task
default with `:available-workers`, defaults store and refs locations to
`.genesis/store` and `.genesis/refs.gc`, and defaults the log store only when the
normalized inline spill threshold is present. Explicit locations always win. Its
closed store remote decision classifies the target as
`absent|invalid-type|empty|valid`, the allowlist as
`absent|invalid-type|invalid-entry|empty|valid`, and HTTP permission as
`absent|invalid-type|valid`; only valid states carry trimmed values. The
host strictly decodes every field into `u64`, platform `usize`, or a UTF-8 path;
rejects invalid result domains and overflow; compares the complete result with
its independently parsed compatibility oracle; installs the validated
GenesisCode log, refs, runtime, store remote, and task values; and only then resolves
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
Network policy likewise retains effect-use timing and exact errors. `io/net`
prefers a present `url_allow` state and otherwise consumes `remote_allow`; sync
and publication consume `remote_allow` independently, so both configured fields
remain observable. Invalid, empty, out-of-range, and overflowing states are
installed and rejected only by a consuming network or remote operation.
Crypto policy likewise retains effect-use timing and exact errors. Missing,
ill-typed, non-string, empty-after-trimming, nonpositive, and overflowing states
are installed and rejected only by the crypto operation that consumes the
corresponding allowlist or bound.
Plugin allowlist policy likewise retains effect-use timing and exact errors.
Missing, ill-typed, non-string, and empty-after-trimming states are installed and
rejected only when a plugin command consumes the corresponding required list, or
when a typed plugin request consumes the optional schema-ID list.
FFI policy likewise retains effect-use timing and exact errors. Missing,
ill-typed, non-string, empty-after-trimming, nonpositive, and overflowing states
are installed and rejected only when an FFI operation consumes the corresponding
required allowlist or bound. The schema-ID allowlist remains optional until a
typed FFI request supplies a request or response schema ID.

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
against the capability-file base. The private max-byte, process-program,
database, network, crypto, plugin, and FFI states likewise remain out of the
descriptor. Every production generic or bridge byte-limit consumer checks the
installed authority state before the raw compatibility field; raw fallback is
reachable only for policies constructed without the self-host authority by
explicit compatibility and test routes.
Process launch dispatch follows the same rule: it consumes the installed
GenesisCode process-program state before the raw compatibility field, then the
host enforces exact, `*`, or suffix-`*` matching without selecting the rule.
Database dispatch likewise consumes installed GenesisCode allowlists and bounds
before raw compatibility fields. Rust validates URL shape, performs the selected
matching rule, injects authorized bounds, and executes the bridge.
Network and remote dispatch consume installed GenesisCode URL/remote allowlists,
HTTP permission, WASI profile, bind rules, and request bound before raw
compatibility fields. Rust parses targets, performs matching, checks actual WASI
backend availability, enforces the selected limits, and executes transport.
Global store and package-registry consumers obtain the configured store remote,
its allowlist, and HTTP permission only from the installed GenesisCode resource
decision. No production store consumer reads those three raw `[store]` fields;
Rust retains URL parsing/normalization and allowlist matching as enforcement.
Crypto dispatch consumes installed GenesisCode algorithm/key-ID allowlists and
all twelve byte-limit states before raw compatibility fields. Rust performs
allowlist matching, key lookup/custody, payload measurement, limit enforcement,
and cryptographic execution without selecting the policy state.
Plugin dispatch consumes installed GenesisCode plugin, command, optional
schema-ID, bridge command allowlist, invocation configuration, digest
requirement, and canonical digest states before raw compatibility fields. Rust
performs matching, executable hashing and digest comparison, schema validation,
and bridge execution without selecting those policy states.
Process, database, network, crypto, editor, browser, GPU, GFX, and XR dispatch
consume the installed bridge-activation fact before selecting a first-party or
bridge path. Those consumers do not reread command, WASI response, response-file,
or WASI-profile fields; a present operation policy without authority enters the
bridge path and fails through the existing sealed bridge-policy boundary rather
than silently selecting a local backend.
GPU dispatch additionally consumes only the installed backend/fallback decision;
it does not reread `gpu_backend`, `gpu_backend_policy`, or
`GENESIS_GPU_BACKEND_POLICY_DEFAULT`. A present operation policy without GPU
authority returns a sealed `gpu/backend-policy` error instead of silently
selecting the in-memory runtime. Rust still decides which operations each device
backend implements and performs device calls, error transport, and fallback
response decoration.
XR dispatch consumes only the installed complete XR policy for backend routing,
haptics admission and bounds, advanced-feature gates, allowlists, and limits. No
first-party XR consumer rereads these raw TOML fields. Rust retains matching,
payload decoding, bound enforcement, deterministic runtime state, adapter and
bridge execution, replay envelopes, and resource lifecycle without selecting
the policy state.
FFI dispatch consumes installed GenesisCode ABI-ID, library, symbol, optional
schema-ID, buffer-bound, call-payload-bound, and closed signed-policy states
plus the installed bridge command allowlist, digest requirement, canonical
digest, and invocation configuration without a raw metadata, allowlist, command,
argument, transport, profile, or digest fallback. Rust maps rejected authority
states to sealed
policy errors and performs provenance/signature verification, matching,
bridge-identity validation, schema validation, payload measurement, bound
enforcement, and bridge execution without selecting those policy states.
Host code retains payload measurement and enforcement mechanisms, filesystem path
resolution, accounting mechanisms, cancellation, effect execution, and replay
mechanisms.
`CapsPolicy::from_toml_str`, `CapsPolicy::empty`, and the independent legacy
composition oracle remain reachable for tests, host mechanisms, and this partial
transition, and therefore are not evidence of H2.

## Residual Decisions And Nonclaims

The machine profile lists the complete residual boundary. It includes TOML syntax
and remaining type decoding; global store credential, TLS, and transport policy;
FFI signed-policy provenance, bridge identity validation/execution, and model
provider lifecycle; secret and path resolution; effect execution and hard
cancellation; strict replay; and removal of the compatibility oracle.
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
Network policy configuration is no longer residual: GenesisCode owns both
per-operation target allowlists, HTTP permission, WASI profile normalization,
bind host/port states, and inbound request-size state across network, sync,
publication, and store-remote operation policies. GenesisCode also owns global
store remote target selection, allowlist normalization, malformed-state
classification, and HTTP permission. TLS credentials, secret/environment
resolution, retry/worker settings, URL parsing and normalization, matching, WASI
backend discovery, DNS/socket/HTTP/WebSocket execution, cancellation, and
measurement remain in the named host residuals.
Crypto policy configuration is no longer residual: GenesisCode owns algorithm
and key-ID list normalization and all twelve positive-limit states across hash,
sign, verify, KDF, AEAD sealing, and AEAD opening. Algorithm/key matching, key
custody and provider configuration, cryptographic implementation, payload
measurement, output enforcement, cancellation, and replay remain host
enforcement/execution mechanisms.
Plugin allowlist configuration is no longer residual: GenesisCode owns the
complete plugin, command, and optional schema-ID list states shared by host and
editor plugin commands. It also owns process-bridge digest-pin requirement
selection, digest normalization, and bridge command allowlist normalization and
malformed-state precedence shared by plugin and FFI bridges. GenesisCode also
owns command/argument selection, compatibility filtering, transport selection
and invalid-mode classification, configured WASI-profile activation, and the
closed bridge-active decision used by every first-party-versus-bridge branch.
Executable path resolution, transport execution, hashing and digest comparison,
schema implementation, matching, model-provider lifecycle, cancellation, and
replay remain in the named host residuals.
GPU backend and fallback policy configuration is no longer residual: GenesisCode
owns normalization, canonical backend selection, explicit-over-default
precedence, and fail-open/fail-closed selection. The
GFX first-party profile decision is also no longer residual: GenesisCode owns
primary/alias precedence, normalization, runtime-profile fallback, and selection
of the bound compile-target production default. XR policy configuration is no
longer residual: GenesisCode owns canonical backend aliases, runtime-profile
precedence, production bridge requirements, invalid-state classification, all
haptics policy, all advanced-feature gates and allowlists, and every XR policy
bound. GPU/XR operation applicability, matching and bound enforcement,
device/graphics execution, adapter implementation, replay envelopes, and
resource lifecycle are enforcement or execution mechanisms under
`effect-execution-and-hard-cancellation`, replay, or bridge lifecycle rather
than policy-selection residuals. Retiring `device-and-graphics-policy` does not
claim that these host mechanisms moved into the pure kernel.
FFI allowlist, byte-bound, and signed-policy admission is no longer residual: GenesisCode
owns ABI-ID, library, symbol, optional schema-ID, buffer-size, and call-payload
states across all three FFI operations. It also owns malformed opt-in rejection,
required-field precedence, hash-form admission, key-ID admission, deterministic
evidence-mode admission, and the closed accepted metadata tuple. The retained
`bridge-identity-validation-execution-and-model-provider-lifecycle` residual covers
signed-policy artifact provenance and cryptographic signature validation,
executable path resolution, transport execution, hashing and digest comparison,
schema implementation, matching,
model-provider lifecycle, cancellation, and replay; it does not cover the
migrated FFI, plugin, bridge-allowlist, bridge-activation, invocation-selection,
digest-requirement, or digest-normalization decisions.

This contract does not promote `SD-EFFECT-POLICY`, close R4.2.d or SH-C, establish
H2/H3/H4, authorize release, or authorize GenesisBench, Genesis Foundry,
GenesisChallenge, or Genesis Model work. It is an independently checked partial
production shadow that narrows the next migration frontier without weakening the
current host enforcement boundary.
