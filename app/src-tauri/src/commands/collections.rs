use serde::Serialize;
use tauri::State;
use alloy::primitives::U256;
use ara_chain::collections::CollectionsClient;

use crate::state::AppState;
use super::types::{TransactionRequest, format_wei};

#[derive(Debug, Serialize)]
pub struct CollectionInfo {
    pub collection_id: i64,
    pub creator: String,
    pub name: String,
    pub description: String,
    pub banner_uri: String,
    pub item_count: u32,
    pub volume_eth: String,
    pub created_at: i64,
}

#[derive(Debug, Serialize)]
pub struct CollectionDetail {
    pub info: CollectionInfo,
    pub items: Vec<super::content::ContentDetail>,
}

#[derive(Debug, Serialize)]
pub struct CollectionRanking {
    pub collection_id: i64,
    pub name: String,
    pub creator: String,
    pub banner_uri: String,
    pub floor_price_eth: String,
    pub item_count: u32,
    pub volume_eth: String,
}

#[tauri::command]
pub async fn create_collection(
    state: State<'_, AppState>,
    name: String,
    description: String,
    banner_uri: String,
) -> Result<Vec<TransactionRequest>, String> {
    let chain = state.chain_client()?;
    let collections_addr = format!("0x{}", alloy::hex::encode(chain.collections_address()));

    let calldata = CollectionsClient::<()>::create_collection_calldata(&name, &description, &banner_uri);

    Ok(vec![TransactionRequest {
        to: collections_addr,
        data: format!("0x{}", alloy::hex::encode(&calldata)),
        value: "0x0".to_string(),
        description: format!("Create collection \"{}\"", name),
    }])
}

#[tauri::command]
pub async fn confirm_create_collection(
    state: State<'_, AppState>,
    _tx_hash: String,
    name: String,
    description: String,
    banner_uri: String,
) -> Result<i64, String> {
    // For now, we sync collection data from on-chain events. Return a placeholder.
    // The event sync will pick up the CollectionCreated event and populate the DB.
    // As a quick path, we can also read the nextCollectionId - 1 from chain.
    let chain = state.chain_client()?;
    let next_id = chain.collections.next_collection_id().await
        .map_err(|e| format!("Failed to read collection ID: {e}"))?;
    let collection_id = next_id.saturating_sub(U256::from(1));
    let id_i64: i64 = collection_id.try_into().map_err(|_| "Collection ID overflow")?;

    // Cache in local DB
    let wallet = state.wallet_address.lock().await;
    let creator = wallet.as_deref().unwrap_or("").to_lowercase();
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64;

    let db = state.db.lock().await;
    db.upsert_collection(id_i64, &creator, &name, &description, &banner_uri, now, true)
        .map_err(|e| format!("DB error: {e}"))?;

    Ok(id_i64)
}

#[tauri::command]
pub async fn update_collection(
    state: State<'_, AppState>,
    collection_id: i64,
    name: String,
    description: String,
    banner_uri: String,
) -> Result<Vec<TransactionRequest>, String> {
    let chain = state.chain_client()?;
    let collections_addr = format!("0x{}", alloy::hex::encode(chain.collections_address()));

    let calldata = CollectionsClient::<()>::update_collection_calldata(
        U256::from(collection_id as u64), &name, &description, &banner_uri,
    );

    Ok(vec![TransactionRequest {
        to: collections_addr,
        data: format!("0x{}", alloy::hex::encode(&calldata)),
        value: "0x0".to_string(),
        description: format!("Update collection \"{}\"", name),
    }])
}

#[tauri::command]
pub async fn confirm_update_collection(
    state: State<'_, AppState>,
    collection_id: i64,
    name: String,
    description: String,
    banner_uri: String,
) -> Result<(), String> {
    let db = state.db.lock().await;
    let wallet = state.wallet_address.lock().await;
    let creator = wallet.as_deref().unwrap_or("").to_lowercase();
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64;
    db.upsert_collection(collection_id, &creator, &name, &description, &banner_uri, now, true)
        .map_err(|e| format!("DB error: {e}"))?;
    Ok(())
}

