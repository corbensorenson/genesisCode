# Tab 1

# **GenesisCode v0.2**

## **A Tiny Pure Calculus \+ Obligations \+ Provenance for Auditable, AI-Symbiotic Programming**

**Draft Paper / Technical Specification (v0.2)**

---

## **Abstract**

GenesisCode v0.2 is an AI-native programming system built around a sharply minimized **pure kernel** and an aggressively modern **verification-and-provenance envelope** that governs how programs evolve—whether written by humans or proposed by large language models (LLMs). The kernel is a small functional calculus (**Gλ**) with immutable data, lexical closures, and a single universal abstraction—**contracts**—that implement message-based behavior and delegation through an explicit, inspectable protocol. Unlike “minimalist manifesto languages,” GenesisCode v0.2 makes its core claims mechanically enforceable: it standardizes **unforgeable seals** to harden protocol invariants; it pushes all nondeterminism (I/O, time, randomness, network, LLM calls) behind a capability boundary with **deterministic effect logs** and replay semantics; and it introduces an **Obligation Engine** that requires packages and extensions to carry evidence artifacts (tests, property checks, proofs, resource budgets, determinism claims, capability policies, compiler equivalence checks) before they can be accepted by tooling or registries.

GenesisCode is “AI-native” in a strict sense: there is one canonical intermediate representation (CoreForm) that fits in small context windows; structural validators reject malformed or policy-violating code before execution; semantic patch artifacts replace raw text edits; and AI becomes a proposal generator whose outputs are accepted only when obligations are satisfied and provenance is recorded. The result is a language system that feels near-magical not because it embeds AI into semantics, but because it **constrains, verifies, and explains program evolution** with a small trusted computing base (TCB), modern compilation techniques (shapes, inline caches, e-graphs), and optional advanced stacks (row-polymorphic contract types with effect rows, refinement proofs, translation validation). This paper specifies Gλ, the hardened contract protocol, the effect-log execution model, the obligation/provenance architecture, the type and proof stacks, the optimization strategy, and a realistic implementation roadmap.

---

## **1\. Thesis and Contributions**

### **1.1 Thesis**

Most “AI-native language” proposals confuse *AI integration* with *AI reliability*. GenesisCode v0.2 is built on the opposite premise:

AI output is untrusted by default. Reliability comes from a tiny deterministic core, explicit effect boundaries, hardened protocols, and mandatory evidence.

### **1.2 Contributions (what’s actually new/serious)**

GenesisCode v0.2 combines several ideas into one coherent, enforceable system:

1. **Gλ (Genesis Calculus):** a small pure core with canonical AST and a clearly bounded TCB.  
2. **Contracts with hardened dispatch:** message-based behavior \+ delegation \+ extension, with **unforgeable seals** so protocol results cannot be spoofed.  
3. **Deterministic effect logs:** effects are expressed as sealed requests; execution produces a replayable trace artifact, making AI and I/O auditable.  
4. **Obligation Engine:** packages must carry evidence artifacts (tests/proofs/bounds/policies/equivalence checks) and can be rejected mechanically.  
5. **Semantic patching:** code evolution is captured as verified, structural “patch artifacts,” not ad hoc text diffs—ideal for AI-driven changes.  
6. **Optional advanced stacks:** row-polymorphic contract typing \+ effect rows; refinement proofs; e-graph optimization; translation validation that keeps compilers outside the TCB.

If you want expert CS people to react with “this is dangerous (in a good way),” the obligation/provenance \+ replay model is the centerpiece.

---

## **2\. Design Goals and Non-Goals**

### **2.1 Goals**

* **Small Trusted Computing Base (TCB):** trust as little as possible.  
* **Determinism by default:** pure evaluation is deterministic; nondeterminism must pass through explicit capabilities.  
* **Hard protocol invariants:** dispatch, unhandled, error, effect request shapes are non-spoofable.  
* **Evidence-carrying software:** code must ship with obligations and artifacts that justify trust.  
* **AI as proposer, not authority:** accept changes only when obligations pass and provenance is recorded.  
* **Performance path without compromising trust:** modern optimization with validation.

### **2.2 Non-Goals (v0.2)**

* Not a “one true syntax” language (stacks can vary).  
* Not “prove everything” from day one (proof stacks are optional; tests are baseline obligations).  
* Not “compiler-first” (interpreter \+ validation first; compilation later with translation validation).

---

## **3\. TCB Model: What Must Be Trusted?**

GenesisCode splits the world into layers:

**TCB-A (Minimal):**

1. **Kernel evaluator for Gλ** (pure)  
2. **Immutable primitive operations** (pure)  
3. **Seal primitive** (unforgeability guarantee)

If you trust only TCB-A, you get deterministic evaluation of pure programs and robust protocol tokens.

**TCB-B (Operational):**  
4\) **Capability runner** (effect interpreter)  
5\) **Effect-log replay checker** (can be small)  
6\) **Package verifier** (hash/signature/policy)

