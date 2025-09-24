use crate::consts::RUNTIME_DIR;

use std::{
    net::{IpAddr, Ipv4Addr},
    sync::OnceLock,
};

// - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - -
pub static IN_ADDR_ANY: IpAddr = IpAddr::V4(Ipv4Addr::UNSPECIFIED);
pub static LOCALHOST: IpAddr = IpAddr::V4(Ipv4Addr::LOCALHOST);

// - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - -

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
