use rust_cc::{Context, Finalize, Trace};

use super::{CompiledLexicalEnv, CompiledModuleCells};

impl Finalize for CompiledLexicalEnv {}

unsafe impl Trace for CompiledLexicalEnv {
    fn trace(&self, ctx: &mut Context<'_>) {
        self.0.trace(ctx);
    }
}

impl Finalize for CompiledModuleCells {}

unsafe impl Trace for CompiledModuleCells {
    fn trace(&self, ctx: &mut Context<'_>) {
        self.0.trace(ctx);
    }
}
