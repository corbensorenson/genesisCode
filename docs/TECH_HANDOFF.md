
Below is a **handoff technical document** you can paste into your repo (or hand directly to a Codex Desktop / GPT‑5.3‑Codex agent). It’s written to be maximally actionable from day 0: repo structure, invariants, module boundaries, file formats, tests, and a task plan that takes advantage of Codex threads \+ worktrees \+ skills.

I’ve also included **templates for `AGENTS.md`** and a **Codex Skill** you can drop into `.agents/skills/` so the agent reliably follows the same workflow every time. Codex explicitly supports project-level instruction files (`AGENTS.md` / `AGENTS.override.md`) and skill folders with `SKILL.md`. ([OpenAI Developers](https://developers.openai.com/codex/guides/agents-md/))

---

# **GenesisCode v0.2 Implementation Guide**

## **Technical Handoff for Codex Desktop \+ GPT‑5.3‑Codex**

### **Purpose**

Implement **GenesisCode v0.2** as specified in the accompanying paper:

* **Kernel:** Gλ (pure), immutable data, lexical closures, and the **seal/unseal** primitive to support unforgeable protocol tags.  
* **Prelude:** contracts \+ hardened protocol (UNHANDLED/EFFECT/ERROR are sealed), delegation via `dispatch`, immutable extension via `extend`, observability via `explain`.  
* **Effects:** explicit capability runner \+ **deterministic effect logs** \+ **replay checker**.  
* **Obligation Engine:** packages carry obligations \+ evidence artifacts; local tooling enforces acceptance policies.  
* **Semantic patches:** changes are represented as structural patch artifacts and validated, not just text diffs.

This guide focuses on building a **high-quality, minimal-TCB interpreter \+ toolchain** with crisp boundaries so later stacks (types, refinement, e-graphs, compiler) can be added without rework.

---

## **0\) Non‑negotiable invariants (read first)**

### **0.1 Trusted Computing Base discipline**

**TCB-A (must remain tiny):**

1. Gλ evaluator (pure)  
2. immutable primitives (pure)  
3. seal/unseal implementation (unforgeable tokens)

Everything else (runner, registry policy, optimizers) should be treated as higher-layer tooling.

### **0.2 Purity boundary is strict**

* Kernel evaluation must not touch filesystem, time, randomness, network, environment vars, or the LLM.  
* Any nondeterminism must be represented as an **effect program** and interpreted by the runner.

### **0.3 No spoofable protocol markers**

UNHANDLED/EFFECT/ERROR must be **unforgeable** by user code:

* Use seal tokens created by Prelude (`S_UNHANDLED`, `S_EFFECT`, `S_ERROR`).  
* Recognition checks must use `unseal`, never raw structure matching.

### **0.4 No host-language panics for user errors**

User-visible failures must be represented as:

* sealed `ERROR(...)` values, or  
* explicit error returns (Result) in Rust internals, converted to a sealed ERROR at the boundary.

---

## **1\) Deliverables (Definition of Done)**

### **Milestone A — Kernel \+ Prelude (minimum viable “language”)**

* CoreForm parser \+ printer (canonical S-exprs)  
* Gλ evaluator: `lambda`, application, `quote`, and a small set of total immutable primitives  
* Immutable value types: nil/bool/int/bytes(or string)/symbol/pair (+ map/vector strongly recommended)  
* Seals: `seal()` produces unforgeable token; `seal(value, token)`; `unseal(sealed, token)`  
* Prelude module:  
  * contract constructor  
  * hardened UNHANDLED/EFFECT/ERROR  
  * `dispatch`, `extend`, `explain`

### **Milestone B — Effects \+ auditability**

* Standard effect IR (Pure/Perform w/ continuation)  
* Capability runner (deny-by-default)  
* Effect log format (deterministic, content-addressed)  
* Replay checker that validates a run against an effect log

### **Milestone C — Obligations \+ packages**

* Package manifest format with obligations and evidence pointers  
* Content-addressed artifact store (hash-based)  
* Test runner \+ evidence artifacts (test logs \+ effect logs)  
* Policy gate: “package accepted only if obligations pass”

### **Milestone D — Semantic patches**

* Patch artifact schema for structural changes  
* Patch apply \+ validator  
* Patch acceptance pipeline reruns relevant obligations

### **Optional (do later, but keep extension points clean)**

* Row-polymorphic contract type checker \+ effect rows as an obligation  
* Refinement proof stack  
* Optimizer (shapes/PICs/e-graphs) with translation validation obligations  
* WASM build target for interpreter/runner (keep the architecture ready)

---

## **2\) Repo layout (recommended)**

Use a Rust workspace so tooling and libraries stay modular:

genesiscode/  
  Cargo.toml  (workspace)  
  crates/  
    gc\_coreform/        \# AST \+ parser/printer \+ canonicalization  
    gc\_kernel/          \# Gλ evaluator \+ values \+ env \+ primitives \+ seals  
    gc\_prelude/         \# contracts \+ dispatch/extend/explain \+ sealed protocol  
    gc\_effects/         \# effect IR \+ runner interface \+ effect log \+ replay  
    gc\_obligations/     \# obligations engine \+ evidence store \+ policy  
    gc\_patches/         \# semantic patch schema \+ apply/validate  
    gc\_cli/             \# CLI executable wiring everything  
  prelude/  
    prelude.gc         \# Prelude in CoreForm (or embedded as Rust string initially)  
  tests/  
    spec/              \# language-level golden tests (CoreForm programs)  
    fuzz/              \# parser/eval fuzz harnesses  
  docs/  
    PAPER\_v0.2.md      \# the paper you wrote  
    TECH\_HANDOFF.md    \# this file

**Rationale:** keep the kernel clean; keep effect/obligation/patch machinery out of TCB-A.

---

## **3\) CoreForm spec (canonical IR)**

### **3.1 Parsing and printing goals**

* Parser must accept a minimal Lisp-like syntax: parentheses, symbols, integers, strings/bytes, quote sugar (`'x` \=\> `(quote x)`).  
* Printer must emit a canonical, stable form (for hashing and patching).  
* Canonicalization must preserve semantics but normalize irrelevant surface differences.

### **3.2 AST representation (Rust)**

Define a single enum:

enum Term {  
  Nil,  
  Bool(bool),  
  Int(BigInt),  
  Bytes(Vec\<u8\>),     // or String  
  Symbol(SymbolId),   // interned  
  Pair(Box\<Term\>, Box\<Term\>),  
  Vector(Vec\<Term\>),  
  Map(BTreeMap\<Term, Term\>), // or persistent map wrapper  
  Quote(Box\<Term\>),          // or (quote ...) as syntactic sugar  
  // optionally: locations/spans for better errors  
}

**Important:** keep “Term” for immutable data and “Expr” for evaluable forms if you prefer, but don’t overcomplicate.

### **3.3 Hashing**

Pick a stable content hash (e.g., BLAKE3) for:

* canonical CoreForm  
* evidence artifacts  
* effect logs  
* patch artifacts

---

## **4\) Gλ Kernel design**

### **4.1 Values (runtime)**

Separate “Term” (syntax/data) from “Value” (runtime):

enum Value {  
  Data(Term),                   // immutable data  
  Closure { param: SymbolId, body: Expr, env: EnvRef },  
  Sealed { token: SealId, payload: Box\<Value\> },  
  SealToken(SealId),  
  // Optional: native function for primitives  
  NativeFn(NativeFnId),  
}

### **4.2 Environment**

Use persistent environments for purity and easier debugging:

* `Env = persistent_map(SymbolId -> Value) + parent pointer`

### **4.3 Evaluation**

Call-by-value, lexical scope.

Keep the kernel forms minimal; treat `let`, `if`, `begin`, multi-arg calls as desugaring helpers in the parser or Prelude.

### **4.4 Seals (unforgeability)**

Implement `seal()` as generating a fresh `SealId` that cannot be constructed by user code. This is a core security guarantee.

Kernel ops:

* `seal()` \-\> `SealToken(id)`  
* `seal(value, SealToken(id))` \-\> `Sealed{id, payload=value}`  
* `unseal(sealed, SealToken(id))` \-\> returns payload if id matches else nil (or sealed ERROR; choose one and standardize)

### **4.5 Reference and optimized execution tiers**

GenesisCode has one semantic reference evaluator and a separately implemented compiled AST tier. The compiled tier may improve representation, lookup, dispatch, allocation, and tail execution, but it does not define language behavior. Normative language and hash specifications remain higher authority than either Rust tier. A mismatch fails the optimized tier; it does not silently redefine the reference or permit a compatibility exception.

`policies/kernel_tcb_contract.toml` assigns every production file under `crates/gc_kernel/src` exactly one exhaustive, non-overlapping role:

* `reference-semantics`: direct CoreForm interpretation and free-variable analysis. It cannot import compiled expressions, closures, runtime environments, forwarding plans, or compiled-runtime helpers.
* `shared-semantics`: deterministic context, limits, coverage, primitive semantics, environments, and explicit errors used by both tiers. It cannot encode compiled-tier shortcuts or benchmark-specific results.
* `tier-bridge`: the public kernel surface and value representation where reference and compiled closures coexist explicitly. It cannot make the optimized tier authoritative.
* `optimized-tier`: CoreForm compilation, compiled artifacts, slot environments, optimized application, and compiled evaluation. It cannot call the treewalk implementation as a hidden fallback.

New production files require an intentional role and line budget in the same reviewed policy change. For every program accepted by both tiers, reference and compiled execution must agree on success or explicit failure; observable value representation and canonical hash; error kind and structured message material; seal creation order and payload behavior; evaluation steps and semantic memory counters; coverage sites and decisions; and tail-call, partial-application, module, and closure behavior. Private optimized state must remain absent from canonical values, hashes, artifacts unless versioned, effects, and resource semantics.

`scripts/check_kernel_tcb_contract.sh` enforces the complete inventory, role partition, forbidden cross-tier markers, evaluator boundary markers, line budgets, and presence of the default differential matrix. Mutation controls prove that role escape, a missing differential suite, and an optimized symbol admitted to the reference path all fail closed. The default `reference_compiled_differential_matrix_covers_semantic_observables` test compares values, hashes, explicit errors, step counters, and memory counters over representative data, collections, closures, shadowing, seals, type errors, unbound names, step exhaustion, and memory exhaustion. Successful cases also run one step short so a tier cannot bypass limits while preserving only the happy-path value.

Any optimized-tier change must keep this gate and the broader kernel differential, coverage, artifact-roundtrip, resource, tail, and panic suites green. A future execution tier must define an equally explicit role and differential or translation-validation boundary before becoming a production path.

Production dispatch must not recognize benchmark IDs, exact source symbols, literal values, expected outputs, or workload AST shapes. The retired `compiled_runtime/patterns.rs` path is a governed tombstone and cannot return under another module name. Optimization plans must be derived from documented semantic properties such as resolved lexical identities and primitive opcodes, remain independent of source spelling and benchmark membership, and preserve all observable semantics under source-equivalent rewrites.

The compiled tail-loop plan is one such semantics-derived optimization. It accepts only a fully applied curried closure whose final body is a conditional with at least one tail call resolved to the same module-closure identity. Its expression subset is constants, resolved local slots, direct primitive opcodes, and closures independently proven to forward arguments to a primitive opcode; sequential `let` and `begin` control are lowered without changing source-order evaluation. Static source-node charges preserve exact step counts. Per-branch last-use counts may move a value from a dead loop-state slot, but repeated or cross-state uses clone it so persistent aliases remain observable. Any step limit, non-default memory limit, coverage run, unsupported form, unresolved callable, arity mismatch, or failed proof selects the ordinary compiled evaluator before arguments are consumed. The plan carries no source names, benchmark IDs, literal-value predicates, expected result material, or result cache.

---

## **5\) Prelude contracts \+ hardened protocol**

### **5.1 Contract representation**

A contract is a callable value with fields:

* `handler: Value` (callable)  
* `proto: Option<Value>` (contract or nil)  
* `meta: Value` (map)  
* optional `shape_id` (later optimization)

In Rust, store contracts as `Rc<Contract>` inside `Value::Contract(...)` or reuse closures with a special apply protocol.

### **5.2 Standard message shape**

Standardize (in docs \+ tests):

* `(msg op payload)` where `op` is a **qualified symbol**.

### **5.3 Sealed protocol constructors**

In Prelude initialization:

* create `S_UNHANDLED`, `S_EFFECT`, `S_ERROR` via `seal()`

Define functions:

* `UNHANDLED(msg)` returns `seal((unhandled msg), S_UNHANDLED)`  
* `EFFECT(req)` returns `seal((effect req), S_EFFECT)`  
* `ERROR(info)` returns `seal((error info), S_ERROR)`

Define predicates:

* `is_unhandled(x)` uses `unseal(x, S_UNHANDLED)`  
* same for effect/error

### **5.4 `dispatch` (normative)**

Implement exactly:

1. call contract handler with msg  
2. if result is sealed UNHANDLED and proto exists: recurse to proto  
3. else return result

### **5.5 `extend`**

Create a new contract whose handler:

* checks override table for `op`  
* if found: call override handler  
* else return sealed UNHANDLED(msg)  
  Proto points to base.

### **5.6 `explain`**

Must return a pure trace object capturing:

* contracts visited (ideally by stable contract-id or hash)  
* whether each step matched an override  
* the final result

This is essential for debugging \+ for AI to self-correct.

---

## **6\) Effects, runner, logs, replay**

### **6.1 Effect IR (standard)**

Represent effectful computations as:

* `Pure(v)`  
* `Perform(op, payload, k)` where `k` is continuation `result -> EffectProgram`

The runner interprets effect programs, not arbitrary “effect values”.

### **6.2 Capabilities**

Runner must be deny-by-default; capabilities are injected by host configuration.

Examples:

* `io/fs.read`  
* `io/fs.write`  
* `sys/time.now`  
* `ai/generate`

### **6.3 Deterministic effect logs**

Define a log entry struct:

* request hash (op \+ payload hash \+ continuation hash)  
* decision (allowed/denied)  
* response hash (+ optional response payload if policy allows)  
* capability identity  
* optional timestamp only if time capability is granted (and then the timestamp becomes part of the log)

Logs must be stable: same program \+ same log \=\> same replay result.

### **6.4 Replay checker**

Implement `replay(program, log)` that:

* steps program  
* when `Perform` encountered, consumes next log entry  
* checks request hash matches  
* returns recorded response  
* errors loudly on mismatch (sealed ERROR or Rust error mapped to ERROR)

---

## **7\) Obligations engine (packages that justify themselves)**

### **7.1 Package format (recommend TOML)**

`package.toml`:

* name/version  
* dependencies pinned by content hash  
* modules list  
* obligations list  
* evidence pointers (hashes)

### **7.2 Evidence store**

A content-addressed store:

* `store/<hash>` containing JSON/TOML blobs, logs, reports.

### **7.3 Baseline obligations (v0.2)**

Implement these first:

* `unit_tests_pass`  
* `determinism_claim` (module must not produce sealed EFFECT)  
* `capabilities_declared` (declared caps must cover observed effect ops)  
* `replayable_tests` (test run must record effect logs and replay must pass)

Later obligations:

* property tests with recorded seeds  
* coverage thresholds  
* resource budgets (bench harness)  
* translation validation certificates

---

## **8\) Semantic patches (AI-safe change representation)**

### **8.1 Patch artifact schema**

Represent patches structurally:

* replace node at path  
* add module  
* remove module  
* update dependency  
* update obligation set  
* add evidence artifact pointers

Paths should be defined over canonical CoreForm AST.

### **8.2 Patch acceptance pipeline**

1. Validate patch schema  
2. Apply patch to canonical AST(s)  
3. Validate structural invariants (no forbidden ops/forms)  
4. Re-run required obligations (or incremental subset)  
5. Record provenance \+ evidence artifacts

---

## **9\) CLI spec (minimum commands)**

Implement `genesis` CLI with:

* `genesis fmt <file>` — canonical formatting  
* `genesis eval <file>` — evaluate pure CoreForm  
* `genesis test` — run obligations/tests, produce evidence artifacts  
* `genesis run <file> --caps <policy>` — run with capability runner, emit effect log  
* `genesis replay <file> --log <logfile>` — replay deterministic run  
* `genesis pack` — build package artifact \+ manifest \+ hashes  
* `genesis apply-patch <patchfile>` — apply semantic patch \+ rerun obligations  
* `genesis explain <expr>` — or `genesis explain <file> --msg ...` for dispatch traces

---

## **10\) Quality plan (how to keep it “v0.2 serious”)**

### **10.1 Test pyramid**

* **Golden tests**: CoreForm programs \+ expected outputs  
* **Protocol tests**: spoof attempts against UNHANDLED/EFFECT/ERROR should fail (sealed checks)  
* **Determinism tests**: same input \=\> same output; effect logs replay identically  
* **Fuzzing**:  
  * parser roundtrip (parse \-\> print \-\> parse)  
  * evaluator should never panic  
* **Metamorphic tests**:  
  * alpha-renaming invariance  
  * canonical formatting invariance

### **10.2 Explicit “no panic” policy**

In Rust:

* avoid `unwrap()` in code paths reachable from user input  
* map internal errors to sealed ERROR values at boundaries

### **10.3 Spec lock-in**

Add `docs/spec/` that restates the *normative* rules (dispatch, seals, replay). Keep it small and enforced by tests.

### **10.4 Runtime profiling and workload ratchets**

Use the workload gate before and after evaluator performance changes:

```bash
bash scripts/check_runtime_workload_budgets.sh
GENESIS_RUNTIME_WORKLOAD_PROFILE=roadmap \
  GENESIS_RUNTIME_WORKLOAD_REQUIRE_ROADMAP_SIZES=1 \
  bash scripts/check_runtime_workload_budgets.sh
```

The default `smoke` workload keeps CI practical before R1/R2 optimizations. The `roadmap`
profile is the full target workload: `fib(25)`, 1M `vec/push`, 100k `map/put`, 10k
`str/concat`, 100k dispatches through a 5-deep contract chain, and selfhost parsing of
`selfhost/parse.gc` plus `prelude/prelude.gc`.

Profile `genesis eval` on the fib workload:

```bash
cat > /tmp/genesis_fib25.gc <<'GC'
(def bench/fib
  (fn (n)
    (if ((core/int::lt? n) 2)
      n
      ((core/int::add (bench/fib ((core/int::sub n) 1)))
        (bench/fib ((core/int::sub n) 2))))))
(bench/fib 25)
GC

cargo build --profile selfhost-strict -p gc_cli
cargo flamegraph --profile selfhost-strict -p gc_cli --bin genesis -- eval /tmp/genesis_fib25.gc
samply record -- ./target/selfhost-strict/genesis eval /tmp/genesis_fib25.gc
```

Perf PR rule: if a Tier change improves a workload metric, tighten the corresponding
`GENESIS_BUDGET_WORKLOAD_*` default or policy seed in the same PR to `new p95 * 1.25`
rounded up to a stable integer. Include the before/after workload report paths and the
dominant flamegraph stacks in the PR notes.

---

## **11\) How to run this project best in Codex Desktop (threads \+ worktrees \+ skills)**

Codex Desktop is explicitly built to run multiple tasks in parallel with worktrees and built-in Git tooling. ([OpenAI Developers](https://developers.openai.com/codex/app/)) Use that structure from the start:

### **Suggested Codex thread/worktree breakdown**

Create separate threads (each gets its own worktree):

1. **Kernel thread**: CoreForm \+ evaluator \+ seals  
2. **Prelude thread**: contracts \+ dispatch/extend/explain \+ sealed protocol  
3. **Effects thread**: effect IR \+ runner \+ logs \+ replay  
4. **Obligations thread**: package \+ evidence store \+ test runner  
5. **Patches thread**: patch schema \+ apply \+ validator  
6. **CLI thread**: command wiring \+ UX \+ docs

Codex app explicitly supports worktrees for parallel tasks and built-in Git review/commit workflows. ([OpenAI Developers](https://developers.openai.com/codex/app/))

---

# **12\) Codex project instruction files (drop-in templates)**

Codex reads `AGENTS.md` before doing work, and supports layered overrides via `AGENTS.override.md` discovered from repo root down to current directory. It concatenates them in order, with closer directories overriding earlier guidance; there’s also a default size cap (32 KiB) for combined instructions. ([OpenAI Developers](https://developers.openai.com/codex/guides/agents-md/))

## **12.1 `AGENTS.md` (repo root) — recommended content**

Create `AGENTS.md` at repo root:

\# GenesisCode v0.2 — Codex Working Agreement

\#\# Mission  
Implement GenesisCode v0.2 exactly per docs/PAPER\_v0.2.md and docs/TECH\_HANDOFF.md.

\#\# Non-negotiable invariants  
\- Kernel (Gλ) must be pure and deterministic. No filesystem/time/network/LLM inside the evaluator.  
\- UNHANDLED/EFFECT/ERROR must be unforgeable using seal tokens created by Prelude.  
\- Never panic on user input. Convert errors to sealed ERROR values at boundaries.  
\- Effect runner must be deny-by-default and must produce deterministic effect logs \+ replay checker.

\#\# Repo conventions  
\- Language: Rust (workspace in crates/).  
\- Always run: cargo fmt, cargo clippy, cargo test before committing.  
\- Keep TCB-A minimal: evaluator \+ immutable primitives \+ seals only.

\#\# Development flow  
\- Make small commits with passing tests.  
\- Add golden tests for every semantic rule you implement.  
\- Prefer implementing behavior in libraries over adding kernel special forms.  
\- When uncertain, write a failing test first and make it pass.

\#\# Deliverables checklist  
\- CoreForm parser/printer  
\- Gλ evaluator \+ seals  
\- Prelude: contracts \+ dispatch/extend/explain \+ sealed protocol  
\- Effects: effect IR \+ runner \+ logs \+ replay  
\- Obligations engine \+ package format \+ evidence store  
\- Semantic patches \+ validator  
\- CLI that ties it all together

## **12.2 Optional: per-subdir overrides**

If you want a stricter rule set for, say, `crates/gc_kernel/`, add:

`crates/gc_kernel/AGENTS.override.md`

\# Kernel rules (override)  
\- Do not add new effects or IO primitives.  
\- Do not add new kernel special forms without updating docs/spec and adding golden tests.  
\- Avoid any dependency that is not essential for correctness.

Codex will automatically prefer `AGENTS.override.md` over `AGENTS.md` in a directory and will layer from the git root down to the working directory. ([OpenAI Developers](https://developers.openai.com/codex/guides/agents-md/))

---

# **13\) Codex Skills (optional but HIGH leverage)**

Codex skills are a supported way to package instructions/resources/scripts so the agent can reliably follow workflows; skills use progressive disclosure and are discovered in `.agents/skills/` directories (scanned upward toward repo root). ([OpenAI Developers](https://developers.openai.com/codex/skills/))

## **13.1 Create a “GenesisCode Spec-First” skill**

Create:

`.agents/skills/genesiscode-spec-first/SKILL.md`

\---  
name: genesiscode-spec-first  
description: \>  
  Use this skill when implementing or modifying any GenesisCode semantics or tooling.  
  It enforces spec-first development: write/extend golden tests, implement minimal code,  
  avoid expanding the kernel, and keep protocol markers unforgeable with seals.  
\---

\#\# Rules  
1\) Before implementing a feature, add or update:  
   \- docs/spec/ (normative rule)  
   \- tests/spec/ (golden tests)  
2\) Implement with smallest TCB-A changes possible.  
3\) Add negative tests for spoofing UNHANDLED/EFFECT/ERROR.  
4\) Ensure no panics on user input. Convert to sealed ERROR.  
5\) For effectful features:  
   \- add effect-log tests  
   \- add replay tests  
6\) Before merging:  
   \- run cargo fmt  
   \- run cargo clippy  
   \- run cargo test

\#\# Output expectations  
\- Keep PRs small and reviewable.  
\- Include a short "Semantics changed" note when behavior changes.

You can explicitly invoke a skill in Codex (e.g., with `$` selection) or let it be auto-chosen based on its description. ([OpenAI Developers](https://developers.openai.com/codex/skills/))

---

## **14\) Suggested “first prompt” to give GPT‑5.3‑Codex (copy/paste)**

When you start the first Codex Desktop thread, paste something like:

You are implementing GenesisCode v0.2 in this repo. Read `docs/PAPER_v0.2.md` and `docs/TECH_HANDOFF.md`. Follow `AGENTS.md`. Start with Milestone A only: CoreForm parser/printer \+ Gλ evaluator \+ seals \+ Prelude contracts (hardened UNHANDLED/EFFECT/ERROR \+ dispatch/extend/explain).  
Constraints: keep TCB-A minimal; no IO/time/network in kernel; no panics; every semantic rule requires golden tests; protocol tags must be unforgeable via seals.  
Deliver: working CLI commands `genesis fmt`, `genesis eval`, and a green `cargo test` suite for Milestone A.

GPT‑5.3‑Codex is explicitly described as being able to handle long-running tasks with tool use and complex execution, and the Codex app is designed for supervising parallel agent work via threads/worktrees. ([OpenAI](https://openai.com/index/introducing-gpt-5-3-codex/))

---

If you want, I can also generate **a complete `docs/spec/` starter** (normative rules in one place) plus an initial **`tests/spec/` golden suite** (seal spoof tests, dispatch chain tests, determinism tests) so the Codex agent starts with a concrete acceptance harness on day 1\.
