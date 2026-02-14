
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