#[tauri::command]
pub async fn delete_collection(
    state: State<'_, AppState>,
    collection_id: i64,
) -> Result<Vec<TransactionRequest>, String> {
    let chain = state.chain_client()?;
    let collections_addr = format!("0x{}", alloy::hex::encode(chain.collections_address()));

    let calldata = CollectionsClient::<()>::delete_collection_calldata(U256::from(collection_id as u64));

    Ok(vec![TransactionRequest {
        to: collections_addr,
        data: format!("0x{}", alloy::hex::encode(&calldata)),
        value: "0x0".to_string(),
        description: "Delete collection".to_string(),
    }])
}

#[tauri::command]
pub async fn confirm_delete_collection(
    state: State<'_, AppState>,
    collection_id: i64,
) -> Result<(), String> {
    let db = state.db.lock().await;
    db.delete_collection_items(collection_id).map_err(|e| format!("DB error: {e}"))?;
    // Mark as inactive
    let _ = db.conn().execute(
        "UPDATE collections SET active = 0 WHERE collection_id = ?1",
        rusqlite::params![collection_id],
    );
    Ok(())
}

#[tauri::command]
pub async fn add_to_collection(
    state: State<'_, AppState>,
    collection_id: i64,
    content_id: String,
) -> Result<Vec<TransactionRequest>, String> {
    let chain = state.chain_client()?;
    let collections_addr = format!("0x{}", alloy::hex::encode(chain.collections_address()));

    let content_id_hex = content_id.strip_prefix("0x").unwrap_or(&content_id);
    let bytes = alloy::hex::decode(content_id_hex).map_err(|e| format!("Invalid content_id: {e}"))?;
    let mut fixed = [0u8; 32];
    fixed.copy_from_slice(&bytes);

    let calldata = CollectionsClient::<()>::add_item_calldata(
        U256::from(collection_id as u64),
        alloy::primitives::FixedBytes(fixed),
    );

    Ok(vec![TransactionRequest {
        to: collections_addr,
        data: format!("0x{}", alloy::hex::encode(&calldata)),
        value: "0x0".to_string(),
        description: "Add item to collection".to_string(),
    }])
}

#[tauri::command]
pub async fn confirm_add_to_collection(
    state: State<'_, AppState>,
    collection_id: i64,
    content_id: String,
) -> Result<(), String> {
    let db = state.db.lock().await;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64;
    db.upsert_collection_item(collection_id, &content_id, now)
        .map_err(|e| format!("DB error: {e}"))?;
    Ok(())
}

#[tauri::command]
pub async fn remove_from_collection(
    state: State<'_, AppState>,
    collection_id: i64,
    content_id: String,
) -> Result<Vec<TransactionRequest>, String> {
    let chain = state.chain_client()?;
    let collections_addr = format!("0x{}", alloy::hex::encode(chain.collections_address()));

    let content_id_hex = content_id.strip_prefix("0x").unwrap_or(&content_id);
    let bytes = alloy::hex::decode(content_id_hex).map_err(|e| format!("Invalid content_id: {e}"))?;
    let mut fixed = [0u8; 32];
    fixed.copy_from_slice(&bytes);

    let calldata = CollectionsClient::<()>::remove_item_calldata(
        U256::from(collection_id as u64),
        alloy::primitives::FixedBytes(fixed),
    );

    Ok(vec![TransactionRequest {
        to: collections_addr,
        data: format!("0x{}", alloy::hex::encode(&calldata)),
        value: "0x0".to_string(),
        description: "Remove item from collection".to_string(),
    }])
}

#[tauri::command]
pub async fn confirm_remove_from_collection(
    state: State<'_, AppState>,
    collection_id: i64,
    content_id: String,
) -> Result<(), String> {
    let db = state.db.lock().await;
    db.remove_collection_item(collection_id, &content_id)
        .map_err(|e| format!("DB error: {e}"))?;
    Ok(())
}