**Non-TCB (should be validated):**  
7\) Optimizing compiler, JIT, e-graph rewriter, WASM backend  
These can be made non-TCB by requiring **translation validation evidence** as obligations.

This explicit TCB story is what experts look for.

---

## **4\. Core Representation: CoreForm as Canonical IR**

GenesisCode standardizes exactly one canonical representation:

### **4.1 CoreForm**

CoreForm is a canonical S-expression AST (not just syntax). All surface syntaxes and DSLs **must desugar** to CoreForm.

CoreForm nodes are built from:

* Symbols  
* Pairs (lists)  
* Vectors/Maps (optional but recommended)  
* Literals (nil, bool, int, bytes/string)  
* Sealed values (see §6)

### **4.2 Why canonical IR matters for AI**

* LLMs target **one form**, not many.  
* Structural validators can be simple and strict.  
* Semantic patches can operate on stable AST shapes.  
* Provenance hashes are stable and reproducible.

---

## **5\. Gλ: The Genesis Calculus (Kernel Semantics)**

Gλ is the smallest normative semantic core. Everything else is desugaring or library.

### **5.1 Syntax (conceptual)**

Terms `e`:

* `x` (variable)  
* `λx. e` (single-argument lambda)  
* `e1 e2` (application)  
* `quote(v)` (quote datum)  
* `prim(op, v1, …, vn)` (total primitive ops on immutable data)  
* `seal()` (fresh unforgeable token)  
* `seal(v, s)` (seal datum `v` under token `s`)  
* `unseal(w, s)` (attempt to unseal `w` with token `s`)

Note: `if`, `let`, multi-arg functions, modules, etc. are **desugaring**, not kernel.

### **5.2 Values**

* Immutable data: nil, bool, int, bytes/string, symbol, pair, vector/map  
* Closures: `<λx.e, env>`  
* Sealed values: `⟦v⟧s` (sealed datum v under seal token s)  
* Seal tokens `s`: unforgeable, fresh, and only comparable by identity (`eq?`)

### **5.3 Seal guarantee (the key hardening primitive)**

* Only `seal()` produces a fresh token.  
* The only way to access the payload of `⟦v⟧s` is `unseal(⟦v⟧s, s)`.  
* Any mismatch returns a benign failure value (e.g., `nil` or a sealed error), never the payload.

This single primitive eliminates spoofing of “unhandled/effect/error” tags.

### **5.4 Evaluation (sketch)**

Standard call-by-value evaluation with lexical scoping. Primitive ops are total (or return explicit error values rather than throwing host exceptions).

---

## **6\. Contracts: Universal Abstraction with Hardened Protocol**

A **contract** is a closure that processes a single **message** datum. Contracts are ordinary values in Gλ/CoreForm.

### **6.1 Standard message shape**

A message is a datum of the shape:

* `(msg op payload)`  
  where `op` is a **qualified symbol** and `payload` is any datum.

**Qualified symbols** prevent collision:

* `pkg/module::name` (or internal `(ns name)` representation)

### **6.2 Non-forgeable protocol results via seals**

The Prelude defines three seals (created once at startup and stored in trusted Prelude bindings):

* `S_UNHANDLED`  
* `S_EFFECT`  
* `S_ERROR`

And standardized constructors:

* `UNHANDLED(msg) := seal((unhandled msg), S_UNHANDLED)`  
* `EFFECT(req) := seal((effect req), S_EFFECT)`  
* `ERROR(info) := seal((error info), S_ERROR)`

Only the Prelude (or code given those seals explicitly) can construct values recognized as unhandled/effect/error by the dispatcher/runner. User code can still *represent* similar structures, but cannot spoof the sealed form.

This is the “minimal but serious” upgrade that makes experts stop calling the protocol a convention.

### **6.3 Delegation: explicit and inspectable**

Each contract has:

* `handler : msg -> value`  
* `proto : contract | nil`  
* `meta : map`  
* optionally a `shape-id` (see optimization)

**Normative dispatch algorithm:**

dispatch(c, msg):  
  r \= c.handler(msg)  
  if is\_sealed\_unhandled(r) and c.proto \!= nil:  
       return dispatch(c.proto, msg)  
  else:  
       return r

`is_sealed_unhandled` is defined by attempting `unseal(r, S_UNHANDLED)`.

### **6.4 Immutable extension**

`extend(base, overrides, meta+) -> newContract`:

* new handler checks `overrides` by `op`  
* if no match: returns `UNHANDLED(msg)`  
* `proto = base`  
* `meta = merge(base.meta, meta+)`

No mutation. No secret inheritance rules.

### **6.5 Introspection and explainability**

Standard ops:

* `meta` → returns meta map  
* `proto` → returns proto pointer or nil  
* `shape` → returns shape-id (if present)  
* `explain` → returns a pure trace object showing resolution path:  
  * which contracts were consulted  
  * which override matched (if any)  
  * final result (sealed/unsealed)

