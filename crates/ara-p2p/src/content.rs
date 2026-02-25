use std::path::Path;

use anyhow::Result;
use futures_lite::StreamExt;
use iroh::NodeAddr;
use iroh_blobs::net_protocol::DownloadMode;
use iroh_blobs::rpc::client::blobs::{DownloadOptions, MemClient as BlobsClient, WrapOption};
use iroh_blobs::store::ExportFormat;
use iroh_blobs::store::ExportMode;
use iroh_blobs::util::SetTagOption;
use iroh_blobs::{BlobFormat, Hash};
use tracing::info;

/// Content hash (BLAKE3, 32 bytes) — the content identifier used for P2P retrieval
/// and registered on-chain in ContentRegistry.
pub type ContentHash = [u8; 32];

/// Manages content blob import/export via iroh.
pub struct ContentManager {
    client: BlobsClient,
}

impl ContentManager {
    pub fn new(client: BlobsClient) -> Self {
        Self { client }
    }

    /// Import a local file into the iroh blob store.
    /// Returns the BLAKE3 hash that identifies this content on the network.
    pub async fn import_file(&self, file_path: &Path) -> Result<ContentHash> {
        info!("Importing file: {}", file_path.display());

        let outcome = self
            .client
            .add_from_path(
                file_path.to_path_buf(),
                false, // copy into store (don't reference in-place)
                SetTagOption::Auto,
                WrapOption::NoWrap,
            )
            .await?
            .finish()
            .await?;

        let hash_bytes = *outcome.hash.as_bytes();
        info!(
            "Imported file: {} -> hash: {} ({} bytes)",
            file_path.display(),
            outcome.hash,
            outcome.size,
        );
        Ok(hash_bytes)
    }

    /// Import raw bytes into the blob store. Useful for metadata or small content.
    pub async fn import_bytes(&self, data: impl Into<bytes::Bytes>) -> Result<ContentHash> {
        let outcome = self.client.add_bytes(data.into()).await?;
        Ok(*outcome.hash.as_bytes())
    }

    /// Export a blob from the iroh store to a local file.
    pub async fn export_blob(&self, hash: &ContentHash, output_path: &Path) -> Result<()> {
        let iroh_hash = Hash::from_bytes(*hash);
        info!(
            "Exporting blob {} to {}",
            iroh_hash.fmt_short(),
            output_path.display(),
        );

        self.client
            .export(
                iroh_hash,
                output_path.to_path_buf(),
                ExportFormat::Blob,
                ExportMode::Copy,
            )
            .await?
            .finish()
            .await?;

        info!("Export complete: {}", output_path.display());
        Ok(())
    }

    /// Download a blob from a specific peer using the queued downloader.
    /// Requires relay or discovery to be configured.
    pub async fn download_from(&self, hash: &ContentHash, node: NodeAddr) -> Result<()> {
        self.download_impl(hash, node, DownloadMode::Queued).await
    }

    /// Download a blob from a specific peer using a direct connection.
    /// Does not require relay or discovery — connects directly to the peer.
    pub async fn download_direct(&self, hash: &ContentHash, node: NodeAddr) -> Result<()> {
        self.download_impl(hash, node, DownloadMode::Direct).await
    }

    async fn download_impl(
        &self,
        hash: &ContentHash,
        node: NodeAddr,
        mode: DownloadMode,
    ) -> Result<()> {
        let iroh_hash = Hash::from_bytes(*hash);
        info!(
            "Downloading blob {} from {}",
            iroh_hash.fmt_short(),
            node.node_id,
        );

        let progress = self
            .client
            .download_with_opts(
                iroh_hash,
                DownloadOptions {
                    format: BlobFormat::Raw,
                    nodes: vec![node],
                    tag: SetTagOption::Auto,
                    mode,
                },
            )
            .await?;
        let outcome = progress.finish().await?;

        info!(
            "Download complete: {} (downloaded {} bytes, {} bytes local)",
            iroh_hash.fmt_short(),
            outcome.downloaded_size,
            outcome.local_size,
        );
        Ok(())
    }

    /// Download a blob with progress reporting via a callback.
    /// The callback receives `(bytes_received, total_size)`.
    /// Falls back to the regular download if progress events aren't emitted.
    pub async fn download_with_progress<F>(
        &self,
        hash: &ContentHash,
        node: NodeAddr,
        mode: DownloadMode,
        on_progress: F,
    ) -> Result<()>
    where
        F: Fn(u64, u64) + Send + 'static,
    {
        let iroh_hash = Hash::from_bytes(*hash);
        info!(
            "Downloading blob {} from {} (with progress)",
            iroh_hash.fmt_short(),
            node.node_id,
        );

        let mut stream = self
            .client
            .download_with_opts(
                iroh_hash,
                DownloadOptions {
                    format: BlobFormat::Raw,
                    nodes: vec![node],
                    tag: SetTagOption::Auto,
                    mode,
                },
            )
            .await?;

        let mut total_size: u64 = 0;

        while let Some(event) = stream.next().await {
            use iroh_blobs::get::db::DownloadProgress;
            match event {
                Ok(DownloadProgress::Found { size, .. }) => {
                    total_size = size;
                    on_progress(0, total_size);
                }
                Ok(DownloadProgress::Progress { offset, .. }) => {
                    if total_size > 0 {
                        on_progress(offset, total_size);
                    }
                }
                Ok(DownloadProgress::AllDone(_stats)) => {
                    if total_size > 0 {
                        on_progress(total_size, total_size);
                    }
                    break;
                }
                Ok(DownloadProgress::Abort(err)) => {
                    return Err(anyhow::anyhow!("Download aborted: {}", err));
                }
                Err(e) => {
                    return Err(anyhow::anyhow!("Download stream error: {}", e));
                }
                _ => {}
            }
        }

        info!(
            "Download complete (with progress): {} ({} bytes)",
            iroh_hash.fmt_short(),
            total_size,
        );
        Ok(())
    }

    /// Check if a blob exists in the local store.
    pub async fn has_blob(&self, hash: &ContentHash) -> Result<bool> {
        let iroh_hash = Hash::from_bytes(*hash);
        Ok(self.client.has(iroh_hash).await?)
    }

    /// Read a blob's content as bytes (for small content like metadata).
    pub async fn read_blob(&self, hash: &ContentHash) -> Result<bytes::Bytes> {
        let iroh_hash = Hash::from_bytes(*hash);
        Ok(self.client.read_to_bytes(iroh_hash).await?)
    }

    /// Delete a blob from the local store.
    pub async fn delete_blob(&self, hash: &ContentHash) -> Result<()> {
        let iroh_hash = Hash::from_bytes(*hash);
        self.client.delete_blob(iroh_hash).await?;
        Ok(())
    }

    /// Get the underlying blobs client for advanced operations.
    pub fn client(&self) -> &BlobsClient {
        &self.client
    }
}