#[tauri::command]
pub async fn get_my_collections(
    state: State<'_, AppState>,
) -> Result<Vec<CollectionInfo>, String> {
    let wallet = state.wallet_address.lock().await;
    let creator = wallet.as_deref().unwrap_or("").to_lowercase();
    let db = state.db.lock().await;

    let rows = db.get_collections_by_creator(&creator)
        .map_err(|e| format!("DB error: {e}"))?;

    let mut result = Vec::new();
    for (id, name, desc, banner, created_at, _active, item_count) in rows {
        let volume = db.get_collection_volume(id).unwrap_or_else(|_| "0".to_string());
        let volume_wei: U256 = volume.parse().unwrap_or(U256::ZERO);
        result.push(CollectionInfo {
            collection_id: id,
            creator: creator.clone(),
            name,
            description: desc,
            banner_uri: banner,
            item_count,
            volume_eth: format_wei(volume_wei),
            created_at,
        });
    }
    Ok(result)
}

#[tauri::command]
pub async fn get_collection(
    state: State<'_, AppState>,
    collection_id: i64,
) -> Result<CollectionInfo, String> {
    let db = state.db.lock().await;
    let row = db.conn().query_row(
        "SELECT creator, name, description, banner_uri, created_at, active FROM collections WHERE collection_id = ?1",
        rusqlite::params![collection_id],
        |row| Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
            row.get::<_, String>(3)?,
            row.get::<_, i64>(4)?,
            row.get::<_, i32>(5)? != 0,
        )),
    ).map_err(|e| format!("Collection not found: {e}"))?;

    let item_count: u32 = db.conn().query_row(
        "SELECT COUNT(*) FROM collection_items WHERE collection_id = ?1",
        rusqlite::params![collection_id],
        |row| row.get(0),
    ).unwrap_or(0);

    let volume = db.get_collection_volume(collection_id).unwrap_or_else(|_| "0".to_string());
    let volume_wei: U256 = volume.parse().unwrap_or(U256::ZERO);

    Ok(CollectionInfo {
        collection_id,
        creator: row.0,
        name: row.1,
        description: row.2,
        banner_uri: row.3,
        item_count,
        volume_eth: format_wei(volume_wei),
        created_at: row.4,
    })
}

#[tauri::command]
pub async fn get_collection_items(
    state: State<'_, AppState>,
    collection_id: i64,
) -> Result<Vec<String>, String> {
    let db = state.db.lock().await;
    db.get_collection_item_ids(collection_id)
        .map_err(|e| format!("DB error: {e}"))
}

#[tauri::command]
pub async fn get_all_collections(
    state: State<'_, AppState>,
    limit: Option<u32>,
    offset: Option<u32>,
) -> Result<Vec<CollectionInfo>, String> {
    let db = state.db.lock().await;
    let rows = db.get_all_collections(limit.unwrap_or(50), offset.unwrap_or(0))
        .map_err(|e| format!("DB error: {e}"))?;

    let mut result = Vec::new();
    for (id, creator, name, desc, banner, created_at, _active, item_count) in rows {
        let volume = db.get_collection_volume(id).unwrap_or_else(|_| "0".to_string());
        let volume_wei: U256 = volume.parse().unwrap_or(U256::ZERO);
        result.push(CollectionInfo {
            collection_id: id,
            creator,
            name,
            description: desc,
            banner_uri: banner,
            item_count,
            volume_eth: format_wei(volume_wei),
            created_at,
        });
    }
    Ok(result)
}

#[tauri::command]
pub async fn get_content_collection(
    state: State<'_, AppState>,
    content_id: String,
) -> Result<Option<i64>, String> {
    let db = state.db.lock().await;
    db.get_content_collection_id(&content_id)
        .map_err(|e| format!("DB error: {e}"))
}

#[tauri::command]
pub async fn get_top_collections(
    state: State<'_, AppState>,
    limit: Option<u32>,
) -> Result<Vec<CollectionRanking>, String> {
    let db = state.db.lock().await;
    let rows = db.get_top_collections(limit.unwrap_or(10))
        .map_err(|e| format!("DB error: {e}"))?;

    Ok(rows.into_iter().map(|(id, name, creator, banner, floor, count, vol)| {
        let floor_wei: U256 = floor.parse().unwrap_or(U256::ZERO);
        let vol_wei: U256 = vol.parse().unwrap_or(U256::ZERO);
        CollectionRanking {
            collection_id: id,
            name,
            creator,
            banner_uri: banner,
            floor_price_eth: format_wei(floor_wei),
            item_count: count,
            volume_eth: format_wei(vol_wei),
        }
    }).collect())
}
