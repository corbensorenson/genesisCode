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

The Rust v0.2 implementation uses `Rc`, `Arc`, `Box`, and persistent collection nodes for many of
these edges. Those types are not part of the language contract. A tracing heap, arena, bytecode VM,
or target runtime may represent the same logical graph differently if observable behavior, hashes,
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

The v0.2 `Rc` representation does not reclaim every unreachable recursive-module cycle. This is an
explicit implementation gap, not accepted release behavior. `ROADMAP.md` R2.2.b owns the cycle
solution and R2.2.e owns retained-root and persistent-sharing stress. A conforming long-lived runtime
must reclaim an unreachable cycle without requiring user action, process exit, a hidden source
restriction, or nondeterministic finalizer behavior.

### Sharing and mutation

GenesisCode values are observationally immutable. Implementations may mutate uniquely owned
storage, use copy-on-write, or share persistent nodes. Once aliases exist, an update creates a new
logical value and cannot alter any previously observable alias. Pointer identity and structural
sharing are not exposed by equality, printing, hashing, logs, serialization, or effects.

The only v0.2 interior mutation that participates in language evaluation is controlled module-scope
initialization and revision tracking. It exists to implement top-level recursion and forward
definitions. User code cannot obtain the module table, mutate a captured frame, install a value into
an arbitrary slot, or observe its address or reference count.

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

Current `MemLimits` are deterministic semantic safety valves: total pair cells and maximum observed
vector, map, bytes, and UTF-8 string lengths. They are not total live-heap accounting and do not
approximate process RSS. R2.2.c must add logical allocation and live-heap units for every value edge
and root class without making allocator size, address, collection timing, or host platform part of
semantics.

All execution tiers must charge the same documented logical events. Reclamation may lower a live
heap counter only when the logical subgraph is unreachable from every declared root. A tier cannot
avoid a limit by sharing nodes differently, delaying a charge, switching representation, forcing a
collection, or moving an allocation into host code.

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
8. The implementation publishes maximum pause/work bounds or uses isolation where a single bounded
   request could otherwise starve unrelated work.

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
