
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

