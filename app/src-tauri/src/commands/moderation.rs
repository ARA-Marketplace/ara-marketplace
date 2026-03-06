use crate::commands::types::TransactionRequest;
use crate::state::AppState;
use alloy::primitives::FixedBytes;
use ara_chain::moderation::ModerationClient;
use serde::{Deserialize, Serialize};
use tauri::State;
use tracing::info;

#[derive(Debug, Serialize, Deserialize)]
pub struct FlagProposalInfo {
    pub content_id: String,
    pub flagger: String,
    pub reason: u8,
    pub is_emergency: bool,
    pub flag_count: u64,
    pub voting_deadline: u64,
    pub uphold_weight: String,
    pub dismiss_weight: String,
    pub status: u8,
    pub appealed: bool,
    pub is_nsfw: bool,
}

// ─── NSFW tagging ───────────────────────────────────────────────────────

/// Creator self-tags their content as NSFW.
#[tauri::command]
pub async fn set_nsfw(
    state: State<'_, AppState>,
    content_id: String,
    is_nsfw: bool,
) -> Result<Vec<TransactionRequest>, String> {
    info!("Setting NSFW={} for content {}", is_nsfw, content_id);

    let wallet = state.wallet_address.lock().await;
    wallet.as_ref().ok_or("No wallet connected")?;
    drop(wallet);

    let content_id_bytes: FixedBytes<32> = content_id
        .parse()
        .map_err(|e| format!("Invalid content ID: {e}"))?;

    let moderation_addr = &state.config.ethereum.moderation_address;
    if moderation_addr.is_empty() {
        // Store locally in DB only
        let db = state.db.lock().await;
        db.conn()
            .execute(
                "UPDATE content SET is_nsfw = ?1 WHERE content_id = ?2",
                rusqlite::params![is_nsfw as i32, &content_id],
            )
            .map_err(|e| format!("DB update failed: {e}"))?;
        return Ok(vec![]);
    }

    let calldata = ModerationClient::<()>::set_nsfw_calldata(content_id_bytes, is_nsfw);

    Ok(vec![TransactionRequest {
        to: moderation_addr.clone(),
        data: format!("0x{}", alloy::hex::encode(&calldata)),
        value: "0x0".to_string(),
        description: if is_nsfw {
            "Tag content as NSFW".to_string()
        } else {
            "Remove NSFW tag".to_string()
        },
    }])
}

/// Confirm NSFW tag was set on-chain, update local DB.
#[tauri::command]
pub async fn confirm_set_nsfw(
    state: State<'_, AppState>,
    content_id: String,
    is_nsfw: bool,
) -> Result<(), String> {
    // SECURITY: Require wallet connection to prevent unauthorized local state changes
    let wallet = state.wallet_address.lock().await;
    wallet.as_ref().ok_or("No wallet connected")?;
    drop(wallet);

    let db = state.db.lock().await;
    db.conn()
        .execute(
            "UPDATE content SET is_nsfw = ?1 WHERE content_id = ?2",
            rusqlite::params![is_nsfw as i32, &content_id],
        )
        .map_err(|e| format!("DB update failed: {e}"))?;
    Ok(())
}

// ─── Content flagging ───────────────────────────────────────────────────

/// Flag content for moderation review.
#[tauri::command]
pub async fn flag_content(
    state: State<'_, AppState>,
    content_id: String,
    reason: u8,
    is_emergency: bool,
) -> Result<Vec<TransactionRequest>, String> {
    info!(
        "Flagging content {} (reason={}, emergency={})",
        content_id, reason, is_emergency
    );

    let wallet = state.wallet_address.lock().await;
    wallet.as_ref().ok_or("No wallet connected")?;
    drop(wallet);

    let moderation_addr = &state.config.ethereum.moderation_address;
    if moderation_addr.is_empty() {
        return Err("Moderation contract not deployed".to_string());
    }

    let content_id_bytes: FixedBytes<32> = content_id
        .parse()
        .map_err(|e| format!("Invalid content ID: {e}"))?;

    let calldata = ModerationClient::<()>::flag_content_calldata(content_id_bytes, reason, is_emergency);

    Ok(vec![TransactionRequest {
        to: moderation_addr.clone(),
        data: format!("0x{}", alloy::hex::encode(&calldata)),
        value: "0x0".to_string(),
        description: if is_emergency {
            "Emergency flag content for immediate review".to_string()
        } else {
            "Flag content for moderation review".to_string()
        },
    }])
}