This is a core AI-native debugging tool.

---

## **7\. Effects: Capabilities \+ Deterministic Effect Logs \+ Replay**

Effects are not “values you return and hope a runner interprets.” v0.2 standardizes an **effect program representation** and a **trace artifact**.

### **7.1 Effect program IR (standardized)**

Represent effectful computations as a free structure:

* `Pure(v)`  
* `Perform(op, payload, k)` where `k` is a continuation `result -> effectProgram`

In CoreForm this is typically a tagged structure; the important part is standardization.

### **7.2 Effect requests are sealed**

An actual “please perform this effect” boundary value is:

* `EFFECT( request )`  
  where `request` contains:  
* `op` (qualified symbol)  
* `payload` (datum)  
* `continuation` (in effect-IR form)

This prevents spoofing “effect” values that might trick runners into privilege escalation.

### **7.3 Capabilities**

A capability is an authority object provided by the host runner, not by pure code:

* filesystem, network, time, randomness, LLM, etc.

Programs cannot access capabilities unless the runner provides them.

### **7.4 Deterministic effect logs**

Every effect interpretation produces an **effect log** artifact:

Each entry includes:

* request hash (op \+ payload hash \+ continuation hash)  
* decision: allowed/denied  
* response hash (and optionally full response data depending on policy)  
* capability identity (which authority responded)  
* optional timestamp only if time capability is permitted

### **7.5 Replay semantics**

Given:

* a program `P` producing an effect program  
* an effect log `L`

`replay(P, L)` must:

* produce the same final value, or  
* fail with a structured replay mismatch error.

This is foundational for:

* auditability  
* deterministic CI  
* “AI-assisted builds you can reproduce”

### **7.6 LLM calls as effects (auditable AI)**

LLM generation is an effect op, e.g.:

* `ai/generate`  
* payload includes prompt, model identifier, parameters  
* response includes generated CoreForm \+ provenance metadata

Crucially:

* outputs are logged  
* replay can pin generation outputs  
* registries can require that AI outputs are attached as signed artifacts

This makes “AI-native” defensible to experts.

---

## **8\. The Obligation Engine: Evidence-Carrying Packages**

This is the v0.2 centerpiece.

### **8.1 Obligations**

An **obligation** is a declarative requirement plus verification procedure and evidence artifacts. Examples:

* **Unit tests** must pass.  
* **Property tests** must pass with recorded seeds.  
* **Coverage threshold** must be ≥ X (tooling obligation).  
* **No capability escalation**: declared capabilities must be a superset of actual used effect ops.  
* **Determinism claim**: pure modules must not emit EFFECT values.  
* **Resource budgets**: time/memory limits in benchmark harness.  
* **Typecheck**: the type stack must accept the module.  
* **Refinement proof**: discharge a predicate using SMT/interactive proof.  
* **Translation validation**: optimized/compiled output is equivalent under a spec.

Obligations are modular: stacks can introduce new obligation types.

### **8.2 Evidence artifacts**

Evidence is stored as immutable artifacts referenced by hash:

* test logs (including effect logs)  
* coverage reports  
* proof objects / SMT traces  
* benchmark results  
* equivalence certificates

### **8.3 Package acceptance and registry policy**

A registry (or local policy) can reject packages unless:

* obligations declared by the package are satisfied, and/or  
* registry-required obligations are satisfied (e.g., minimum tests \+ determinism \+ signed provenance)

This is how you prevent “AI wrote it, trust me.”

### **8.4 Reproducible builds and supply chain posture**

Packages are content-addressed:

* artifact hash includes code \+ obligations \+ evidence \+ dependency graph \+ toolchain ID  
* optional signatures \+ transparency log (append-only)

This aligns with modern supply chain expectations and makes the system feel “adult.”

---

## **9\. Semantic Patching: Verified Evolution Instead of Text Diffs**

LLMs are great at proposing edits. They are terrible at being trusted editors. v0.2 standardizes **semantic patches**.

### **9.1 Patch artifacts**

A patch is a typed, structural transformation over CoreForm, such as:

* “replace handler for op X in contract Y”  
* “add new override mapping”  
* “strengthen obligation set”  
* “refactor pure function f by rewrite rule R”

### **9.2 Patch validation pipeline**

A patch is accepted only if:

1. Structural validation passes (well-formed CoreForm, no forbidden forms)  
2. Dependency constraints hold  
3. Obligations re-check successfully (or remain valid under incremental checking)  
4. Provenance recorded (including AI proposal source if used)

### **9.3 Why this is AI-native**

Instead of asking AI to “edit the file,” you ask it to produce a patch artifact, and the system verifies the patch. This dramatically reduces hallucination damage.

---

## **10\. Type Stack: Row-Polymorphic Contract Types \+ Effect Rows**

Types are not in the kernel; they are an enforceable stack plus obligations.

### **10.1 Contract interface types**

