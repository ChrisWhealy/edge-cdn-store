use async_trait::async_trait;
use pingora_core::{server::ShutdownWatch, services::background::BackgroundService};
use pingora_error::{Error, ErrorType};
use tracing_subscriber::EnvFilter;

// - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - -
pub struct BackgroundLogger {
    pub path: std::path::PathBuf,
    // Set to "edge_cdn_store=debug,pingora=info" during development
    pub filter: Option<String>,
}

#[async_trait]
impl BackgroundService for BackgroundLogger {
    // Ensure that trace logs are written to a file, not stdout/stderr
    async fn start(&self, _shutdown: ShutdownWatch) {
        let file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)
            .expect("open app.log");

        let filter = self.filter.as_deref().map(EnvFilter::new).unwrap_or_else(|| {
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"))
        });

        tracing_subscriber::fmt()
            .with_env_filter(filter)
            .with_target(true)
            .with_ansi(false)
            .with_writer(file) // sync writer; no background thread
            .init();

        tracing::info!("background logger initialized -> {}", self.path.display());

        // keep service alive until shutdown
        futures_util::future::pending::<()>().await;
    }
}

// - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - -
pub trait Trace {
    fn struct_name() -> &'static str;
    fn fn_enter(fn_name: &str) {
        tracing::debug!("---> {}::{fn_name}()", Self::struct_name())
    }
    fn fn_enter_exit(fn_name: &str) {
        tracing::debug!("<--> {}::{fn_name}()", Self::struct_name())
    }
    fn fn_exit(fn_name: &str) {
        tracing::debug!("<--- {}::{fn_name}()", Self::struct_name())
    }
}

macro_rules! impl_trace {
    ($name:ident $(<$($gen:tt),* $(,)? >)? $(where $($whr:tt)*)? ) => {
        impl $(<$($gen),*>)? Trace for $name $(<$($gen),*>)? $(where $($whr)*)? {
            fn struct_name() -> &'static str {
                stringify!($name)
            }
        }
    };
}
pub(crate) use impl_trace;

// - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - -
// Development helper functions
pub fn trace_fn_exit(fn_name: &str, err_msg: &str, trace_fn_enter: bool) {
    if trace_fn_enter {
        tracing::debug!("---> {fn_name}");
    }
    tracing::debug!("     {err_msg}");
    tracing::debug!("<--- {fn_name}");
}

pub fn trace_fn_exit_with_err<E>(fn_name: &str, err_msg: &str, error_type: Option<ErrorType>, trace_fn_enter: bool) -> pingora_error::Result<E> {
    trace_fn_exit(fn_name, err_msg, trace_fn_enter);
    Error::e_explain(error_type.unwrap_or(ErrorType::InternalError), err_msg.to_string())
}
