use super::Value;

impl Value {
    pub(crate) fn closure_captured_value_count(&self) -> Option<usize> {
        match self {
            Self::Closure(data) => Some(data.env.captured_local_binding_count()),
            Self::CompiledClosure(data) => Some(
                data.compiled_env
                    .as_ref()
                    .map_or(0, |env| env.captured_value_count()),
            ),
            _ => None,
        }
    }

    pub(crate) fn compiled_closure_capture_slot_span(&self) -> Option<usize> {
        match self {
            Self::CompiledClosure(data) => {
                Some(data.compiled_env.as_ref().map_or(0, |env| env.slot_span()))
            }
            _ => None,
        }
    }
}
