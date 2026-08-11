# Contract Composition Profile v0.1

Status: normative for modules that opt into
`genesis/contract-composition-profile-v0.1` under `genesis/language-profile/v0.2`.

The closed machine contract is `docs/spec/CONTRACT_COMPOSITION_PROFILE_v0.1.json`, validated
against `docs/spec/CONTRACT_COMPOSITION_PROFILE_v0.1.schema.json`. This profile freezes static
interface composition, blame, shape and refinement identity, effect interaction, the supported
parametric fragment, and the necessary preconditions for optimization. It does not add a runtime
cast, a refinement prover, a new generic type system, or optimizer authority.

## Activation and closure

The profile is opt-in until the R4.2 self-hosted authority migration. If any supplied module
declares `:contract-composition-profile`, every module in that package closure must declare the
exact profile and the already frozen module-resolution profile:

~~~clojure
:module-profile genesis/module-resolution-profile-v0.1
:contract-composition-profile genesis/contract-composition-profile-v0.1
:strict-shapes true
:strict-effects true
:refinements {package/module::export [] ...}
~~~

The module-resolution profile's existing closed `:requires-profiles` map is not reinterpreted or
mutated. The composition marker is a separate exact opt-in and requires successful module
resolution. Every `:types` and `:refinements` key is exactly one declared export; missing or extra
keys fail closed. Export, type, and refinement maps are sets in canonical lexical order even when
their source representation is a vector or map.

Activation is all-or-nothing for a supplied package closure. Mixed profiled and unprofiled modules,
unknown profile IDs, false or malformed strict flags, unqualified symbols, duplicate entries, and
malformed metadata are boundary errors. The profile never activates by version inference.

## Static compatibility

An implementation type `I` satisfies a declared interface type `D` only when `I` is a structural
subtype of `D` after the declaration's supported rank-1 effect-row variables are instantiated.

- `?` is gradual top only in legacy v0.2 checking. It is forbidden anywhere in a profiled public
  interface, including nested payloads, fields, methods, parameters, and results.
- Scalars are invariant by constructor.
- Function parameters are contravariant: the implementation must accept every input promised by
  the interface. Function results and latent effect rows are covariant.
- Program results and effect rows are covariant.
- Record and contract rows use width subtyping. Every interface member must be present and
  compatible in the implementation.
- Because `:strict-shapes true` is mandatory, a declared closed record or contract row is exact:
  the implementation row is closed and has no extra members.
- Duplicate record fields, contract methods, and effect operations are errors; later entries never
  overwrite earlier declarations.
- A contract row's method key binds the method's message operation. The source type `(Msg T)` does
  not repeat that operation, but compatibility compares both method parameters as the keyed
  operation. Outside a keyed contract method, an unresolved message operation remains unknown.

Compatibility is checked after canonical parsing and module resolution and before execution.
Unknown inference cannot establish a concrete profiled interface. A failed typecheck means no
accepted implementation, regardless of whether the static declaration has a reproducible identity.

## Blame

Blame identifies which side failed its declared responsibility; it never grants recovery authority.

- **Provider blame** names `base-relative-module-path#qualified-export` when an implementation does
  not satisfy its declaration, a public declaration is gradual or malformed, a contract method is
  not a function, or an effect declaration is unsound.
- **Consumer blame** names the importing or call site when an argument, expected result, requested
  method, or effect context violates an already accepted interface.
- **Boundary blame** names this exact profile for mixed profiles, malformed metadata, duplicate
  ownership, unsupported refinements, identity drift, or any failure that prevents a provider and
  consumer contract from being established.

Errors are explicit, deterministic, attributed to portable module paths, and projected in lexical
order. A provider failure cannot be relabeled as consumer blame to preserve an artifact, and a
boundary failure cannot be assigned to either program party. Future structured span diagnostics
may add locations without changing these responsibility rules.

## Refinement identity

`:refinements` is explicit for every export so absence cannot be confused with an unchecked
predicate. Each value is a duplicate-free vector of qualified refinement IDs, canonicalized as a
lexical set. The refinement identity hashes the exact profile and canonical set.

Profile v0.1 supports only the empty set. Any non-empty set fails as an unsupported boundary
feature. A predicate name, model assertion, test result, or optimizer claim is not a proof and may
not refine a type. Introducing executable refinements requires a new profile with a total predicate
language, proof/receipt schema, termination and resource bounds, provider/consumer enforcement
points, independent verification, replay behavior, and migration rules. Existing v0.1 identities
remain immutable.

The explicit empty-set identity is still useful: it prevents later tooling from silently attaching
a predicate to an already accepted interface and lets caches distinguish refinement-free contracts
from future verified-refinement profiles.

