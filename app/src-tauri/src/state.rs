use alloy::primitives::Address;
use ara_chain::{connect_http, AraChain, ContractAddresses};
use ara_core::config::AppConfig;
use ara_core::storage::Database;
use ara_p2p::node::IrohNode;
use std::sync::Arc;
use tokio::sync::Mutex;
use tracing::info;

use crate::gossip_actor::{self, GossipCmd};

/// Application state shared across all Tauri commands.
pub struct AppState {
    pub config: AppConfig,
    pub db: Arc<Mutex<Database>>,
    pub iroh_node: Arc<Mutex<Option<IrohNode>>>,
    pub wallet_address: Arc<Mutex<Option<String>>>,
    pub gossip_tx: Arc<Mutex<Option<tokio::sync::mpsc::Sender<GossipCmd>>>>,
}

impl AppState {
    pub fn new(config: AppConfig, db: Database) -> Self {
        Self {
            config,
            db: Arc::new(Mutex::new(db)),
            iroh_node: Arc::new(Mutex::new(None)),
            wallet_address: Arc::new(Mutex::new(None)),
            gossip_tx: Arc::new(Mutex::new(None)),
        }
    }

    /// Ensure the iroh P2P node is started (lazy init).
    /// Returns a MutexGuard with a guaranteed `Some(IrohNode)`.
    /// Callers should extract what they need (e.g. `blobs_client()`)
    /// and drop the guard before doing async work.
    pub async fn ensure_iroh(
        &self,
    ) -> Result<tokio::sync::MutexGuard<'_, Option<IrohNode>>, String> {
        let mut guard = self.iroh_node.lock().await;
        if guard.is_none() {
            let data_dir = std::path::Path::new(&self.config.iroh.data_dir);
            std::fs::create_dir_all(data_dir)
                .map_err(|e| format!("Failed to create iroh data dir: {e}"))?;
            info!("Starting iroh P2P node at {:?}...", data_dir);
            let node = IrohNode::start(data_dir)
                .await
                .map_err(|e| format!("Failed to start iroh node: {e}"))?;
            info!("Iroh P2P node started: {}", node.node_id());

            // Spawn gossip actor (if not already running)
            let mut gossip_guard = self.gossip_tx.lock().await;
            if gossip_guard.is_none() {
                let gossip = node.gossip().clone();
                let node_id = node.node_id();
                let tx = gossip_actor::spawn(gossip, node_id);
                *gossip_guard = Some(tx);
                info!("Gossip actor spawned");
            }
            drop(gossip_guard);

            *guard = Some(node);
        }
        Ok(guard)
    }

    /// Send a command to the gossip actor. No-op if iroh hasn't started yet.
    pub async fn send_gossip(&self, cmd: GossipCmd) -> Result<(), String> {
        let guard = self.gossip_tx.lock().await;
        if let Some(tx) = guard.as_ref() {
            tx.send(cmd)
                .await
                .map_err(|e| format!("Gossip actor send failed: {e}"))?;
        }
        Ok(())
    }

    /// Create an AraChain client from the current config.
    /// HTTP providers are stateless, so this is cheap to call per-request.
    pub fn chain_client(
        &self,
    ) -> Result<AraChain<impl alloy::providers::Provider + Clone>, String> {
        let eth = &self.config.ethereum;

        let addresses = ContractAddresses {
            ara_token: parse_address(&eth.ara_token_address, "ara_token")?,
            staking: parse_address(&eth.staking_address, "staking")?,
            registry: parse_address(&eth.registry_address, "registry")?,
            marketplace: parse_address(&eth.marketplace_address, "marketplace")?,
        };

        connect_http(&eth.rpc_url, addresses).map_err(|e| format!("Chain connect failed: {e}"))
    }
}

fn parse_address(s: &str, name: &str) -> Result<Address, String> {
    if s.is_empty() {
        // Return zero address for contracts not yet deployed
        return Ok(Address::ZERO);
    }
    s.parse::<Address>()
        .map_err(|e| format!("Invalid {name} address '{s}': {e}"))
}
