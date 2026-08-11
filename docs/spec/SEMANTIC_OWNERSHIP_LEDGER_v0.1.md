# Semantic Ownership Ledger v0.1

Status: normative.

## Purpose

`docs/spec/SEMANTIC_OWNERSHIP_LEDGER_v0.1.json` is the canonical inventory of
GenesisCode command and semantic-decision ownership. Its closed schema is
`docs/spec/SEMANTIC_OWNERSHIP_LEDGER_v0.1.schema.json`. The ledger reports current
truth; it does not infer authority from command routing, file extensions, migration
tables, or generated status prose.

## Closed Command Inventory

The command surface is every leaf reachable from the Clap `Cmd` enum, including
hidden internal commands. The validator parses the complete `cli_args.rs` include
closure, applies Clap's kebab-case and explicit-name rules, expands each ledger
selector, and requires exactly-once coverage. A new, removed, hidden, renamed,
duplicated, or uncovered command fails the gate.

Each command binding names its routing implementation, tests, and all semantic
decisions it can request. `SD-ROUTE-SELECTION` is deliberately separate from those
functional decisions: proving that a command enters a `.gc` artifact does not prove
that GenesisCode computes the requested result.

## Semantic Decision Rows

Every decision row names:

- normative specification authority;
- producing implementations and the implementation that controls production output;
- stage0 domain and explicit host bindings;
- independently controlled verifier/check paths and behavioral tests;
- exact fallback reachability and rollback posture;
- applicability disposition and current H-level assessment;
- command selectors or an explicit internal-only declaration;
- migration/closure tasks and residual limitations.

`currentLevel: null` means the applicable decision has not proven H0. It is a
fail-closed reporting state, not a new closure level. `N/A` is represented only by a
non-applicable disposition under the closure-level contract and likewise is not an
H-level. No row in v0.1 claims H1 or higher.

## Coverage and Authority Rules

1. Every command leaf is covered exactly once and references existing decisions.
2. Every decision is referenced by a command or is explicitly `internalOnly`.
3. Every path is repository-relative, exists, and is classified by role.
4. Every applicable row names fallback reachability; only H2 or higher may claim
   `none-proven`.
5. H0 requires route-custody evidence for the exact decision, not merely the command
   group's dashboard row.
6. A residual-stage0 row must name S0-K or S0-H and the retained trust reason.
7. Production authority and verifier custody cannot be the same path for an H2+
   promotion.
8. Missing, stale, contradictory, or unknown ownership facts fail closed; generated
   views cannot repair the source ledger.

## Nonclaims

- This inventory changes no route, implementation, fallback, or production authority.
- H0 rows prove routing only; null rows prove no closure level.
- It does not establish H2 authority, H3 bootstrap closure, or H4 independence.
- It does not authorize GenesisBench campaigns, Foundry, GenesisChallenge, or Genesis
  Model work.
