# Prelude Modules

`prelude/prelude.gc` is an assembled artifact.

Authoring source lives in ordered module files:
- `00_core.gc`
- `10_gfx.gc`
- `20_editor.gc`

To rebuild the assembled prelude:

```bash
scripts/assemble_prelude.sh
```

Validation is enforced in `crates/gc_prelude/tests/prelude_modularization.rs`.
