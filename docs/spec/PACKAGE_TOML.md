# `package.toml` (Package Manifest) v0.2

This file defines a package, its modules, dependencies, and required obligations.

## Required Keys

- `schema` (integer): current writers emit `1`. The documented pre-schema form is read as schema 1 under migration `M-PACKAGE-PRESCHEMA-TO-1`; every other explicit value is rejected.
- `name` (string)
- `version` (string)
- `modules` (array of tables): `[{ path = "...", hash = "..." }, ...]`
- `dependencies` (array of tables): `[{ name = "...", path = "...", hash = "..." }, ...]`
- `obligations` (array of strings): obligation IDs to run
  - includes `core/obligation::ai-style` for AI-oriented module quality gates

## Optional Keys

For compatibility with the documented pre-schema form, an omitted `dependencies`
key is normalized to an empty array. Current writers still emit the key. Omitted
`tests`, `property_tests`, `limits`, `budgets`, `property`, and `gfx` values use
their documented empty or false defaults. Unknown TOML keys are ignored by the
v0.2 structural admission boundary; they grant no capability and have no package
identity or execution meaning unless a later schema version assigns one.

- `tests` (array of strings): suite symbols to execute as unit tests
- Coverage obligations over these unit tests:
  - `core/obligation::coverage` (symbol profile)
  - `core/obligation::coverage-decision` (statement+decision profile)
  - `core/obligation::coverage-mcdc` (MC/DC profile)
- `property_tests` (array of strings): suite symbols to execute as property tests (used by `core/obligation::property-tests`)
- `caps_policy` (string): path to a `caps.toml` relative to the manifest directory
- `limits` (table): evaluation limits enforced for package evaluation and tests
- `budgets` (table): per-test budgets enforced by the `core/obligation::budgets` obligation
- `property` (table): configuration for property tests
- `gfx` (table): graphics-obligation configuration used by:
  - `core/obligation::gfx-golden-images`
  - `core/obligation::gfx-frame-budgets`
  - `core/obligation::gfx-api-stability`
- `core/obligation::ai-style` emits canonical machine-readable diagnostics and patch-intent metadata (see `docs/spec/AI_STYLE.md`)

`limits` keys:
- `step_limit` (integer, optional): kernel evaluation step limit for package evaluation/tests
  - If omitted, the v0.2 toolchain default is used.
- `allow_unlimited` (bool, default `false`): if `true`, permits disabling the step limit via `genesis test --no-step-limit`.
- `max_alloc_units` (integer, optional): maximum cumulative representation-independent logical allocation units
- `max_live_units` (integer, optional): maximum logical units reachable from evaluator roots at a safe point
- `max_pair_cells` (integer, optional): maximum total number of `pair/cons` cells allocated during evaluation
- `max_vec_len` (integer, optional): maximum observed vector length (vector literals and `vec/push`)
- `max_map_len` (integer, optional): maximum observed map length (map literals, `map/put`, `map/merge`)
- `max_bytes_len` (integer, optional): maximum observed bytes length (bytes literals and `bytes/concat`)
- `max_string_len` (integer, optional): maximum observed string length in UTF-8 bytes (string literals and `str/concat`)

`budgets` keys (all optional):
- `max_steps_per_test` (integer): maximum kernel evaluation steps for each unit test
- `max_effect_entries_per_test` (integer): maximum effect log entries for each effectful test
- `max_effect_log_bytes_per_test` (integer): maximum canonical `.gclog` byte length for each effectful test

`property` keys:
- `cases_per_test` (integer, optional): default cases per property test when not specified by the test entry

`gfx` keys:
- `golden_tests` (array of strings): suite symbols for golden graphics checks.
  - Each suite entry must be a map test entry:
    - `:body` callable
    - `:kind` `:frame-graph | :scene` (defaults to `:frame-graph`)
    - `:expect-h` 64-char lowercase hex expected hash
    - optional pixel-golden fields (frame-graph only):
      - `:expect-png-h` 64-char lowercase hex expected PNG hash
      - `:pixel-width` / `:pixel-height` positive integers (defaults `256`)
- `frame_budget_tests` (array of strings): suite symbols for frame budget checks.
  - Each suite entry is a callable or `{ :body callable }`.
  - Body result must be:
    - a `:gfx/frame-graph` term, or
    - `{ :frame <frame-graph> :frame-time-ms <int|nil> }`
- `api_exports` (array of strings): strict expected exported gfx symbols (typically `core/gfx/*`) for API stability checks.
- `api_surface_hash` (string): expected 64-char lowercase hex surface hash (computed from tracked export symbols + defining expression hashes).
- `max_render_passes_per_frame` (integer, optional)
- `max_compute_passes_per_frame` (integer, optional)
- `max_draw_commands_per_frame` (integer, optional)
- `max_compute_commands_per_frame` (integer, optional)
- `max_frame_graph_bytes` (integer, optional)
- `max_frame_time_ms` (integer, optional)

## Module Table

Each entry:
- `path` (string): Unicode-NFC module file path relative to the manifest directory, using `/` separators; it must not be empty or contain a leading slash, drive prefix, backslash, repeated slash, `.` component, or `..` component
- `hash` (string): BLAKE3 hex of the module *canonical printed bytes* with the `GCv0.2` tag

`genesis pack --pkg package.toml` computes and writes module hashes.

## Dependency Table

Each entry:
- `name` (string)
- `path` (string): Unicode-NFC local path to the dependency package directory, with the same portable package-relative constraints as module paths
- `hash` (string): BLAKE3 hex of the dependency package artifact hash (as produced by `genesis pack`)

## Normative Behavior

- Artifact-loaded `core/pkg::package-manifest-authority` exclusively decides
  structural package-manifest admission and normalization for production CLI,
  obligation, recursive dependency, and patch routes. Its request and result are
  bound to the exact BLAKE3 hash of the source bytes. Rust may perform a bounded
  file read, UTF-8 validation, generic TOML syntax decoding into a neutral term,
  bounded artifact evaluation, strict closed result decoding, and typed structure
  materialization; those mechanisms do not decide manifest semantics.
- The retained Rust `PackageManifest::load` implementation is a compatibility
  oracle for tests and the explicit parity profile, plus a named temporary
  production residual in package-low and editor effect routes. Those effect routes
  remain host-authoritative until their separate R4.2.e lifecycle transaction;
  this partial cutover does not claim H2 or aggregate R4.2.e closure.

- `genesis test` must verify that each module’s current hash matches the pinned `hash` field.
- Dependencies must be hash-checked before use (local path deps are allowed but must match pinned hashes).
- Package acceptance is granted only if all listed `obligations` succeed.
- Module vector order is semantic and identity-bound. Under
  `genesis/module-resolution-profile-v0.1`, local imports may target only public exports of earlier
  entries; forward, self, and cyclic imports fail closed.
- There is no ambient workspace override. A local replacement requires an explicit manifest/lock
  update naming exact content identities and produces a new package closure identity.
- Package evaluation limits are enforced for `genesis test` and `genesis apply-patch`:
  - if `limits.allow_unlimited = false` (default), `--no-step-limit` must be rejected as a manifest policy error
  - the effective step limit is the minimum of the CLI request (if any) and `limits.step_limit` (or the toolchain default when omitted)
  - each effective memory limit is the minimum of the CLI value and manifest value when both are present; either side may tighten an omitted value
  - logical unit definitions, safe points, reset behavior, and sealed exhaustion payloads are normative in `docs/spec/VALUE_EFFECT_HASH.md`
