use std::path::Path;
use std::sync::Arc;

use anyhow::Result;
use iroh::protocol::Router;
use iroh::Endpoint;
use iroh_blobs::net_protocol::Blobs;
use iroh_blobs::rpc::client::blobs::MemClient as BlobsClient;
use iroh_blobs::store::fs::Store as FsStore;
use iroh_blobs::store::mem::Store as MemStore;
use iroh_blobs::util::local_pool::LocalPool;
use iroh_gossip::net::Gossip;
use tracing::info;

/// Manages the iroh networking endpoint, blob storage, gossip, and protocol router.
/// This is the main entry point for all P2P operations.
pub struct IrohNode {
    endpoint: Endpoint,
    router: Router,
    blobs: Arc<Blobs<FsStore>>,
    gossip: Gossip,
    _local_pool: LocalPool,
}

/// In-memory variant for testing.
pub struct IrohNodeMem {
    endpoint: Endpoint,
    router: Router,
    blobs: Arc<Blobs<MemStore>>,
    gossip: Gossip,
    _local_pool: LocalPool,
}

impl IrohNode {
    /// Create and start a new iroh node with persistent storage.
    pub async fn start(data_dir: &Path) -> Result<Self> {
        info!("Starting iroh node with data dir: {}", data_dir.display());

        let endpoint = Endpoint::builder()
            .discovery_n0()
            .bind()
            .await?;

        let local_pool = LocalPool::default();

        let blobs = Blobs::persistent(data_dir.join("blobs"))
            .await?
            .build(local_pool.handle(), &endpoint);

        let addr = endpoint.node_addr().await?;
        let gossip = Gossip::from_endpoint(
            endpoint.clone(),
            Default::default(),
            &addr.info,
        );

        let router = Router::builder(endpoint.clone())
            .accept(iroh_blobs::ALPN, blobs.clone())
            .accept(iroh_gossip::ALPN, Arc::new(gossip.clone()))
            .spawn()
            .await?;

        let node_id = endpoint.node_id();
        info!("Iroh node started. NodeId: {}", node_id);

        Ok(Self {
            endpoint,
            router,
            blobs,
            gossip,
            _local_pool: local_pool,
        })
    }

    /// Get a blobs client for importing/exporting/downloading content.
    pub fn blobs_client(&self) -> BlobsClient {
        self.blobs.clone().client()
    }

    /// Get a reference to the gossip protocol handler.
    pub fn gossip(&self) -> &Gossip {
        &self.gossip
    }

    /// Get a reference to the iroh endpoint.
    pub fn endpoint(&self) -> &Endpoint {
        &self.endpoint
    }

    /// Get this node's public NodeId.
    pub fn node_id(&self) -> iroh::NodeId {
        self.endpoint.node_id()
    }

    /// Get this node's full address (NodeId + relay + direct addresses).
    pub async fn node_addr(&self) -> Result<iroh::NodeAddr> {
        Ok(self.endpoint.node_addr().await?)
    }

    /// Stop the iroh node gracefully.
    pub async fn shutdown(self) -> Result<()> {
        info!("Shutting down iroh node");
        self.router.shutdown().await?;
        Ok(())
    }
}

impl IrohNodeMem {
    /// Create an in-memory iroh node (for testing).
    /// No relay or discovery — tests should share addresses explicitly.
    pub async fn start() -> Result<Self> {
        let endpoint = Endpoint::builder()
            .relay_mode(iroh::RelayMode::Disabled)
            .bind()
            .await?;

        let local_pool = LocalPool::default();

        let blobs = Blobs::memory()
            .build(local_pool.handle(), &endpoint);

        let addr = endpoint.node_addr().await?;
        let gossip = Gossip::from_endpoint(
            endpoint.clone(),
            Default::default(),
            &addr.info,
        );

        let router = Router::builder(endpoint.clone())
            .accept(iroh_blobs::ALPN, blobs.clone())
            .accept(iroh_gossip::ALPN, Arc::new(gossip.clone()))
            .spawn()
            .await?;

        Ok(Self {
            endpoint,
            router,
            blobs,
            gossip,
            _local_pool: local_pool,
        })
    }

    pub fn blobs_client(&self) -> BlobsClient {
        self.blobs.clone().client()
    }

    pub fn gossip(&self) -> &Gossip {
        &self.gossip
    }

    pub fn endpoint(&self) -> &Endpoint {
        &self.endpoint
    }

    pub fn node_id(&self) -> iroh::NodeId {
        self.endpoint.node_id()
    }

    pub async fn node_addr(&self) -> Result<iroh::NodeAddr> {
        Ok(self.endpoint.node_addr().await?)
    }

    pub async fn shutdown(self) -> Result<()> {
        self.router.shutdown().await?;
        Ok(())
    }
}
