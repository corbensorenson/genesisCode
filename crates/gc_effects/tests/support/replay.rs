use std::path::PathBuf;

use gc_effects::{EffectLog, EffectsError, replay_with_selfhost_authority};
use gc_kernel::{EvalCtx, Value};
use gc_prelude::SelfhostBootstrapMode;

fn artifact() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../selfhost/toolchain.gc")
}

pub(crate) fn replay(
    context: &mut EvalCtx,
    program: Value,
    log: &EffectLog,
) -> Result<Value, EffectsError> {
    replay_with_selfhost_authority(
        context,
        program,
        log,
        None,
        log.program_hash,
        SelfhostBootstrapMode::ArtifactOnly,
        Some(artifact().as_path()),
    )
}
