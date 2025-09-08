use pingora_cache::{
    key::{CacheHashKey, CompactCacheKey},
    CacheKey,
};
use pingora_error::{Error, ErrorType};
use std::fmt::Write;
use std::str::FromStr;

const HEX_CHARS: &[u8] = b"0123456789ABCDEF";

// - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - -
// Returns an environment variable or falls back to the default
pub fn env_var_or_str(var_name: &str, default: &'static str) -> String {
    std::env::var(var_name).unwrap_or_else(|_| default.to_string())
}

// Parses an environment variable value as a number or falls back to the default
pub fn env_var_or_num<T>(var_name: &str, default: T) -> T
where
    T: FromStr + Copy,
{
    std::env::var(var_name)
        .ok()
        .and_then(|s| s.trim().parse::<T>().ok())
        .unwrap_or(default)
}

// - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - -
// Parse a Host header (authority): host[:port] or [IPv6]:port
// Handle malformed values gracefully by returning HTTP 400 Bad Request
pub fn parse_host_authority(raw: &str) -> pingora_error::Result<(String, Option<u16>)> {
    // Handle malformed value - trailing slash
    let s = raw.trim().trim_end_matches('/');

    if s.is_empty() {
        Error::e_explain(ErrorType::HTTPStatus(400), "Host header empty")
    } else {
        // IPv6 address?
        if let Some(rest) = s.strip_prefix('[') {
            // IPv6 literal: "[::1]" or "[::1]:6143"
            if let Some(end) = rest.find(']') {
                let host = &rest[..end]; // no brackets for SNI
                let after = &rest[end + 1..]; // maybe ":port"
                let port = after.strip_prefix(':').and_then(|p| p.parse::<u16>().ok());
                Ok((host.to_string(), port))
            } else {
                Error::e_explain(ErrorType::HTTPStatus(400), "Malformed IPv6 address in Host")
            }
        } else {
            // hostname / IPv4: "example.com[:port]"
            if s.contains('/') || s.contains(' ') {
                Error::e_explain(ErrorType::HTTPStatus(400), "Invalid characters in Host")
            } else {
                let mut it = s.splitn(2, ':');
                let host = it.next().unwrap();
                let port = it.next().and_then(|p| p.parse::<u16>().ok());

                Ok((host.to_string(), port))
            }
        }
    }
}

// - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - -
// Development helper functions
pub fn trace_fn_exit(fn_name: &str, err_msg: &str, trace_fn_enter: bool) {
    if trace_fn_enter {
        tracing::debug!("---> {fn_name}");
    }
    tracing::debug!("     {err_msg}");
    tracing::debug!("<--- {fn_name}");
}

pub fn trace_fn_exit_with_err<E>(fn_name: &str, err_msg: &str, trace_fn_enter: bool) -> pingora_error::Result<E> {
    trace_fn_exit(fn_name, err_msg, trace_fn_enter);
    Error::e_explain(ErrorType::InternalError, err_msg.to_string())
}

// - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - -
pub fn display_opt_str(opt_str: &Option<&str>) -> String {
    match opt_str {
        Some(s) if !s.is_empty() => s.to_string(),
        _ => "\"\"".to_string(),
    }
}

pub fn hex2str(hex: &[u8; 16]) -> String {
    let mut s = String::with_capacity(hex.len() * 2);

    for &byte in hex {
        s.push(HEX_CHARS[(byte >> 4) as usize] as char);
        s.push(HEX_CHARS[(byte & 0xf) as usize] as char);
    }
    s
}

#[allow(dead_code)]
pub fn format_cache_key(ck: &CacheKey) -> String {
    let mut buff: String = String::new();

    let ns = format!("Namespace: {}", display_opt_str(&ck.namespace_str()));
    let pk = format!("Primary Key: {}", display_opt_str(&ck.primary_key_str()));
    let p = format!("Primary: {:?}", hex2str(&ck.primary_bin()));
    let v = format!("Variance: {:?}", ck.variance());
    let ut = format!("User tag: {}", if ck.user_tag.is_empty() { "\"\"" } else { &ck.user_tag });
    let ext = format!("Extension: {:?}", ck.extensions);

    write!(buff, "{ns}, {pk}, {p}, {v}, {ut}, {ext}").unwrap();
    buff
}

#[allow(dead_code)]
pub fn format_compact_cache_key(ck: &CompactCacheKey) -> String {
    let mut buff: String = String::new();

    let p = format!("Primary: {:?}", hex2str(&ck.primary_bin()));
    let v = format!("Variance: {:?}", ck.variance());
    let ut = format!("User tag: {}", if ck.user_tag.is_empty() { "\"\"" } else { &ck.user_tag });

    write!(buff, "{p}, {v}, {ut}").unwrap();
    buff
}