A contract is typed by the operations it handles:

* `Contract { op1: A1 -> R1, op2: A2 -> R2 | r }`

where `r` is a **row variable** representing the rest of the interface (row polymorphism).

This matches extension perfectly:

* extending a contract adds fields to the row  
* delegation is typed as “proto provides row r”

### **10.2 Effect rows**

Functions can declare effect requirements:

* `A -> B ! {io, ai, time}`

and pure code is `! {}`.

### **10.3 Gradual typing (pragmatic)**

Allow:

* inferred types for common code  
* explicit types for public interfaces  
* gradual “unknown” (`?`) for early-stage code

### **10.4 Refinement layer (optional but “wow”)**

Add refinement types as an advanced stack:

* `Int{n >= 0}`  
* `Map{k:Key -> v:Val | preservesInvariant(...)}`

Proof obligations can be discharged by SMT or interactive proofs and recorded as evidence artifacts.

This is where “provably safe concurrent map” stops being a slogan.

---

## **11\. Optimization and Compilation Without Expanding the TCB**

GenesisCode can be fast without trusting a huge compiler.

### **11.1 Shapes and inline caches for dispatch**

Each contract can have a stable `shape-id` derived from:

* override table structure  
* proto’s shape-id

Dispatch can be accelerated by:

* polymorphic inline caches (PICs) keyed by `(shape-id, op)`  
* compiled jump tables for frequently used shapes

### **11.2 E-graph rewriting (pure-code superpower)**

Because the kernel is pure, aggressive equational optimization is viable:

* normalize CoreForm  
* build an e-graph  
* apply rewrite rules (beta, fusion, algebraic simplifications, deforestation where safe)  
* extract minimal-cost program

### **11.3 Translation validation as an obligation**

To keep the optimizer/compiler outside the TCB, require evidence:

* run equivalence checks on a test suite (baseline)  
* optionally use symbolic testing or proof-carrying rewrite traces (advanced)

The key is the architecture: optimization is powerful, but *not trusted by default*.

### **11.4 WASM and portability (implementation direction)**

The runtime can target WASM for portability:

* kernel stays pure  
* capability runner bridges to host APIs  
* effect logs remain the audit trail across platforms

No magic claims needed—just a clean layering story.

---

## **12\. “Genesis” and Self-Reference: Optional, Derivable, Not Required**

v0.2 avoids relying on a mystical fixed-point kernel definition. The kernel is Gλ; “Genesis” is a standard library root contract with meta/proto behavior.

However, a self-referential “Genesis fixed point” can be **derived in a stack** as a demonstration of expressiveness:

* fixed-point combinators exist in pure Gλ  
* self-hosting can be built as obligations mature  
* but the language doesn’t bet its implementability on a clever tautology

This is how you keep beauty without sacrificing engineering credibility.

---

## **13\. Example: A Verified Immutable Variable Contract**

**Intent:** an immutable variable with `get` and `set`, where `set` returns a new variable and does not mutate the original.

* `v0 = makeVar(42)`  
* `dispatch(v0, (msg get nil)) => 42`  
* `v1 = dispatch(v0, (msg set 100))`  
* `dispatch(v1, (msg get nil)) => 100`  
* `dispatch(v0, (msg get nil)) => 42`

Obligations:

* unit tests above  
* determinism: must not emit EFFECT  
* typecheck: `Contract { get: Unit -> Int, set: Int -> Contract{...} }`

Explainability:

* `explain(v1, msg get nil)` returns the exact resolution path and override source.

This example is simple, but it exercises the hardened protocol and obligation framing.

---

## **14\. Example: AI-Assisted Extension That Is Auditable and Reproducible**

Goal: “Generate a persistent ordered map implementation.”

Workflow:

1. User writes tests \+ property tests \+ performance budget obligation.  
2. AI proposes a semantic patch adding module `ordered-map`.  
3. Tooling validates structure, runs tests, records effect logs for:  
   * AI generation step (ai/generate)  
   * any benchmark harness effects (time capability)  
4. Package is accepted only if obligations pass; AI output provenance is attached.

If the AI used nondeterministic sampling, replay pins the exact output by replaying the effect log (or by caching the generation response artifact).

This is the real “AI-native magic”: not generation, but *verified evolution with replay.*

---

## **15\. Implementation Roadmap (Realistic and High-Impact)**

### **Phase 1: Minimal kernel \+ seals \+ CoreForm**

* CoreForm parser/printer  
* Gλ evaluator  
* immutable data  
* seal/unseal primitives  
* Prelude: contracts, dispatch, extend, explain

### **Phase 2: Effect IR \+ capability runner \+ effect logs \+ replay**

* standardized effect program representation  
* runner with capability injection  
* effect log format \+ replay checker  
* baseline policies (deny by default)

### **Phase 3: Obligation engine \+ package format \+ registry rules**

* package manifest with obligations  
* evidence artifact store (content-addressed)  
* CI-grade runner that enforces obligations

