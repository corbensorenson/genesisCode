# Runtime Value Graph, Lifetime, and Hashing v0.2

This document is **normative** for the GenesisCode v0.2 runtime value graph, lifetime model,
value hashing, and effect request hashing. Effect logs and replay depend on the stable hashing
rules. Resource enforcement and every future execution tier depend on the same graph and lifetime
rules.

## Runtime Value Graph

### Terms, values, roots, and edges

A **term** is an immutable, finite CoreForm tree. A term can contain terms, strings, bytes, integers,
symbols, pairs, vectors, and maps, but it cannot contain a runtime `Value`, closure, seal, contract,
effect continuation, or host handle. CoreForm construction therefore cannot create a reference
cycle.

A **value node** is one runtime `Value` allocation or one allocation reachable from it. A **strong
edge** keeps its target live. A **root** is a value held directly by the evaluator, caller, runner,
session, module result, or host API. Physical reference counts, addresses, allocation order, and
storage layout are implementation details and are not language-visible identity.

The v0.2 value variants and their logical outgoing edges are:

| Value variant | Logical outgoing edges | Cycle notes |
|---|---|---|
| `Data` | one immutable CoreForm term | acyclic |
| inline `Int` | none | acyclic |
| `Vector` | ordered element values | can join a cycle if an element reaches a recursive module |
| `Map` | CoreForm keys and ordered value entries | keys are acyclic; values can join a recursive-module cycle |
| `Closure` | parameter/body metadata and captured `Env` | cycle-capable through a recursive module scope |
| `CompiledClosure` | parameter/body metadata, compiled expression, external `Env`, captured lexical slots, and module cells | cycle-capable through module cells or the external recursive scope |
| `SealToken` | none | acyclic |
| `Sealed` | payload value | inherits payload reachability |
| `NativeFn` | collected partial-application arguments | inherits argument reachability |
| `Contract` | handler, optional prior prototype, metadata, and override handlers | prototype construction is append-only and acyclic; contained handlers or metadata can reach a module cycle |
| `EffectProgram` | pure result or sealed request | inherits contained reachability |
| `EffectRequest` | immutable CoreForm payload and continuation | the continuation can reach a module cycle |

The Rust v0.2 implementation places every allocation that can transitively own a runtime `Value`
behind the trace-aware `Shared<T>` abstraction. `Shared<T>` currently uses the pinned `rust-cc`
collector with automatic collection and user finalization disabled. Vectors and ordered maps use
trace-aware persistent nodes plus bounded copy-on-write transients as specified below. Ordinary
`Rc` remains only for immutable CoreForm terms and symbol text;
`Arc` remains only for immutable compiler expressions, coverage tables, and optimization metadata.
These Rust types are not part of the language contract. A tracing heap, arena, bytecode VM, or
target runtime may represent the same logical graph differently if observable behavior, hashes,
resource charges, and reclamation requirements remain equal.

### Environments and closure capture

Tree-walk lexical environments form a parent chain. Ordinary closures retain only bindings selected
by scope-aware free-variable analysis. A closure that requires recursive or forward module names
also retains the nearest marked module scope, whose binding table is updated as top-level `def`
forms complete.

Compiled closures retain only compiler-resolved lexical slots required by the body, plus the shared
compiled module cells needed for recursive and forward module references. External bindings remain
in an `Env`. Compiled expression and coverage tables are immutable and may be shared across any
number of closures.

Capture minimization is an implementation obligation, not permission to change scope. Removing an
unreferenced binding must not change values, hashes, errors, coverage, effects, or resource charges.
Retaining an unrelated lexical frame indefinitely is a runtime defect even when semantics remain
correct.

### Recursive module cycles

Recursive and mutually recursive definitions require a backedge. The current implementation has
two cycle-capable anchors:

1. A tree-walk module `Env` owns its binding values, while a bound closure can own that same `Env`.
2. Compiled module cells own their values, while a compiled closure can own those same module cells.

