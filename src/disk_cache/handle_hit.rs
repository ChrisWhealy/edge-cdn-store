use crate::utils::{impl_trace, trace_fn_exit_with_err, Trace};

use async_trait::async_trait;
use bytes::Bytes;
use pingora_cache::{storage::HandleHit, trace::SpanHandle, CacheKey, Storage};
use std::{any::Any, cmp::min, io::SeekFrom, sync::Arc};
use tokio::{
    fs::File,
    io::{AsyncReadExt, AsyncSeekExt},
};

// - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - -
//  Streaming Hit handler
pub struct DiskHitHandler {
    pub file: File,
    pub total_len: u64,
    pub range_start: u64,
    pub range_end: u64,
    pub pos: u64,           // current read cursor
    pub pending_seek: bool, // seek at next read_body (seek() is not async)
    pub chunk: usize,       // read chunk size
    pub metrics: Arc<crate::metrics::CacheMetrics>,
}

impl_trace!(DiskHitHandler);

// - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - -
#[async_trait]
impl HandleHit for DiskHitHandler {
    async fn read_body(&mut self) -> pingora_error::Result<Option<Bytes>> {
        let fn_name = "read_body";
        <Self as Trace>::fn_enter(fn_name);

        // Deferred seek?
        if self.pending_seek {
            if let Err(e) = self.file.seek(SeekFrom::Start(self.range_start)).await {
                return trace_fn_exit_with_err(fn_name, &format!("seek failed: {e}"), false);
            }

            self.pos = self.range_start;
            self.pending_seek = false;
        }

        if self.pos >= self.range_end {
            <Self as Trace>::fn_exit(fn_name);
            return Ok(None);
        }

        let to_read = min(self.chunk as u64, self.range_end - self.pos) as usize;
        let mut buf = vec![0u8; to_read];
        let n = match self.file.read(&mut buf).await {
            Ok(n) => n,
            Err(e) => return trace_fn_exit_with_err(fn_name, &format!("read of cache file failed: {e}"), false),
        };

        // Have we hit EOF?
        if n == 0 {
            self.pos = self.range_end;
            tracing::debug!("     EOF");
            <Self as Trace>::fn_exit(fn_name);
            return Ok(None);
        }

        self.pos += n as u64;
        buf.truncate(n);

        tracing::debug!("     {n} bytes read");
        <Self as Trace>::fn_exit(fn_name);
        Ok(Some(Bytes::from(buf)))
    }

    // - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - -
    async fn finish(
        self: Box<Self>,
        _storage: &'static (dyn Storage + Sync),
        _key: &CacheKey,
        _trace: &SpanHandle,
    ) -> pingora_error::Result<()> {
        <Self as Trace>::fn_enter_exit("finish");
        self.metrics.served_hits.inc();
        Ok(())
    }

    // - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - -
    fn can_seek(&self) -> bool {
        true
    }

    // - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - -
    fn seek(&mut self, start: usize, end: Option<usize>) -> pingora_error::Result<()> {
        let fn_name = "seek";
        <Self as Trace>::fn_enter(fn_name);

        let end = if let Some(end) = end {
            if end > self.total_len as usize {
                return trace_fn_exit_with_err(
                    fn_name,
                    &format!("seek end > length: {end} > {}", self.total_len),
                    false,
                );
            }

            end as u64
        } else {
            self.total_len
        };

        if start as u64 > end {
            return trace_fn_exit_with_err(fn_name, &format!("seek start > end: {start} > {end}"), false);
        }

        self.range_start = start as u64;
        self.range_end = end;
        self.pending_seek = true; // defer actual seek to next read_body() call

        <Self as Trace>::fn_exit(fn_name);
        Ok(())
    }

    // - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - -
    fn get_eviction_weight(&self) -> usize {
        // This should be safe for practical sizes, but might need to clamp
        self.total_len as usize
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