### **Phase 4: Semantic patches \+ structural validators**

* patch artifact schema  
* patch validator  
* incremental obligation checking

### **Phase 5: Type stack (row polymorphism \+ effect rows)**

* checker as obligation  
* optional gradual typing

### **Phase 6: Optimization (shapes/PICs/e-graphs) \+ translation validation**

* interpreter instrumentation for shapes  
* optional compiler backend  
* equivalence evidence as obligations

This roadmap emphasizes what makes the project *beyond SOTA*: trust architecture \+ verification envelope, not premature compiler heroics.

---

## **16\. Conclusion**

GenesisCode v0.2 is designed to be impressive to experts for the right reasons:

* The kernel is small and precisely bounded (Gλ \+ seals).  
* Protocol invariants are hardened (unforgeable UNHANDLED/EFFECT/ERROR).  
* Effects are explicit, capability-gated, and auditable (deterministic effect logs \+ replay).  
* Software must justify itself (obligations \+ evidence artifacts).  
* AI is integrated as a proposer with recorded provenance, not as a magical oracle.  
* Performance is attainable with modern techniques while keeping optimizers outside the TCB (shapes, PICs, e-graphs, translation validation).  
* Multiple paradigms are supported without core bloat (stacks), including serious typing and proofs.

If v0.1 was “a minimalist language that’s friendly to LLMs,” v0.2 is a stronger claim:

**A minimal deterministic calculus plus an evidence-and-provenance system that makes AI-assisted programming auditable, reproducible, and policy-constrained—without trusting the AI, the network, or the compiler.**

That is the “near magical” effect experts respect.

---

## **Appendix A: v0.2 Conformance Checklist**

A conforming GenesisCode v0.2 system provides:

**Kernel (TCB-A):**

1. CoreForm canonical representation  
2. Gλ evaluator (λ, application, quote, primitives)  
3. Total immutable primitives (pair/symbol/int/bytes/etc.)  
4. `seal()` and `unseal()` with unforgeability guarantee

**Prelude:**  
5\. Contract constructor (handler/proto/meta)  
6\. Hardened UNHANDLED/EFFECT/ERROR via seals  
7\. Normative `dispatch`, `extend`, `explain`

**Effects \+ Auditability:**  
8\. Standard effect IR (Pure/Perform) or equivalent  
9\. Capability runner (deny-by-default)  
10\. Effect log generation \+ replay checker

**Obligations \+ Supply Chain:**  
11\. Package format with obligations \+ evidence artifacts  
12\. Content-addressed hashing and dependency pinning  
13\. Policy hooks for registry acceptance

**AI-Native Evolution:**  
14\. Structural validators for CoreForm \+ patches  
15\. Semantic patch artifact format \+ validation pipeline

**Optional stacks (non-required but v0.2 vision-complete):**  
16\. Row-polymorphic contract typing \+ effect rows  
17\. Refinement proof stack  
18\. Optimization via shapes/PICs/e-graphs \+ translation validation as obligations

---

If you want, I can also produce **(1)** a formal-ish small-step semantics appendix for Gλ \+ seals, **(2)** a concrete CoreForm schema for effect logs \+ patch artifacts, and **(3)** a minimal “GenesisCode Registry Policy” spec that reads like something serious organizations would actually adopt.

# Tab 2

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

# Tab 3

# **GenesisCode Style Guide v0.2**

## **Best Practices, Syntax Conventions, and “How It Should Look \+ Behave” Before the Language Exists**

This document is the **writing bible** for GenesisCode. It defines how code should be shaped, named, structured, documented, tested, and packaged so that:

* programs are **pure by default** and **auditable** when effectful  
* contracts are **predictable** and **introspectable**  
* extensions are **verifiable** (obligations \+ evidence)  
* diffs and semantic patches remain stable  
* LLMs can generate and refactor code **reliably**

Where something is not yet implemented, treat this as the target behavior and stable convention.

---

## **1\) Core philosophy**

### **1.1 Canonical form wins**

GenesisCode has one canonical IR: **CoreForm S-expressions**. Any “sweet” syntax must desugar to CoreForm.

**Rule:** Published libraries and reference implementations should be written (or emitted) in canonical CoreForm formatting even if authored with sugar.

### **1.2 Pure core, explicit effects**

The kernel is pure and deterministic. Anything involving I/O, time, randomness, network, or LLM calls must be expressed as an **effect program** and interpreted by a capability runner.

**Rule:** Pure functions never “accidentally” trigger effects.

### **1.3 Protocols are sealed, not conventional**

UNHANDLED/EFFECT/ERROR are **sealed** so user code cannot spoof them.

**Rule:** You do not pattern-match on raw `(unhandled ...)`. You use the standard predicates/unsealers.

### **1.4 Evidence-carrying software**

Packages must ship with obligations \+ evidence (tests, property checks, replay logs, proofs, budgets).