The cycle may be indirect. For example, a module cell can own a vector, map, sealed value, contract,
native partial application, or effect continuation that eventually owns a closure pointing back to
the module. Cycle handling therefore cannot special-case only a direct function binding.

The v0.2 Rust runtime traces all logical edges listed above and reclaims unreachable direct and
indirect recursive-module cycles. It does not special-case function bindings: the reclamation corpus
routes cycles through vectors, maps, sealed payloads, native partials, every contract edge, pure and
perform effect programs, and effect continuations. `ROADMAP.md` R2.2.e separately owns retained-root
and adversarial sharing bounds. Every conforming target runtime must provide equivalent reclamation
without requiring user action, process exit, a hidden source restriction, or nondeterministic
finalizer behavior.

### Collection safe points and work

The Rust collector runs explicitly after the outermost evaluator or value-application boundary has
unwound all internal borrows. Caller-owned inputs, environments, and a successful returned value
remain roots at that point. The same safe point runs after explicit errors and caught panics so a
failed request cannot indefinitely retain cycles. Nested evaluator calls and native callbacks never
collect independently. A collector panic is caught by the same boundary and becomes an explicit
`KernelErrorKind::Internal`; it cannot unwind through the public evaluation API.

Automatic allocation-triggered collection is disabled. Each safe point examines the collector's
buffered cycle candidates and their traced closure exactly according to the pinned collector
algorithm. For `V` candidate-reachable traced nodes and `E` traced edges, pause work and temporary
collector state are `O(V + E)`; no scan of immutable CoreForm or compiler metadata is required.
Untrusted warm requests execute in isolated, resource-bounded workers, so a bounded request cannot
monopolize unrelated session work. Representation-independent logical allocation and live-heap
ceilings are defined below; the collector's physical byte counters and invocation timing are not
substitutes for those semantic limits.

### Sharing and mutation

GenesisCode values are observationally immutable. Implementations may mutate uniquely owned
storage, use copy-on-write, or share persistent nodes. Once aliases exist, an update creates a new
logical value and cannot alter any previously observable alias. Pointer identity and structural
sharing are not exposed by equality, printing, hashing, logs, serialization, or effects.

The only v0.2 interior mutation that participates in language evaluation is controlled module-scope
initialization and revision tracking. It exists to implement top-level recursion and forward
definitions. User code cannot obtain the module table, mutate a captured frame, install a value into
an arbitrary slot, or observe its address or reference count.

### Persistent collection bounds

The Rust v0.2 vector is a balanced sequence tree with at most 32 values per leaf. A retained point
update copies only the root-to-leaf path; append mutates a unique path or creates a bounded number
of balanced path nodes. Across `U` retained updates of a vector with `N` elements, physical node
growth is `O(N / 32 + U log N)`, not `O(U * N)`. Indexing and iteration remain ordered by source
position, and physical tree shape cannot affect equality, printing, hashing, or serialization.

The Rust v0.2 ordered map uses a bounded transient-to-persistent transition. A small or uniquely
owned map uses the standard ordered transient representation. The first update to aliased storage
with at least 4,096 entries freezes it once into sorted 32-way pages; subsequent retained updates
copy only one page path. Below that boundary, a copy can contain at most 4,096 entries. Thus no
single retained update copies an unbounded flat map, and `U` retained updates after freezing grow
physical storage by `O(N / 32 + U log_32 N)` pages. Page occupancy, freeze timing, and insertion
history remain unobservable; iteration is always canonical CoreForm key order and merge remains
right-biased.

These bounds are implementation constraints, not new language-visible limits. The conformance
suite retains 1,025 versions of 4,096-element collections while counting distinct physical nodes,
and the isolated composite lane retains 4,097 versions at the map transition boundary while also
exercising bounded strings, package graphs, effect logs, and workspace snapshots. A zero-node or
flat-copy-only map result is a test failure rather than acceptable vacuous evidence. Other runtimes
may use HAMTs, RRB trees, arenas, or equivalent representations if they prove no weaker asymptotic
retained-root bound and preserve every semantic observation.

