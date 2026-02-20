use std::collections::BTreeMap;
use std::fs::OpenOptions;
#[cfg(not(target_os = "wasi"))]
use std::io::Read;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

#[cfg(not(target_os = "wasi"))]
use fs2::FileExt;
use gc_coreform::{Term, TermOrdKey};
#[cfg(not(target_os = "wasi"))]
use reqwest::StatusCode;
use reqwest::Url;
#[cfg(not(target_os = "wasi"))]
use reqwest::blocking::Client;
use serde::{Deserialize, Serialize};
use thiserror::Error;

#[cfg(not(target_os = "wasi"))]
mod server;
#[cfg(not(target_os = "wasi"))]
pub use server::{
    HttpRegistryServerConfig, HttpRegistryServerHandle, spawn_http_file_registry_server,
};

include!("registry/types_and_client.rs");
include!("registry/client_impl.rs");
include!("registry/remote_helpers.rs");
include!("registry/file_backend.rs");