**Rule:** If it’s not backed by obligations, it’s not “real code” yet.

---

## **2\) Formatting and whitespace (canonical style)**

### **2.1 Indentation**

* **2 spaces**, no tabs.  
* Closing parens belong at the end of the last line of the form.  
* One “major form” per line at top-level (`def`, `module`, etc.).

**Good**

(def add  
  (fn (x)  
    (fn (y)  
      (prim int/add x y))))

**Bad**

(def add (fn (x) (fn (y) (prim int/add x y))))

### **2.2 Line length**

* Hard max: **100 columns**  
* Prefer breaking long maps/vectors and long call forms onto multiple lines.

### **2.3 Multi-argument application formatting**

Since evaluation is unary/curry-based, you’ll often see nested calls.

**Preferred for readability**: keep application visually grouped:

((f x) y)

If your formatter prints `(f x y)` as sugar, it must desugar canonically to `((f x) y)`.

### **2.4 Literal data formatting**

* Lists: `'(a b c)` is allowed sugar for `(quote (a b c))`  
* Maps/vectors should be printed with stable ordering and stable key formatting.

---

## **3\) Names, namespaces, and symbols**

### **3.1 Qualified operation symbols (mandatory)**

Operations are always **qualified symbols** to avoid collisions:

**Format**

pkg/module::op-name

Examples:

* `core/contract::meta`  
* `core/contract::proto`  
* `core/contract::dispatch`  
* `io/fs::read`  
* `ai/llm::generate`

**Rule:** Never use bare ops like `get`, `set`, `read` in published code.

### **3.2 Identifier casing**

* Functions, values, ops: **kebab-case**  
* Modules: **kebab-case** path segments (`my-lib/data`, not `MyLib/Data`)  
* Constants (rare): kebab-case is still preferred; if you need emphasis, use `*stars*`:  
  * `*s-unhandled*` (seal token)  
  * `*genesis*` (root contract)

### **3.3 Keyword-like symbols**

For map keys, use colon-prefixed symbols:

* `:intent`  
* `:doc`  
* `:examples`  
* `:caps`  
* `:obligations`  
* `:tests`

These are still just symbols, but the colon convention signals “data key.”

---

## **4\) File and module conventions**

### **4.1 “One module per file” (recommended)**

A file should define a module’s public surface and private helpers.

**Module header convention**

(def core/module::meta  
  {:module "my-lib/data"  
   :intent "Persistent data structures"  
   :doc "…"  
   :exports \[my-lib/data::map  
             my-lib/data::filter\]  
   :caps \[\] ; pure module \=\> empty  
   :obligations \[core/obligation::unit-tests  
                 core/obligation::determinism\]})

### **4.2 Exports list**

Exports are always explicit and qualified. Avoid “export everything” patterns; they destroy auditability.

---

## **5\) Contracts: message protocol and writing style**

GenesisCode’s universal abstraction is the **contract**. Contracts are invoked with a **message** datum:

(msg op payload)

### **5.1 Message payload shape**

Prefer payloads as **maps with keyword keys**, not positional tuples, for clarity and AI reliability.

**Good**

(msg my-lib/counter::add {:delta 1})

**Bad**

(msg my-lib/counter::add 1\)

Exceptions:

* Ultra-hot primitives where performance matters (stack may allow positional payloads)  
* Tiny internal helpers in private modules

### **5.2 Contract handler signature**

The canonical handler signature is:

* `handler : msg -> value`

Write handlers so that:

* they parse `op` and `payload` explicitly  
* they **return sealed UNHANDLED(msg)** if they do not handle an op  
* they never “silently” return nil for unknown ops

**Template**

(def my-lib/counter::make  
  (fn (n0)  
    (core/contract::extend  
      core/contract::genesis  
      {my-lib/counter::get  
        (fn (msg) n0)

       my-lib/counter::add  
        (fn (msg)  
          (let ((p (core/msg::payload msg))  
                (d (core/map::get p :delta)))  
            (my-lib/counter::make  
              (prim int/add n0 d))))}  
      {:intent "Immutable counter"  
       :caps \[\]  
       :tests \[my-lib/counter::tests\]})))

Note: `core/msg::payload`, `core/map::get`, etc. are standard-library placeholders in this style guide. The early implementation should provide these helpers quickly because they dramatically reduce errors and improve AI generation.

### **5.3 Delegation and override rules**

* A contract should only delegate via `core/contract::dispatch` (or equivalent standard function).  
* Handler should not “manually” walk proto chains unless it’s a tooling module.

### **5.4 Introspection ops (standard)**

Every meaningful contract should support:

* `core/contract::meta`  
* `core/contract::proto`

Even if your handler doesn’t implement them, `genesis` (as proto) should.

### **5.5 “Explainability-first”**

If a contract is part of public API, include an obligation/test asserting that:

* `core/contract::explain` returns a trace that includes the expected op resolution path.

This helps humans and LLMs debug.

---

