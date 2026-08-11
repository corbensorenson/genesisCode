# Numeric Profile v0.1

Status: normative for numeric behavior in `genesis/language-profile/v0.2`.

The closed machine contract is `docs/spec/NUMERIC_PROFILE_v0.1.json`, validated against
`docs/spec/NUMERIC_PROFILE_v0.1.schema.json`. Numeric behavior not listed in that contract is
unsupported. A backend, optimizer, host bridge, GPU kernel, or library cannot silently introduce a
second arithmetic model.

## Integers

`Int` is an arbitrary-precision signed integer. Source literals use `-?[0-9]+`; canonical CoreForm
printing uses normalized base-10 with no leading zeroes and represents zero as `0`. Runtime `i64`
storage is an unobservable fast path: overflow promotes to the arbitrary-precision representation
and never wraps, saturates, traps, or changes the value.

The v0.2 integer primitives are `int/add`, `int/sub`, `int/mul`, `int/div`, `int/mod`, `int/eq?`,
`int/lt?`, and `int/to-str`. Division and modulo are Euclidean. For nonzero `b`, they return the
unique `q` and `r` satisfying `a = b*q + r` and `0 <= r < abs(b)`. A zero divisor returns trusted
sealed `ERROR` with code `core/numeric-error`; it does not panic the host.

## Fixed Decimals

`Dec` is an opaque scalar type constructed through `dec/parse` or `dec/from-int`. Its canonical
data encoding is exactly:

```text
{:num/kind :fixed-decimal :num/scale Int :num/unscaled Int}
```

The mathematical value is `unscaled * 10^-scale`. Scale is in `0..=4096`. Constructors reject a
larger scale before exponentiation or proportional allocation. Zero always normalizes to unscaled
`0`, scale `0`; nonzero values remove trailing decimal zeroes. `dec/to-str` emits the unique ordinary
decimal spelling with no exponent and no redundant fractional zeroes.

`dec/parse` accepts `[+-]?[0-9]+(\.[0-9]+)?`. The supported operations are `dec/parse`,
`dec/to-str`, `dec/from-int`, `dec/add`, `dec/sub`, `dec/mul`, `dec/eq?`, and `dec/lt?`. Addition,
subtraction, and comparison align exact scales; multiplication adds scales and rejects a result
above 4096. Division, rounding, exponent syntax, implicit integer coercion, and decimal literals are
unsupported in v0.2. Invalid decimal syntax or scale arithmetic returns sealed
`core/numeric-error`; an argument of the wrong language type returns sealed `core/type-error`.

The map encoding is public canonical data for serialization and hashing, but a map literal infers as
`Rec`, not `Dec`. Code should use the constructors so the typechecker can establish `Dec`.

## Floating Point

Core v0.2 has no floating-point type, literal, primitive, NaN, infinity, signed zero, subnormal, or
rounding-mode semantics. A host ABI may carry a profile-scoped float as bytes or structured data,
but it is not a Core number and cannot enter canonical arithmetic without an explicit future profile.

## Serialization And Hashing

Integer identity uses canonical CoreForm integer serialization. Decimal-producing primitives
normalize before returning the canonical map, so equivalent decimal spellings have the same returned
term and value hash. Source spelling is not semantic identity. No host locale, machine word size,
floating-point unit, or accelerator participates in numeric serialization or hashing.

## Execution Tiers

- `gc_kernel` reference evaluation defines numeric semantics.
- The compiled AST evaluator must match reference values, trusted sealed failures, hashes, and
  resource observations.
- `gc_wasm` executes through `gc_kernel` and inherits the reference profile.
- Stage 1 folds integers with arbitrary precision and may not narrow them.
- Stage 2 may use an `i64` candidate representation only behind exact translation validation. Every
  emitted artifact requires a successful validation report; unsupported or mismatched input fails
  closed before artifact authority is returned.
- GPU and accelerator backends define no Core arithmetic in v0.2. Future kernels must name a
  separate negotiated profile and prove conversion and result boundaries.

Direct `stage2_compile_module` output is an unauthoritative compiler candidate. Only the validated
command pipeline can emit an artifact for execution or publication.

## Change Rule

A numeric change updates this prose, the closed JSON and schema, parser/type/runtime/Prelude paths,
all affected backends, positive and malformed controls, agent cards, and migration guidance in one
reviewed transaction. New numeric kinds or operations require profile negotiation; implementation
availability alone is not authority.

## Nonclaims

- This profile does not provide floating point, rational numbers, complex numbers, units, decimal
  division, or rounding.
- It does not claim Stage 2 supports every integer program; it requires unsupported programs to fail
  closed.
- It does not make GPU arithmetic part of Core or grant host capabilities.
- It does not promote a self-host, optimizer, backend, Foundry result, or release level.
