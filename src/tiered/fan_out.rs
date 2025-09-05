use async_trait::async_trait;
use pingora_cache::{MissHandler, storage::{HandleMiss, MissFinishType}};
use pingora_error::{Error, ErrorType};

// - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - -
/// Fanout miss handler that writes to primary and (optionally) secondary.
/// - Primary failure is fatal.
/// - Secondary failure is logged and ignored (best-effort write-through).
pub struct FanoutMiss {
    pub primary: Option<MissHandler>,
    pub secondary: Option<MissHandler>,
    pub created_bytes_primary: usize,
}

// - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - -
#[async_trait]
impl HandleMiss for FanoutMiss {
    async fn write_body(&mut self, data: bytes::Bytes, is_eof: bool) -> pingora_error::Result<()> {
        let fn_name = "FanoutMiss::write_body()";
        tracing::debug!("---> {fn_name}");

        if let Some(p) = self.primary.as_mut() {
            p.write_body(data.clone(), is_eof).await?;
        } else {
            return Error::e_explain(ErrorType::InternalError, "primary miss handler missing");
        }

        if let Some(s) = self.secondary.as_mut() {
            if let Err(e) = s.write_body(data, is_eof).await {
                tracing::warn!("secondary write_body failed (ignored): {e}");
                // keep going; best-effort
            }
        }

        tracing::debug!("<--- {fn_name}");
        Ok(())
    }

    // - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - -
    async fn finish(self: Box<Self>) -> pingora_error::Result<MissFinishType> {
        let fn_name = "FanoutMiss::finish()";
        tracing::debug!("---> {fn_name}");

        // Finish primary first (returns Created(n) or NotModified/etc.)
        let mut me = *self;
        let primary = if let Some(p) = me.primary.take() {
            p
        } else {
            tracing::debug!("<--- {fn_name}");
            return pingora_error::Error::e_explain(ErrorType::InternalError, "primary miss handler missing at finish")
        };

        let result = primary.finish().await?;

        if let MissFinishType::Created(n) = result {
            me.created_bytes_primary = n;
        }

        // Finish secondary (best-effort)
        if let Some(s) = me.secondary {
            if let Err(e) = s.finish().await {
                tracing::warn!("secondary finish failed (ignored): {e}");
            }
        }

        tracing::debug!("<--- {fn_name}");
        Ok(result) // eviction accounting should use the primary's Created(bytes)
    }

    // - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - -
    fn streaming_write_tag(&self) -> Option<&[u8]> {
        None
    }
}
