#[derive(Subcommand)]
enum SyncCmd {
    /// Pull artifacts reachable from refs and/or explicit roots, and optionally update local refs.
    Pull {
        /// Remote spec (e.g. gen://example.com/registry or https://...).
        #[arg(long)]
        remote: String,

        /// Refs to pull from the remote (may be repeated).
        #[arg(long = "ref")]
        refs: Vec<String>,

        /// Explicit root hashes to pull (may be repeated).
        #[arg(long = "root")]
        roots: Vec<String>,

        /// Commit parent depth to include when roots are commits (0 = no parents).
        #[arg(long, default_value_t = 0)]
        depth: u64,

        /// Overwrite local refs if they differ from the remote.
        #[arg(long)]
        force: bool,
    },

    /// Push artifacts reachable from explicit roots, and optionally advance remote refs (CAS).
    Push {
        /// Remote spec (e.g. gen://example.com/registry or https://...).
        #[arg(long)]
        remote: String,

        /// Explicit root hashes to push (may be repeated).
        #[arg(long = "root")]
        roots: Vec<String>,

        /// Commit parent depth to include when roots are commits (0 = no parents).
        #[arg(long, default_value_t = 0)]
        depth: u64,

        /// Optionally advance remote refs after uploading artifacts.
        ///
        /// Format: `<refname>:<commit-hash>:<policy-hash>[@<expected-old-hash|nil>]`
        #[arg(long = "set-ref")]
        set_refs: Vec<String>,
    },
}

#[derive(Subcommand)]
enum RegistryCmd {
    /// Serve a policy-gated HTTP registry backed by a local file store.
    Serve {
        /// Bind address (supports :0 for ephemeral local ports).
        #[arg(long, default_value = "127.0.0.1:8080")]
        addr: String,

        /// Registry root directory; server stores data under <root>/v1.
        #[arg(long)]
        root: PathBuf,

        /// Maximum accepted upload chunk size in bytes.
        #[arg(long, default_value_t = 4_194_304)]
        max_chunk_bytes: u64,

        /// Optional request cap (mainly for deterministic tests).
        #[arg(long)]
        max_requests: Option<u64>,
    },
}
