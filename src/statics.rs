use crate::consts::DEFAULT_RUNTIME_DIR;

use crate::utils::env_var_or_str;
use std::{
    net::{IpAddr, Ipv4Addr},
    sync::OnceLock,
};

// - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - -
pub static IN_ADDR_ANY: IpAddr = IpAddr::V4(Ipv4Addr::UNSPECIFIED);
pub static LOCALHOST: IpAddr = IpAddr::V4(Ipv4Addr::LOCALHOST);

// - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - -

static RUNTIME_DIR: OnceLock<String> = OnceLock::new();
pub fn runtime_dir() -> &'static str {
    RUNTIME_DIR.get_or_init(|| env_var_or_str("EDGE_RUNTIME_DIR", DEFAULT_RUNTIME_DIR))
}

static CACHE_DIR: OnceLock<String> = OnceLock::new();
pub fn cache_dir() -> &'static str {
    CACHE_DIR.get_or_init(|| format!("{}/cache", runtime_dir()))
}

static PATH_TO_SERVER_KEYS: OnceLock<String> = OnceLock::new();
pub fn server_keys_dir() -> &'static str {
    PATH_TO_SERVER_KEYS.get_or_init(|| format!("{}/keys", runtime_dir()))
}

static PATH_TO_APP_LOG: OnceLock<String> = OnceLock::new();
pub fn path_to_app_log() -> &'static str {
    PATH_TO_APP_LOG.get_or_init(|| format!("{}/app.log", runtime_dir()))
}

static PATH_TO_PANIC_LOG: OnceLock<String> = OnceLock::new();
pub fn path_to_panic_log() -> &'static str {
    PATH_TO_PANIC_LOG.get_or_init(|| format!("{}/panic.log", runtime_dir()))
}
