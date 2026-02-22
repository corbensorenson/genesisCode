# AI Style Obligation (`core/obligation::ai-style`)

This document defines the normative behavior of the AI-oriented style obligation used as a quality gate for AI-authored Genesis modules.

Consolidated source:
- legacy top-level style guidance (redirected through the deprecation map)

## Purpose

`core/obligation::ai-style` enforces:
- stable machine-readable diagnostics
- canonical fix schemas
- patch-intent metadata for automated remediation

The obligation runs on package modules and emits a content-addressed report artifact.

## Evaluation Rules

`core/obligation::ai-style` MUST:
1. execute the lint analyzer (`core/editor/lint::lint-module`) for every module.
2. normalize diagnostics into canonical records with stable IDs.
3. emit canonical fix entries when an autofix patch is available.
4. emit patch-intent metadata for each autofix patch.
5. fail when any diagnostic is `:error`.
6. fail when any warning is in the required-style set:
   - `editor/lint/missing-meta`
   - `editor/lint/malformed-meta`
   - `editor/lint/missing-exports`
   - `editor/lint/export-not-symbol`
   - `editor/lint/missing-types-map`
   - `editor/lint/missing-type`
   - `editor/lint/missing-intent`
   - `editor/lint/intent-not-string`
   - `editor/lint/missing-caps`
   - `editor/lint/caps-not-vector`

## Report Schema

The artifact is a CoreForm map with:
- `:kind = "genesis/ai-style-v0.1"`
- `:schema = "genesis/diagnostics-schema-v1"`
- `:obligation = "core/obligation::ai-style"`
- `:package` string
- `:ok` bool
- `:lint-artifact` hash string
- `:diagnostics` vector of canonical diagnostic maps
- `:patch-intents` vector of patch-intent maps
- `:errors` vector of human-readable failure strings

Canonical diagnostic map keys:
- `:id` string (`<path>#<diag-index>#<code>`)
- `:code` string
- `:severity` symbol (`:error | :warn | :info`)
- `:message` string
- `:path` string
- `:symbol` symbol or `nil`
- `:module-index` int
- `:diag-index` int
- `:fixes` vector

Canonical fix map keys:
- `:kind = :gcpatch`
- `:schema = "genesis/fix-schema-v1"`
- `:patch` hash string
- `:intent` string
- `:reasons` vector of strings

Patch-intent map keys:
- `:path` string
- `:patch` hash string
- `:schema = "genesis/patch-intent-v1"`
- `:intent` string
- `:reasons` vector of strings