## Lifetime and Reclamation

### Root classes

At a boundary, implementations must account for at least these root classes:

- the current expression, evaluator stack or continuation stack, and lexical/module environments;
- the caller-owned input values and returned result;
- the current effect program, request continuation, replay state, and deterministic log material;
- Prelude and module exports retained by a warm session;
- in-flight agent transaction, task, and worker-owned values;
- explicit host-runtime resource tables, which are outside the kernel value graph.

Temporary evaluator values cease to be roots when their operation completes. Request roots cease to
be roots on success, sealed or explicit error, cancellation, timeout, client disconnect, or session
teardown, subject only to an explicitly documented returned value or durable artifact. Cache entries
are roots only for their declared lease and quota; a cache cannot silently become permanent heap
authority.

### Finalizers, destructors, and weak references

GenesisCode v0.2 exposes no user finalizers, destructors, weak references, resurrection, pointer
identity, or collection trigger. Reclamation timing and drop order are not observable semantics.
Programs cannot depend on garbage collection to perform an effect, release a capability, write a
log entry, or choose a replay result.

Internal weak references or tracing metadata are permitted only as representation details. They
must never cause a live value to disappear, turn reclamation order into a semantic input, or alter
canonical identity. If weak references are ever added to the language, they require a new profile
with deterministic observation rules; they cannot be introduced as an unversioned primitive.

### Host handles and effects

Kernel values never contain an operating-system file, socket, process, thread, timer, device,
graphics object, GPU allocation, model session, or other ambient host handle. An effect request
contains only a qualified operation, a CoreForm payload, and a continuation. When an operation needs
stateful host resources, the runner owns the real handle in a scoped resource table and exposes only
a bounded logical identifier in data.

The runner, not garbage collection, owns host cleanup. Every handle must have one explicit owner,
capability and workspace scope, creation budget, use budget, close transition, and teardown path.
Success, error, denial, cancellation, hard timeout, worker failure, client disconnect, and daemon
restart must close or reap the handle exactly once where the host permits. A stale, cross-session,
or already-closed logical identifier fails with a sealed host error and cannot revive authority.
R2.2.f owns complete implementation evidence for these paths.

### Resource accounting boundary

`MemLimits` exposes two graph-wide limits in addition to the existing pair-cell and maximum
vector/map/bytes/string shape valves:

- `max_alloc_units` bounds cumulative logical allocation units since the last trusted counter reset;
- `max_live_units` bounds logical units reachable from the declared roots at a safe point.

These counters never approximate process RSS, allocator bytes, collector metadata, compiler IR, or
host resources. A missing limit disables its traversal/charging path and reports zero for that
counter. A configured limit is inclusive: `observed == limit` succeeds and `observed > limit` fails.
All additions saturate at `u64::MAX`, which is itself a valid configured limit rather than an
unlimited sentinel.

#### Logical unit schedule

One CoreForm node costs one unit. Each pair field, vector element, and map key/value relation costs
one edge unit. UTF-8 strings and symbols cost one unit per encoded byte, byte strings cost one unit
per byte, and integers cost one unit per byte in their canonical signed little-endian encoding.

A newly constructed runtime value charges the following cumulative units:

| Value | Allocation units |
|---|---|
| `Data(term)` | one value + one term edge + the complete term tree |
| inline `Int`, `SealToken` | one value |
| `Vector(values)` | one value + one edge per element |
| `Map(entries)` | one value + a key edge and key term tree + a value edge per entry |
| `Closure`, `CompiledClosure` | one value + parameter UTF-8 bytes + body edge/tree + environment edge |
| `Sealed(payload)` | one value + one payload edge |
| `NativeFn` | one value + native name UTF-8 bytes + one edge per collected argument |
| `Contract` | one value + handler and metadata edges + optional prototype edge + override-name bytes and handler edge per override |
| `EffectProgram` | one value + one result/request edge |
| `EffectRequest` | one value + operation-name bytes + payload edge/tree + continuation edge |

