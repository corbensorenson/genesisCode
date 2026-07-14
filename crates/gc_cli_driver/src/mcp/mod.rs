mod catalog;
mod resources;
mod session;

pub(crate) use catalog::interface_manifest;
pub(crate) use session::cmd_mcp;

pub(crate) struct McpOptions<'a> {
    pub(crate) prime_selfhost: bool,
    pub(crate) max_queue: usize,
    pub(crate) max_frame_bytes: usize,
    pub(crate) max_output_bytes: usize,
    pub(crate) max_requests: u64,
    pub(crate) max_roots: usize,
    pub(crate) workspace_root: &'a std::path::Path,
}
