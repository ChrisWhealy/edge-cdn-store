use std::{
    net::{IpAddr, Ipv4Addr},
    sync::OnceLock,
};

// - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - -
pub static DEFAULT_PROXY_PORT_HTTP: u16 = 6188;
pub static DEFAULT_PROXY_PORT_HTTPS: u16 = 6143;
pub static DEFAULT_PORT_HTTP: u16 = 80;
pub static DEFAULT_PORT_HTTPS: u16 = 443;
pub static DEFAULT_CACHE_SIZE_BYTES: &'static usize = &(2 * 1024 * 1024 * 1024); // Default cache size = 2Gb
pub static IN_ADDR_ANY: IpAddr = IpAddr::V4(Ipv4Addr::UNSPECIFIED);
pub static LOCALHOST: IpAddr = IpAddr::V4(Ipv4Addr::LOCALHOST);

// - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - -
pub static DEFAULT_READ_BUFFER_SIZE: usize = 256 * 1024; // This will probably need to be made configurable
pub static CACHE_STATE_FILENAME: &str = "_cache_state.json";

static RUNTIME_DIR: &'static str = "/tmp/edge-cdn-store";

static CACHE_DIR: OnceLock<String> = OnceLock::new();
pub fn cache_dir() -> &'static str {
    CACHE_DIR.get_or_init(|| format!("{RUNTIME_DIR}/cache"))
}

static PATH_TO_SERVER_KEYS: OnceLock<String> = OnceLock::new();
pub fn server_keys_dir() -> &'static str {
    PATH_TO_SERVER_KEYS.get_or_init(|| format!("{RUNTIME_DIR}/keys"))
}

static PATH_TO_APP_LOG: OnceLock<String> = OnceLock::new();
pub fn path_to_app_log() -> &'static str {
    PATH_TO_APP_LOG.get_or_init(|| format!("{RUNTIME_DIR}/app.log"))
}

static PATH_TO_PANIC_LOG: OnceLock<String> = OnceLock::new();
pub fn path_to_panic_log() -> &'static str {
    PATH_TO_PANIC_LOG.get_or_init(|| format!("{RUNTIME_DIR}/panic.log"))
}