Cloning a runtime owner or immutable term does not charge a new logical allocation. Creating a new
language value after copy-on-write or persistent update does charge the applicable constructor row.
Compiler expressions, coverage tables, inline slots, collector bookkeeping, and physical node
packing never add units. The compiled tier disables allocation-eliding application plans while any
semantic memory limit is active, and replays the same curried constructor events as the reference
tier.

Live traversal begins with one edge unit for each declared root. Every reached runtime `Value`
occurrence costs one unit and its outgoing edges, metadata bytes, and CoreForm trees use the same
schedule above. An environment costs one unit, a parent edge when present, and binding-name bytes
plus one value edge per binding. Tree-walk and compiled closures expose the same semantic
parameter/body/environment graph; compiled-only lexical layout and module storage do not change the
charge.

Physical aliases are expanded once per logical incoming path, so replacing two equal values with a
shared physical owner cannot lower the result. Traversal tracks owner identity only on the active
path to terminate cycles; the closing edge is charged, but an already-active owner is not expanded
again. Traversal order, addresses, hash-table layout, reference counts, and collection timing cannot
change the total.

#### Safe points and failure

Allocation charging is active only inside the outermost public evaluation or value-application
transaction. Nested evaluator calls and native callbacks contribute to that same cumulative ledger
without creating extra checkpoints. Allocation exhaustion is checked before the outer boundary
returns. Live units are then measured from the successful result plus the module/term environment;
a direct value application declares its result as its boundary root. Runner/session/effect roots are
owned and bounded by their corresponding outer policies until those subsystems join this graph
meter.

`reset_counters` is a trusted initialization boundary: it resets steps, shape observations,
cumulative allocation, current live units, and peak live units together. Tooling uses it after
Prelude/self-host bootstrap so user budgets do not include trusted initialization.

Exhaustion is an internal `KernelErrorKind::MemoryLimit` carrying the exact dimension, observed
units, and limit. A language or host boundary converts that structure to the reserved ERROR seal
with code `core/resource-exhausted` and a context map containing `:dimension`, `:observed`, and
`:limit`; user code cannot forge recognition because it does not possess the ERROR token. The
message string is explanatory and is not a routing identifier.

All execution tiers must charge the same documented logical events. A tier cannot avoid a limit by
sharing nodes differently, delaying a charge, switching representation, forcing collection, or
moving a language allocation into host code.

#### Physical allocation failure

User-sized bulk outputs compute their exact byte or element capacity with checked arithmetic, apply
the semantic shape limit, and only then use a fallible host reservation. Compiled-module decoding
also rejects declared collection counts that cannot fit the remaining input before reserving. A
failed reservation returns an explicit `KernelErrorKind::MemoryLimit`; it has no fabricated
`:limit`, because the host allocator does not expose a deterministic numeric ceiling. Small fixed
runtime allocations and third-party allocator internals can still abort rather than return a Rust
error, so this is not represented as a universal in-process OOM guarantee.

Long-lived native macOS/Linux services therefore execute every untrusted command in a separately
reapable process tree. A residual allocator abort or external fatal signal terminates only that
worker, produces a `worker-signal-contained` audit and typed request failure, and leaves the daemon
initialized for subsequent work. Resource-monitor kills remain classified by their measured
dimension. WASI's inline worker profile cannot provide process OOM isolation and must continue to
advertise that weaker boundary rather than claiming parity.

### Cycle-solution acceptance constraints

R2.2.b may choose structural cycle prevention, tracing collection, arena ownership, or another
deterministic design. It is acceptable only if all of the following hold:

1. Recursive and mutually recursive modules, first-class closures, containers of closures,
   contracts, sealed values, native partials, and effect continuations retain existing semantics.