## Static shape and interface identity

For each export, `gc_types::compose_contract_profile` parses the declared type and emits:

1. a canonical static type term;
2. a static shape identity hashing this profile and that term;
3. a refinement identity;
4. an interface identity binding profile, portable module path, qualified export, shape identity,
   and refinement identity; and
5. deterministic blame and optimization-precondition records.

Static type normalization sorts fields, methods, and effects. Rank-1 effect-row variable names are
alpha-normalized by first structural occurrence (`effect-row-0`, `effect-row-1`, ...), so renaming a
bound variable does not change shape identity. Record and contract open-tail marker names normalize
to `shape-open`; they are openness markers, not substitutable value-type variables. Changing a
constructor, member, operation, effect, open/closed status, refinement, path, export, or profile
changes the appropriate identity.

The package profile identity hashes the manifest-ordered module paths and their lexically ordered
interface identities. It exists only when the composition metadata is valid. It identifies the
declaration closure, not implementation acceptance; consumers must additionally require successful
module resolution and `TypecheckReport::ok` for the exact modules.

## Runtime contract identities

Static interface identities and runtime contract identities answer different questions and are
never interchangeable:

- runtime `shape_id` hashes the prototype runtime shape and ordered override operation keys; it is
  suitable for dispatch caches and changes when dispatch topology changes;
- runtime `contract_id` additionally binds the prototype contract identity, handler value, and
  merged metadata; it identifies one runtime contract value; and
- static shape/interface identities bind declared types, effects, refinements, module path, export,
  and profile, but not a runtime closure or metadata value.

An optimizer or cache must name which identity class it consumes. Equality in one class never
implies equality in another. Runtime contract operations remain opaque optimizer boundaries.

## Effects and parametricity

Every profiled module uses strict effects. Anonymous effect tails are forbidden. A closed declared
effect row accepts only a closed implementation row whose operations are a subset. Latent function
effects and program effects are observable interface facts and participate in static shape identity.
Adding, removing, or opening an effect row changes the interface and invalidates dependent evidence.

The only generic fragment in v0.1 is explicit rank-1 effect-row polymorphism already defined by
`TYPES.md`. A named effect-row variable must be bound by the outermost function parameter, is fresh
per application, captures one consistent remainder, and may flow to that function's result and
latent effect. Unknown arguments instantiate it as unknown rather than allowing a symbolic row to
escape. Higher-rank effects, implicit method polymorphism, value-type variables, generic
constructors, type classes, subtyping inference, and record/contract row substitution are
unsupported and fail rather than being guessed.

## Optimization preconditions

Each composed export receives seven independently inspectable predicates:

- `concrete`: no gradual type or anonymous row;
- `closedShapes`: every record and contract row is closed;
- `closedEffects`: every effect row is closed;
- `pure`: every closed effect row has an empty operation set;
- `refinementFree`: the explicit refinement set is empty;
- `contractFree`: no runtime `Contract` value occurs in the interface; and
- `monomorphic`: no effect-row variable or open shape marker occurs.

`eligible` is true only when all seven predicates are true. This is a necessary admission
precondition for rewrites that rely on the static contract; it is not proof of equivalence, a rewrite
request, or promotion authority. The optimizer must still remain inside `OPTIMIZER.md`, preserve
opaque seal/effect/contract/quote boundaries, obey resource limits, emit exact before/after
identities, and pass independent translation validation. An ineligible export may still be
optimized by a separately valid syntax-local rule that does not rely on this profile, but no tool
may fabricate missing preconditions or widen effects to obtain eligibility.

## Authority and migration

`gc_types::compose_contract_profile` and the v0.2 typechecker are the current independently
testable Rust stage0 realization. The profile does not switch production semantic ownership; R4.2
must move the same decisions into reviewed `.gc` implementations with differential and retirement
evidence. A profile update changes the prose, closed JSON/schema, source bindings, implementation,
positive and adversarial controls, generated agent references, and content identity together. An
existing profile ID is immutable.

## Nonclaims

- The profile does not provide runtime casts, dependent types, non-empty refinements, value-type
  generics, higher-rank polymorphism, implicit dictionaries, or nominal inheritance.
- A static declaration identity alone does not prove its implementation, package, or caller valid.
- Optimization eligibility does not replace translation validation or authorize a rewrite.
- Runtime dispatch `shape_id`, runtime `contract_id`, static shape identity, and interface identity
  are distinct domains.
- The profile does not switch self-host authority or promote a package, backend, benchmark, Foundry
  result, model, or release level.