/// Vote on an active flag proposal.
#[tauri::command]
pub async fn vote_on_flag(
    state: State<'_, AppState>,
    content_id: String,
    uphold: bool,
) -> Result<Vec<TransactionRequest>, String> {
    info!("Voting {} on flag for {}", if uphold { "uphold" } else { "dismiss" }, content_id);

    let wallet = state.wallet_address.lock().await;
    wallet.as_ref().ok_or("No wallet connected")?;
    drop(wallet);

    let moderation_addr = &state.config.ethereum.moderation_address;
    if moderation_addr.is_empty() {
        return Err("Moderation contract not deployed".to_string());
    }

    let content_id_bytes: FixedBytes<32> = content_id
        .parse()
        .map_err(|e| format!("Invalid content ID: {e}"))?;

    let calldata = ModerationClient::<()>::vote_calldata(content_id_bytes, uphold);

    Ok(vec![TransactionRequest {
        to: moderation_addr.clone(),
        data: format!("0x{}", alloy::hex::encode(&calldata)),
        value: "0x0".to_string(),
        description: format!(
            "{} flag for content",
            if uphold { "Uphold" } else { "Dismiss" }
        ),
    }])
}

/// Resolve a flag proposal after voting period ends.
#[tauri::command]
pub async fn resolve_flag(
    state: State<'_, AppState>,
    content_id: String,
) -> Result<Vec<TransactionRequest>, String> {
    info!("Resolving flag for {}", content_id);

    let moderation_addr = &state.config.ethereum.moderation_address;
    if moderation_addr.is_empty() {
        return Err("Moderation contract not deployed".to_string());
    }

    let content_id_bytes: FixedBytes<32> = content_id
        .parse()
        .map_err(|e| format!("Invalid content ID: {e}"))?;

    let calldata = ModerationClient::<()>::resolve_flag_calldata(content_id_bytes);

    Ok(vec![TransactionRequest {
        to: moderation_addr.clone(),
        data: format!("0x{}", alloy::hex::encode(&calldata)),
        value: "0x0".to_string(),
        description: "Resolve moderation flag".to_string(),
    }])
}

/// Creator appeals an active flag.
#[tauri::command]
pub async fn appeal_flag(
    state: State<'_, AppState>,
    content_id: String,
) -> Result<Vec<TransactionRequest>, String> {
    info!("Appealing flag for {}", content_id);

    let wallet = state.wallet_address.lock().await;
    wallet.as_ref().ok_or("No wallet connected")?;
    drop(wallet);

    let moderation_addr = &state.config.ethereum.moderation_address;
    if moderation_addr.is_empty() {
        return Err("Moderation contract not deployed".to_string());
    }

    let content_id_bytes: FixedBytes<32> = content_id
        .parse()
        .map_err(|e| format!("Invalid content ID: {e}"))?;

    let calldata = ModerationClient::<()>::appeal_calldata(content_id_bytes);

    Ok(vec![TransactionRequest {
        to: moderation_addr.clone(),
        data: format!("0x{}", alloy::hex::encode(&calldata)),
        value: "0x0".to_string(),
        description: "Appeal content flag".to_string(),
    }])
}

/// Get moderation status for a content item.
#[tauri::command]
pub async fn get_moderation_status(
    state: State<'_, AppState>,
    content_id: String,
) -> Result<FlagProposalInfo, String> {
    let db = state.db.lock().await;
    let conn = db.conn();

    let is_nsfw: bool = conn
        .query_row(
            "SELECT is_nsfw FROM content WHERE content_id = ?1",
            rusqlite::params![&content_id],
            |row| row.get::<_, i32>(0),
        )
        .map(|v| v != 0)
        .unwrap_or(false);

    // Check on-chain status if moderation contract is deployed
    let moderation_addr = &state.config.ethereum.moderation_address;
    if !moderation_addr.is_empty() {
        if let Ok(chain) = state.chain_client() {
            let content_id_bytes: FixedBytes<32> = content_id
                .parse()
                .map_err(|e| format!("Invalid content ID: {e}"))?;

            let status = chain
                .moderation
                .get_proposal_status(content_id_bytes)
                .await
                .unwrap_or(0);

            return Ok(FlagProposalInfo {
                content_id,
                flagger: String::new(),
                reason: 0,
                is_emergency: false,
                flag_count: 0,
                voting_deadline: 0,
                uphold_weight: "0".to_string(),
                dismiss_weight: "0".to_string(),
                status,
                appealed: false,
                is_nsfw,
            });
        }
    }

    Ok(FlagProposalInfo {
        content_id,
        flagger: String::new(),
        reason: 0,
        is_emergency: false,
        flag_count: 0,
        voting_deadline: 0,
        uphold_weight: "0".to_string(),
        dismiss_weight: "0".to_string(),
        status: 0,
        appealed: false,
        is_nsfw,
    })
}