2. Unreachable direct and indirect module cycles are reclaimed in bounded long-lived-session tests.
3. Live roots are never collected, and collection cannot forge or expose a seal or host handle.
4. Collection order and timing do not alter values, hashes, logs, effects, errors, scheduling, or
   logical resource charges.
5. Tree-walk, compiled AST, bytecode, WebAssembly, and any later tier pass the same graph and limit
   corpus.
6. Hash cycle handling remains byte-compatible unless a reviewed hash/log profile migration is
   shipped.
7. Malformed, exhausted, or adversarial graphs return sealed or explicit bounded errors; user input
   cannot panic or terminate the daemon.
8. The implementation publishes pause/work bounds and uses isolation where a single bounded request
   could otherwise starve unrelated work.

This contract deliberately separates graph definition from cycle implementation. Hash cycle
breaking makes hashing total; it does not reclaim memory and cannot be cited as cycle-collection
evidence.

## Value Hash (`gc_kernel::value_hash`)

`value_hash(v)` is BLAKE3 over a structured tagged encoding. Each variant contributes a domain tag and then its fields.

Important properties:
- Hashing is stable and deterministic.
- Hashing is total for all runtime values.
- Hashing of closures includes the closure body and the captured environment so that continuation hashes are replayable.

This version (v0.2) uses an encoding that is amenable to caching of shared environment prefixes; this does not change the output semantics, but avoids pathological blowups when many closures capture large shared environments.

### Data Values

- `Value::Data(t)` hashes as `BLAKE3("GCv0.2\\0value\\0data\\0" || hash_term(t))`.

### Closures

- Hash includes:
  - tag `GCv0.2\0value\0closure\0`
  - parameter name bytes
  - the closure body as `hash_term(body)` (canonical CoreForm hash)
  - the captured environment hash

Environment hashing:
- hashed as a persistent chain of frames (parent-first)
- each frame hashes its bindings in stable key order (by binding name string)
- values inside the environment are hashed recursively by `value_hash`

### Recursive Environments (Cycle Handling)

Top-level `def` bindings are evaluated in a *recursive module scope*: later `def`s become visible to earlier closures,
and mutual recursion is supported. This can introduce cycles in the runtime graph (e.g., a function bound in a scope
closes over that same scope).

`value_hash` remains total by defining a cycle break rule:
- While hashing an environment frame, if hashing re-enters the *same* frame before completing, the re-entrant
  `hash_env` call returns `BLAKE3("GCv0.2\\0env-cycle\\0")`.

This rule is stable and deterministic and prevents stack overflows during hashing. Implementations may cache
environment hashes keyed by `(frame_identity, revision)` but must respect this cycle break rule.

### Seal Tokens and Sealed Values

- Tokens hash by identity (`SealId`).
- Sealed values hash as tag `GCv0.2\0value\0sealed\0`, token id, and payload hash.

### Native Functions

- Hash includes:
  - name
  - arity
  - collected (partially applied) arguments, hashed recursively

### Contracts

- Contracts hash by stable `contract_id` only (not by the whole structure).

### Effect Programs / Requests

- Effect programs hash their structure (`pure` vs `perform`) and contained values.
- Effect requests hash op symbol, payload term hash, and continuation hash.

## Effect Request Hash (Log `:req-h`)

For a performed effect request with:
- op symbol `op` (string)
- payload datum hash `payload_h` (bytes32)
- continuation hash `cont_h` (bytes32)

the request hash is:

`BLAKE3( "GCv0.2\\0effect-req\\0" || op || "\\0" || payload_h || cont_h )`

This hash is recorded in logs and must match during replay.

## Stability Requirements

- Any change to `value_hash` or request hashing is a compatibility break for `.gclog` replay.
- If such a change is required, bump log version and/or the version tag prefix so mixed logs are rejected deterministically.

## Log Version Note

GenesisCode v0.2 uses `.gclog :version = 3` for the current `value_hash` encoding (parser remains backward-compatible with legacy `:version = 2` logs).