## **6\) UNHANDLED, ERROR, and “don’t lie” rules**

### **6.1 Unhandled**

Unknown op handling must return **sealed UNHANDLED(msg)**.

**Rule:** Never return raw `(unhandled msg)` and never return nil for unhandled ops.

### **6.2 Errors**

Errors are values, not host crashes.

**Rule:** Prefer returning **sealed ERROR(info)** over ambiguous sentinel values.

Recommended error payload shape:

{:error/code "my-lib/not-found"  
 :error/message "Key not found"  
 :error/context {:key k  
                 :module "my-lib/data"}}

### **6.3 Never swallow errors**

If you call something that may return ERROR, you must either:

* propagate it unchanged, or  
* explicitly handle it with a documented policy

---

## **7\) Effects and capabilities: how to write effectful code**

### **7.1 Pure vs effectful functions**

A **pure function** returns a value.

An **effectful function** returns an **effect program** (not “random effects happening inside evaluation”).

### **7.2 Capability declarations (mandatory for effectful modules)**

Every effectful module must declare required capabilities in metadata:

{:caps \[io/fs::read io/fs::write sys/time::now\]}

**Rule:** If `:caps` is empty, obligations should enforce that the module never emits EFFECT values.

### **7.3 Effect program construction (standard constructors)**

The style guide recommends standard constructors (names may vary in implementation, but keep semantics):

* `core/effect::pure : v -> program`  
* `core/effect::perform : op -> payload -> (result -> program) -> program`  
* `core/effect::bind : program -> (v -> program) -> program` (optional convenience)

**Example: read file**

(def my-lib/io::read-all  
  (fn (path)  
    (core/effect::perform  
      io/fs::read  
      {:path path}  
      (fn (bytes)  
        (core/effect::pure bytes)))))

### **7.4 Deterministic effect logs (writing for replay)**

When you write effectful code:

* keep payloads small and stable  
* avoid embedding gigantic AST blobs in payloads unless necessary  
* prefer content hashes in payloads for large artifacts

Example:

(core/effect::perform  
  ai/llm::generate  
  {:prompt-hash prompt-h  
   :model "gpt-5.3-codex"  
   :params {:temperature 0.2}}  
  ...)

---

## **8\) Obligations and tests: required structure and naming**

### **8.1 Tests are first-class obligations**

For any published module, you should provide:

* unit tests  
* determinism/replay tests (as applicable)  
* protocol spoofing negative tests (for prelude/security code)

### **8.2 Naming conventions**

* Test names: `"module::behavior - scenario"`  
* Obligation identifiers are qualified symbols.

Example:

(def my-lib/counter::tests  
  \[(test "my-lib/counter::get \- initial"  
     (check-eq 0  
       (core/contract::dispatch  
         (my-lib/counter::make 0\)  
         (msg my-lib/counter::get nil))))

   (test "my-lib/counter::add \- returns new"  
     (let ((c0 (my-lib/counter::make 0))  
           (c1 (core/contract::dispatch  
                 c0 (msg my-lib/counter::add {:delta 1}))))  
       (begin  
         (check-eq 0 (core/contract::dispatch c0 (msg my-lib/counter::get nil)))  
         (check-eq 1 (core/contract::dispatch c1 (msg my-lib/counter::get nil))))))\])

### **8.3 Property tests (recommended once supported)**

Property tests must record seeds in evidence artifacts so reruns are deterministic.

### **8.4 “No proof without spec”**

If you add a proof/refinement obligation, the module must also include:

* a precise statement of the property in `:doc`  
* a minimal set of examples showing its meaning

---

## **9\) Semantic patch friendliness rules (for stable evolution)**

GenesisCode will use **structural patches**. Code should be written to minimize meaningless changes.

### **9.1 Stable ordering**

* Keep override maps sorted by op symbol.  
* Keep module metadata keys in a stable order: `:module, :intent, :doc, :exports, :caps, :obligations, :tests, :examples`.

### **9.2 One definition per top-level form**

Avoid huge “megadefs” that make patches noisy.

### **9.3 Avoid generated names**

Don’t generate symbol names dynamically for public API.

### **9.4 Prefer data-driven payloads**

When messages carry structured payload maps, patches can add keys without breaking callers.

---

## **10\) Documentation style (AI- and human-friendly)**

### **10.1 Always include `:intent`**

Every public function/contract must include an intent line in metadata (or in a doc form) that is short and literal.

**Good:** `"Persistent ordered map with immutable updates"`  
**Bad:** `"A magical container of destiny"`

### **10.2 `:doc` should include invariants**

Document:

* purity/effect behavior  
* time/space notes if important  
* error behavior (what ERROR codes can occur)  
* obligations guaranteed by evidence

### **10.3 Examples are executable**

Examples should be runnable snippets (or tests). Avoid pseudo-code in docs if it can become stale.

---

## **11\) Suggested “core standard library” naming (so we don’t drift)**

