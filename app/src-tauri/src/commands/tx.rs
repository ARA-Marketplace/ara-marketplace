use crate::state::AppState;
use alloy::primitives::TxHash;
use alloy::providers::{Provider, ProviderBuilder};
use std::time::Duration;
use tauri::State;
use tokio::time::timeout;
use tracing::{info, warn};

/// All RPC endpoints to try (primary Alchemy + public fallbacks).
/// rpc.sepolia.org is excluded — it consistently times out.
const FALLBACK_RPCS: &[&str] = &[
    "https://sepolia.drpc.org",
    "https://1rpc.io/sepolia",
];

/// Per-RPC call timeout.
const RPC_TIMEOUT: Duration = Duration::from_secs(8);

/// Check all RPCs for a tx. Returns true if found on any of them.
async fn tx_exists_anywhere(hash: TxHash, rpc_urls: &[String]) -> bool {
    for rpc_url in rpc_urls {
        let Ok(parsed) = rpc_url.parse() else { continue };
        let provider = ProviderBuilder::new().connect_http(parsed);
        if let Ok(Ok(Some(_))) = timeout(RPC_TIMEOUT, provider.get_transaction_by_hash(hash)).await {
            return true;
        }
    }
    false
}

/// Poll for a transaction receipt, trying all RPCs each round.
/// Fails fast if the tx is not found in any mempool after 30 seconds
/// (indicates a dropped or never-broadcast transaction).
#[tauri::command]
pub async fn wait_for_transaction(
    state: State<'_, AppState>,
    tx_hash: String,
) -> Result<(), String> {
    let primary_rpc = state.config.ethereum.rpc_url.clone();
    let hash: TxHash = tx_hash
        .parse()
        .map_err(|e| format!("Invalid tx hash '{tx_hash}': {e}"))?;

    info!("Polling for receipt: {}", tx_hash);

    // Build full RPC list
    let mut rpc_urls: Vec<String> = vec![primary_rpc];
    for fb in FALLBACK_RPCS {
        rpc_urls.push(fb.to_string());
    }

    // Give the tx up to 30 seconds to appear in any mempool before giving up.
    // MetaMask submits to its own node (Infura); propagation to other nodes
    // can take a few seconds. After 30s if nobody has heard of it, it was dropped.
    let mut found_in_mempool = false;
    for check in 0..6 {
        if tx_exists_anywhere(hash, &rpc_urls).await {
            found_in_mempool = true;
            info!("Tx {} found in mempool (check {})", tx_hash, check + 1);
            break;
        }
        if check == 0 {
            warn!("Tx {} not yet visible on any RPC — waiting for propagation...", tx_hash);
        }
        tokio::time::sleep(Duration::from_secs(5)).await;
    }

    if !found_in_mempool {
        return Err(format!(
            "Transaction {tx_hash} was not found on any RPC after 30 seconds. \
             It was likely dropped (nonce conflict or gas issue). \
             In MetaMask: Settings → Advanced → Clear activity tab data, then retry."
        ));
    }

    // Poll every 3 seconds for up to 5 minutes (100 attempts)
    for attempt in 0..100 {
        for rpc_url in &rpc_urls {
            let Ok(parsed) = rpc_url.parse() else { continue };
            let provider = ProviderBuilder::new().connect_http(parsed);

            let result = timeout(RPC_TIMEOUT, provider.get_transaction_receipt(hash)).await;

            match result {
                Ok(Ok(Some(receipt))) if receipt.status() => {
                    info!(
                        "Transaction confirmed after {} polls via {}: {} (block {})",
                        attempt + 1,
                        rpc_url,
                        tx_hash,
                        receipt.block_number.unwrap_or_default()
                    );
                    return Ok(());
                }
                Ok(Ok(Some(_))) => {
                    return Err(format!("Transaction {tx_hash} was reverted on-chain"));
                }
                Ok(Ok(None)) => {
                    if attempt % 5 == 0 {
                        info!("Receipt not yet on {} (attempt {})", rpc_url, attempt + 1);
                    }
                }
                Ok(Err(e)) => {
                    warn!("Receipt poll error on {} (attempt {}): {}", rpc_url, attempt + 1, e);
                }
                Err(_) => {
                    warn!(
                        "Receipt poll timeout on {} (attempt {}): no response in {}s",
                        rpc_url, attempt + 1, RPC_TIMEOUT.as_secs()
                    );
                }
            }
        }

        tokio::time::sleep(Duration::from_secs(3)).await;
    }

    Err(format!(
        "Transaction {tx_hash} not confirmed after 5 minutes. \
         Check https://sepolia.etherscan.io/tx/{tx_hash}"
    ))
}
