use crate::{metrics::CacheMetrics, utils::{Trace, impl_trace, trace_fn_exit_with_err}};

use async_trait::async_trait;
use bytes::Bytes;
use pingora_cache::storage::{HandleMiss, MissFinishType};
use pingora_error::{Error, ErrorType};
use std::{
    path::{Path, PathBuf},
    sync::Arc,
};
use tokio::{fs, io::AsyncWriteExt};

// - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - -
// Miss handler
pub struct DiskMissHandler {
    pub tmp_path: PathBuf,
    pub tmp_bytes_written: usize,

    // final locations
    pub dir: PathBuf,
    pub body_path: PathBuf,
    pub meta_path: PathBuf,
    pub hdr_path: PathBuf,

    pub meta_internal: Vec<u8>,
    pub meta_header: Vec<u8>,

    pub metrics: Arc<CacheMetrics>,
}

impl_trace!(DiskMissHandler);

// - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - -
impl DiskMissHandler {
    pub async fn create_tmp(path_dir: &Path, hint: &str) -> pingora_error::Result<(PathBuf, fs::File)> {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let tmp_name = format!("{}.tmp-{}-{}", hint, std::process::id(), nanos);
        let tmp_path = path_dir.join(tmp_name);

        if let Some(parent) = tmp_path.parent() {
            fs::create_dir_all(parent).await.ok(); // best-effort
        }

        match fs::File::create(&tmp_path).await {
            Ok(f) => Ok((tmp_path, f)),
            Err(e) => Error::e_explain(ErrorType::InternalError, format!("failed to create tmp file: {e}")),
        }
    }
}

// - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - -
#[async_trait]
impl HandleMiss for DiskMissHandler {
    async fn write_body(&mut self, data: Bytes, is_eof: bool) -> pingora_error::Result<()> {
        // Due to the frequency with which this function is called, trace output is only written when an error occurs
        let fn_name = "write_body";
        <Self as Trace>::fn_enter(fn_name);

        // Temp file created in get_miss_handler - should be able to open it here
        let mut file = match fs::OpenOptions::new().append(true).open(&self.tmp_path).await {
            Ok(f) => f,
            Err(e) => {
                return trace_fn_exit_with_err(fn_name, &format!("Can't open tmp file for append: {e}"), true);
            },
        };

        if let Err(e) = file.write_all(&data).await {
            return trace_fn_exit_with_err(fn_name, &format!("Error writing to tmp file: {e}"), true);
        }

        self.tmp_bytes_written += data.len();

        if is_eof {
            // flush IO buffer but leave rename to finish()
            file.flush().await.ok();
        }

        <Self as Trace>::fn_exit(fn_name);
        Ok(())
    }

    // - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - -
    async fn finish(self: Box<Self>) -> pingora_error::Result<MissFinishType> {
        let fn_name = "finish";
        <Self as Trace>::fn_enter(fn_name);

        // Ensure directory exists (idempotent)
        fs::create_dir_all(&self.dir).await.ok();

        // Promote tmp file to actual cache file
        if let Err(e) = fs::rename(&self.tmp_path, &self.body_path).await {
            tracing::debug!("rename of tmp file to cache file failed. Attempting manual transfer: {e}");

            // If rename fails manually transfer data from tmp location to cache location
            let mut tmp_file = match fs::File::open(&self.tmp_path).await {
                Ok(f) => f,
                Err(e) => {
                    return trace_fn_exit_with_err(fn_name, &format!("Can't open tmp file for append: {e}"), false);
                },
            };

            let mut cache_file = match fs::File::create(&self.body_path).await {
                Ok(f) => f,
                Err(e) => {
                    return trace_fn_exit_with_err(fn_name, &format!("Can't create cache file: {e}"), false);
                },
            };

            if let Err(e) = tokio::io::copy(&mut tmp_file, &mut cache_file).await {
                return trace_fn_exit_with_err(fn_name, &format!("Failed to copy tmp to cache body: {e}"), false);
            }

            // clean up tmp
            let _ = fs::remove_file(&self.tmp_path).await;
        }

        // Write meta parts
        if let Err(e) = fs::write(&self.meta_path, &self.meta_internal).await {
            return trace_fn_exit_with_err(fn_name, &format!("Failed to write cache meta: {e}"), false);
        }

        if let Err(e) = fs::write(&self.hdr_path, &self.meta_header).await {
            return trace_fn_exit_with_err(fn_name, &format!("Failed to write cache hdr: {e}"), false);
        }

        self.metrics.inserts.inc();
        self.metrics.size_bytes.add(self.tmp_bytes_written as i64);

        tracing::debug!("     {} bytes written", self.tmp_bytes_written);
        <Self as Trace>::fn_exit(fn_name);
        Ok(MissFinishType::Created(self.tmp_bytes_written))
    }

    // - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - -
    // We don't support associating a streaming write tag for partial read hits.
    fn streaming_write_tag(&self) -> Option<&[u8]> {
        None
    }
}

// - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - -
// Should the writer be dropped before finish() is called, clean up tmp file ignoring any errors this might generate
impl Drop for DiskMissHandler {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.tmp_path);
    }
}
