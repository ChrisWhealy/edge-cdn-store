use std::time::Duration;
use pingora_cache::cache_control::Cacheable::Default;
use crate::utils::env_var_or_str;

pub const DEFAULT_PROXY_PORT_HTTP: u16 = 6188;
pub const DEFAULT_PROXY_PORT_HTTPS: u16 = 6143;
pub const DEFAULT_PORT_HTTP: u16 = 80;
pub const DEFAULT_PORT_HTTPS: u16 = 443;

pub const DEFAULT_CACHE_SIZE_BYTES: usize = 2 * 1024 * 1024 * 1024; // Default cache size = 2Gb
pub const DEFAULT_READ_BUFFER_SIZE: usize = 256 * 1024; // This will probably need to be made configurable

pub const DEFAULT_RUNTIME_DIR: &'static str = "/tmp/edge-cdn-store";
pub const CACHE_STATE_FILENAME: &'static str = "_cache_state.json";

pub const ONE_HOUR: Duration = Duration::from_secs(3600);
pub const HTTPS: &str = "https";
pub const HTTP: &str = "http";
pub const HEX_CHARS: &[u8] = b"0123456789ABCDEF";
