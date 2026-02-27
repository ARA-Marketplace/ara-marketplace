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
        _ => Ok(None),
    }
}

#[tauri::command]
pub async fn get_display_names(
    state: State<'_, AppState>,
    addresses: Vec<String>,
) -> Result<HashMap<String, String>, String> {
    let db = state.db.lock().await;
    let addr_refs: Vec<&str> = addresses.iter().map(|s| s.as_str()).collect();
    let result = db.get_names_batch(&addr_refs);
    Ok(result)
}
