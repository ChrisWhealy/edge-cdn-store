use crate::{metrics::CacheMetrics, utils::trace_fn_exit_with_err};

use async_trait::async_trait;
use bytes::Bytes;
use pingora_cache::{
    storage::HandleHit,
    trace::SpanHandle,
    {CacheKey, Storage},
};
use std::{any::Any, sync::Arc};

// - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - -
//  Hit handler
pub struct DiskHitHandler {
    pub body: Arc<Vec<u8>>,
    pub done: bool,
    pub range_start: usize,
    pub range_end: usize,
    pub metrics: Arc<CacheMetrics>,
}

// - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - -
#[async_trait]
impl HandleHit for DiskHitHandler {
    async fn read_body(&mut self) -> pingora_error::Result<Option<Bytes>> {
        tracing::debug!("<--> DiskHitHandler::read_body()");

        Ok(if self.done {
            None
        } else {
            self.done = true;
            Some(Bytes::copy_from_slice(&self.body.as_slice()[self.range_start..self.range_end]))
        })
    }

    // - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - -
    async fn finish(
        self: Box<Self>,
        _storage: &'static (dyn Storage + Sync),
        _key: &CacheKey,
        _trace: &SpanHandle,
    ) -> pingora_error::Result<()> {
        tracing::debug!("<--> DiskHitHandler::finish()");
        self.metrics.served_hits.inc();
        Ok(())
    }

    // - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - -
    fn can_seek(&self) -> bool {
        true
    }

    // - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - -
    fn seek(&mut self, start: usize, end: Option<usize>) -> pingora_error::Result<()> {
        let fn_name = "DiskHitHandler::seek()";
        tracing::debug!("---> {fn_name}");

        if start >= self.body.len() {
            trace_fn_exit_with_err(
                fn_name,
                &format!("out-of-range seek start. {start} >= {}", self.body.len()),
                false,
            )
        } else {
            self.range_start = start;

            if let Some(end) = end {
                self.range_end = std::cmp::min(self.body.len(), end);
            }
            self.done = false; // Range read is complete

            tracing::debug!("<--- {fn_name}");
            Ok(())
        }
    }

    // - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - -
    fn get_eviction_weight(&self) -> usize {
        self.body.len()
    }

    // - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - -
    fn as_any(&self) -> &(dyn Any + Send + Sync) {
        self
    }

    // - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - -
    fn as_any_mut(&mut self) -> &mut (dyn Any + Send + Sync) {
        self
    }
}
