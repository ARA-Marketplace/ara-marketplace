use serde::Serialize;
use std::collections::HashMap;
use tauri::State;

use ara_chain::names::NameRegistryClient;

use crate::state::AppState;
use super::types::TransactionRequest;

#[derive(Debug, Serialize)]
pub struct DisplayNameInfo {
    pub address: String,
    pub display_name: Option<String>,
}

#[tauri::command]
pub async fn register_name(
    state: State<'_, AppState>,
    name: String,
) -> Result<Vec<TransactionRequest>, String> {
    let chain = state.chain_client()?;
    let registry_addr = format!("0x{}", alloy::hex::encode(chain.name_registry_address()));

    let calldata = NameRegistryClient::<()>::register_name_calldata(&name);

    Ok(vec![TransactionRequest {
        to: registry_addr,
        data: format!("0x{}", alloy::hex::encode(&calldata)),
        value: "0x0".to_string(),
        description: format!("Register display name \"{}\"", name),
    }])
}

#[tauri::command]
pub async fn confirm_register_name(
    state: State<'_, AppState>,
    name: String,
) -> Result<(), String> {
    let wallet = state.wallet_address.lock().await;
    let address = wallet.as_deref().unwrap_or("").to_lowercase();
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64;
    let db = state.db.lock().await;
    db.upsert_name(&address, &name, now)
        .map_err(|e| format!("DB error: {e}"))?;
    Ok(())
}

#[tauri::command]
pub async fn remove_display_name(
    state: State<'_, AppState>,
) -> Result<Vec<TransactionRequest>, String> {
    let chain = state.chain_client()?;
    let registry_addr = format!("0x{}", alloy::hex::encode(chain.name_registry_address()));

    let calldata = NameRegistryClient::<()>::remove_name_calldata();

    Ok(vec![TransactionRequest {
        to: registry_addr,
        data: format!("0x{}", alloy::hex::encode(&calldata)),
        value: "0x0".to_string(),
        description: "Remove display name".to_string(),
    }])
}

#[tauri::command]
pub async fn confirm_remove_name(
    state: State<'_, AppState>,
) -> Result<(), String> {
    let wallet = state.wallet_address.lock().await;
    let address = wallet.as_deref().unwrap_or("").to_lowercase();
    let db = state.db.lock().await;
    db.remove_name(&address)
        .map_err(|e| format!("DB error: {e}"))?;
    Ok(())
}

#[tauri::command]
pub async fn get_display_name(
    state: State<'_, AppState>,
    address: String,
) -> Result<Option<String>, String> {
    let addr_lower = address.to_lowercase();

    // Try local cache first
    {
        let db = state.db.lock().await;
        if let Some(name) = db.get_name(&addr_lower) {
            return Ok(Some(name));
        }
    }

    // Fall back to on-chain lookup
    let chain = state.chain_client()?;
    let parsed: alloy::primitives::Address = address.parse()
        .map_err(|e| format!("Invalid address: {e}"))?;
    match chain.name_registry.get_name(parsed).await {
        Ok(name) if !name.is_empty() => {
            // Cache it
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs() as i64;
            let db = state.db.lock().await;
            let _ = db.upsert_name(&addr_lower, &name, now);
            Ok(Some(name))
        }
        _ => {
            // ENS fallback: try reverse resolution if no Ara name
            match ens_reverse_resolve(&state, &parsed).await {
                Some(ens_name) => {
                    let now = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_secs() as i64;
                    let db = state.db.lock().await;
                    let _ = db.upsert_name(&addr_lower, &ens_name, now);
                    Ok(Some(ens_name))
                }
                None => Ok(None),
            }
        }
    }
}

/// Attempt ENS reverse resolution for an address.
/// ENS lives on Ethereum mainnet — on Sepolia/testnets this will always return None.
/// On mainnet deployment, this queries the ENS reverse registrar via a separate
/// mainnet RPC endpoint configured in the app config.
async fn ens_reverse_resolve(
    state: &AppState,
    _address: &alloy::primitives::Address,
) -> Option<String> {
    // ENS reverse resolution requires a mainnet provider (ENS registry is on L1).
    // For the Sepolia deployment, this is a no-op. When moving to mainnet,
    // we'll add a mainnet_rpc_url config field and resolve ENS names via:
    //   1. Build reverse node: namehash(addr.reverse)
    //   2. Call ENS registry.resolver(node)
    //   3. Call resolver.name(node)
    let _ = state; // suppress unused warning
    None
}

#[tauri::command]
pub async fn get_display_names(
    state: State<'_, AppState>,
    addresses: Vec<String>,
) -> Result<HashMap<String, String>, String> {
    // 1. Try local cache first
    let mut result: HashMap<String, String>;
    {
        let db = state.db.lock().await;
        let addr_refs: Vec<&str> = addresses.iter().map(|s| s.as_str()).collect();
        result = db.get_names_batch(&addr_refs);
    }

    // 2. Find cache misses
    let missing: Vec<String> = addresses
        .iter()
        .filter(|a| !result.contains_key(&a.to_lowercase()))
        .cloned()
        .collect();

    if missing.is_empty() {
        return Ok(result);
    }

    // 3. Batch on-chain lookup for misses
    let chain = match state.chain_client() {
        Ok(c) => c,
        Err(_) => return Ok(result), // return partial results if chain unavailable
    };

    let parsed_addrs: Vec<alloy::primitives::Address> = missing
        .iter()
        .filter_map(|a| a.parse::<alloy::primitives::Address>().ok())
        .collect();

    if parsed_addrs.is_empty() {
        return Ok(result);
    }

    match chain.name_registry.get_names(parsed_addrs.clone()).await {
        Ok(names) => {
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs() as i64;
            let db = state.db.lock().await;
            for (addr, name) in parsed_addrs.iter().zip(names.iter()) {
                if !name.is_empty() {
                    let addr_lower = format!("{:#x}", addr).to_lowercase();
                    let _ = db.upsert_name(&addr_lower, name, now);
                    result.insert(addr_lower, name.clone());
                }
            }
        }
        Err(e) => {
            tracing::warn!("On-chain batch name lookup failed: {e}");
        }
    }

    Ok(result)
}

#[tauri::command]
pub async fn check_name_available(
    state: State<'_, AppState>,
    name: String,
) -> Result<bool, String> {
    let chain = state.chain_client()?;
    match chain.name_registry.get_address(&name).await {
        Ok(addr) => Ok(addr == alloy::primitives::Address::ZERO),
        Err(e) => Err(format!("Failed to check name availability: {e}")),
    }
}