Even before implementation, reserve these namespaces:

* `core/msg::*` — message constructors/accessors  
  * `core/msg::op`  
  * `core/msg::payload`  
  * `core/msg::make` (construct `(msg op payload)`)  
* `core/contract::*`  
  * `core/contract::genesis`  
  * `core/contract::dispatch`  
  * `core/contract::extend`  
  * `core/contract::explain`  
  * `core/contract::meta`  
  * `core/contract::proto`  
* `core/effect::*`  
  * `core/effect::pure`  
  * `core/effect::perform`  
  * `core/effect::bind`  
* `core/obligation::*`  
  * `core/obligation::unit-tests`  
  * `core/obligation::determinism`  
  * `core/obligation::replayable-tests`  
  * `core/obligation::capabilities-declared`  
* `core/error::*`  
  * `core/error::is-error`  
  * `core/error::code`  
  * `core/error::message`

Keeping these stable pays huge dividends later.

---

## **12\) “AI-native authoring rules” (practical constraints for generation)**

If you want GPT‑style agents to produce consistently correct code:

### **12.1 Prefer explicit over clever**

* Don’t rely on implicit conventions or “magic” macros in core libs.  
* Keep local helper functions small and named.

### **12.2 Keep functions shallow**

Deeply nested lambdas are correct but hard to read. Use `let`\-style bindings (even if desugared) to name intermediate values.

### **12.3 Avoid ambiguous nil**

Nil should mean “empty list / nothing,” not “error,” not “unhandled.”

### **12.4 Always include obligations with new public features**

LLMs should be prompted: “Add tests \+ determinism obligations.”

### **12.5 Write for `explain`**

If behavior depends on delegation/overrides, make it traceable.

---

## **13\) Minimal “reference formatting spec” (for a future formatter)**

A canonical formatter should:

1. Use 2-space indentation.  
2. Place each `def` on its own block.  
3. Print maps with stable key order:  
   * keywords first (sorted)  
   * then symbols (sorted)  
4. Print vectors with one item per line if \> 3 items or if any item is a compound form.  
5. Normalize quote sugar (`'x`) optionally, but output must be stable.

---

## **14\) Example: Complete “public contract module” template**

(def my-lib/counter::meta  
  {:module "my-lib/counter"  
   :intent "Immutable counter contract"  
   :doc "Counter supports get and add. Pure, deterministic."  
   :exports \[my-lib/counter::make\]  
   :caps \[\]  
   :obligations \[core/obligation::unit-tests  
                 core/obligation::determinism\]  
   :tests \[my-lib/counter::tests\]})

(def my-lib/counter::make  
  (fn (n0)  
    (core/contract::extend  
      core/contract::genesis  
      {my-lib/counter::get  
        (fn (msg) n0)

       my-lib/counter::add  
        (fn (msg)  
          (let ((p (core/msg::payload msg))  
                (d (core/map::get p :delta)))  
            (my-lib/counter::make (prim int/add n0 d))))}  
      {:intent "Immutable counter instance"  
       :caps \[\]  
       :doc "Instance contract. get returns current count; add returns new instance."  
       :tests \[\]})))

(def my-lib/counter::tests  
  \[(test "my-lib/counter::get \- initial"  
     (check-eq 0  
       (core/contract::dispatch  
         (my-lib/counter::make 0\)  
         (msg my-lib/counter::get nil))))

   (test "my-lib/counter::add \- returns new"  
     (let ((c0 (my-lib/counter::make 0))  
           (c1 (core/contract::dispatch  
                 c0  
                 (msg my-lib/counter::add {:delta 1}))))  
       (begin  
         (check-eq 0 (core/contract::dispatch c0 (msg my-lib/counter::get nil)))  
         (check-eq 1 (core/contract::dispatch c1 (msg my-lib/counter::get nil))))))\])

This is the “look” you want from day one: predictable naming, explicit ops, explicit payload maps, obligations, tests, no ambiguity.

---

# **Appendix: Decisions we should lock early (so style stays stable)**

These are style-adjacent design choices that affect how code looks:

1. **String vs bytes**: Do we have both? If yes, standardize literal syntax.  
2. **Map literal syntax**: If CoreForm uses `{}` sugar, define canonical printing order.  
3. **Qualified symbol encoding**: Is `pkg/mod::name` a single symbol or structured `(ns name)`? (Either is fine; style assumes you can print as `pkg/mod::name`.)  
4. **Nil vs empty list**: treat nil as empty list; lists are pairs ending in nil.  
5. **Error unsealing behavior**: `unseal` mismatch returns nil vs ERROR—pick one and enforce via tests.

If you want, I can turn this into:

* a **short “GenesisCode Formatting Spec”** that a formatter can implement  
* a **lint rule set** (“no bare ops”, “must declare caps”, “must include :intent”, “must not return nil for unhandled”)  
* a **Codex prompt template** that tells the agent exactly how to emit code in this style every time

