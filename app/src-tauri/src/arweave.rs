//! Irys (Arweave bundler) client for permanent content storage.
//!
//! Irys accepts ETH payment and handles the AR token conversion internally.
//! Files uploaded via Irys are permanently stored on Arweave and retrievable
//! via `https://arweave.net/{tx_id}`.
//!
//! Upload flow (ephemeral key approach):
//! 1. Backend generates an ephemeral secp256k1 keypair (stored in DB)
//! 2. User sends ETH to the ephemeral address (funding for Irys upload)
//! 3. Ephemeral key sends ETH to Irys deposit address (funds the Irys balance)
//! 4. Backend reads file from iroh, creates ANS-104 data item, signs with ephemeral key
//! 5. Backend uploads signed data item to Irys node
//! 6. Returns the Arweave transaction ID

use alloy::network::{EthereumWallet, TransactionBuilder};
use alloy::primitives::{Address, U256};
use alloy::providers::{Provider, ProviderBuilder};
use alloy::rpc::types::TransactionRequest as AlloyTxRequest;
use alloy::signers::local::PrivateKeySigner;
use alloy::signers::Signer;
use anyhow::{Context, Result};
use serde::Deserialize;
use sha2::{Digest, Sha384};
use tracing::info;

/// Irys network configuration.
#[derive(Debug, Clone)]
pub struct IrysConfig {
    /// Irys node URL for uploads (e.g. "https://node2.irys.xyz")
    pub node_url: String,
    /// Arweave gateway for downloads (e.g. "https://arweave.net")
    pub gateway_url: String,
}

impl Default for IrysConfig {
    fn default() -> Self {
        Self {
            node_url: "https://node2.irys.xyz".to_string(),
            gateway_url: "https://arweave.net".to_string(),
        }
    }
}

/// Response from the Irys price API.
#[derive(Debug, Deserialize)]
struct IrysPriceResponse {
    /// Price in the smallest unit of the payment token (wei for ETH)
    #[serde(deserialize_with = "deserialize_string_or_number")]
    price: u64,
}

fn deserialize_string_or_number<'de, D>(deserializer: D) -> std::result::Result<u64, D::Error>
where
    D: serde::Deserializer<'de>,
{
    use serde::de;
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum StringOrNumber {
        String(String),
        Number(u64),
    }
    match StringOrNumber::deserialize(deserializer)? {
        StringOrNumber::String(s) => s.parse().map_err(de::Error::custom),
        StringOrNumber::Number(n) => Ok(n),
    }
}

/// Irys upload receipt returned after a successful upload.
#[derive(Debug, Deserialize)]
pub struct IrysReceipt {
    /// Arweave transaction ID — use to retrieve via gateway
    pub id: String,
}

/// Irys node info (from GET /info).
#[derive(Debug, Deserialize)]
struct IrysInfo {
    addresses: std::collections::HashMap<String, String>,
}

// ─── Cost Estimation ──────────────────────────────────────────────────────────

/// Estimate the cost (in wei) to permanently store `file_size_bytes` on Arweave via Irys.
///
/// Calls the Irys price API: `GET /price/{bytes}/ethereum`
pub async fn estimate_upload_cost(
    client: &reqwest::Client,
    config: &IrysConfig,
    file_size_bytes: u64,
) -> Result<u64> {
    let url = format!("{}/price/ethereum/{}", config.node_url, file_size_bytes);
    info!("Fetching Irys price estimate: {}", url);

    let resp = client
        .get(&url)
        .send()
        .await
        .context("Failed to reach Irys price API")?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        anyhow::bail!("Irys price API returned {}: {}", status, body);
    }

    // Irys may return just a number or a JSON object with "price" field
    let text = resp.text().await.context("Failed to read Irys price response")?;

    // Try parsing as plain number first
    if let Ok(price) = text.trim().parse::<u64>() {
        info!("Irys upload cost for {} bytes: {} wei", file_size_bytes, price);
        return Ok(price);
    }

    // Try parsing as JSON object
    let parsed: IrysPriceResponse =
        serde_json::from_str(&text).context("Failed to parse Irys price response")?;
    info!(
        "Irys upload cost for {} bytes: {} wei",
        file_size_bytes, parsed.price
    );
    Ok(parsed.price)
}

// ─── Download ─────────────────────────────────────────────────────────────────

/// Download a file from Arweave by transaction ID.
///
/// Returns the file bytes. Caller is responsible for saving to disk.
pub async fn download_from_arweave(
    client: &reqwest::Client,
    config: &IrysConfig,
    tx_id: &str,
) -> Result<Vec<u8>> {
    let url = format!("{}/{}", config.gateway_url, tx_id);
    info!("Downloading from Arweave: {}", url);

    let resp = client
        .get(&url)
        .send()
        .await
        .context("Failed to reach Arweave gateway")?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        anyhow::bail!("Arweave gateway returned {}: {}", status, body);
    }

    let bytes = resp
        .bytes()
        .await
        .context("Failed to read Arweave response body")?;
    info!("Downloaded {} bytes from Arweave tx {}", bytes.len(), tx_id);
    Ok(bytes.to_vec())
}

// ─── Ephemeral Key Management ─────────────────────────────────────────────────

/// Generate or load the ephemeral key used for Irys uploads.
/// The key is stored hex-encoded in the SQLite config table under "irys_upload_key".
pub fn load_or_generate_irys_key(db: &ara_core::storage::Database) -> Result<PrivateKeySigner> {
    if let Some(hex_key) = db.get_config("irys_upload_key") {
        let bytes = alloy::hex::decode(&hex_key).context("Invalid stored Irys key hex")?;
        let key_bytes: [u8; 32] = bytes
            .try_into()
            .map_err(|_| anyhow::anyhow!("Irys key must be 32 bytes"))?;
        let signer =
            PrivateKeySigner::from_bytes(&key_bytes.into()).context("Invalid stored Irys key")?;
        info!("Loaded Irys upload key: {}", signer.address());
        return Ok(signer);
    }

    // Generate a new key
    let signer = PrivateKeySigner::random();
    let key_hex = alloy::hex::encode(signer.credential().to_bytes());
    db.set_config("irys_upload_key", &key_hex)
        .context("Failed to store Irys upload key")?;
    info!("Generated new Irys upload key: {}", signer.address());
    Ok(signer)
}

/// Get the Irys deposit address for Ethereum.
pub async fn get_irys_deposit_address(
    client: &reqwest::Client,
    config: &IrysConfig,
) -> Result<String> {
    let url = format!("{}/info", config.node_url);
    let resp = client
        .get(&url)
        .send()
        .await
        .context("Failed to reach Irys info API")?;

    let info: IrysInfo = resp.json().await.context("Failed to parse Irys info")?;
    info.addresses
        .get("ethereum")
        .cloned()
        .ok_or_else(|| anyhow::anyhow!("Irys info missing Ethereum deposit address"))
}

/// Fund the ephemeral key's Irys balance by sending ETH from the ephemeral key
/// to the Irys deposit address.
pub async fn fund_irys_from_key(
    rpc_url: &str,
    signer: &PrivateKeySigner,
    irys_deposit_address: &str,
    amount_wei: u64,
) -> Result<String> {
    let deposit_addr: Address = irys_deposit_address
        .parse()
        .context("Invalid Irys deposit address")?;

    let wallet = EthereumWallet::from(signer.clone());
    let provider = ProviderBuilder::new()
        .wallet(wallet)
        .connect_http(rpc_url.parse().context("Invalid RPC URL")?);

    let tx = AlloyTxRequest::default()
        .with_to(deposit_addr)
        .with_value(U256::from(amount_wei));

    info!(
        "Funding Irys: {} wei from {} to {}",
        amount_wei,
        signer.address(),
        irys_deposit_address
    );

    let pending = provider
        .send_transaction(tx)
        .await
        .context("Failed to send Irys funding TX")?;
    let tx_hash = *pending.tx_hash();
    let receipt = pending
        .get_receipt()
        .await
        .context("Irys funding TX failed")?;

    if !receipt.status() {
        anyhow::bail!("Irys funding TX reverted: {tx_hash}");
    }

    info!("Irys funded with {} wei, TX: {}", amount_wei, tx_hash);
    Ok(format!("{tx_hash}"))
}

/// Notify Irys about a funding deposit so it credits the balance promptly.
///
/// Without this POST, Irys must auto-detect the deposit on-chain which can take
/// very long or never happen on devnet/testnets.
pub async fn notify_irys_of_deposit(
    client: &reqwest::Client,
    config: &IrysConfig,
    tx_hash: &str,
) -> Result<()> {
    let url = format!("{}/account/balance/ethereum", config.node_url);
    info!("Notifying Irys of funding deposit: {} -> {}", tx_hash, url);

    let resp = client
        .post(&url)
        .json(&serde_json::json!({ "tx_id": tx_hash }))
        .send()
        .await
        .context("Failed to notify Irys of deposit")?;

    if resp.status().is_success() {
        info!("Irys acknowledged deposit TX {}", tx_hash);
    } else {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        // Log but don't fail — Irys may still auto-detect the deposit
        info!("Irys deposit notification returned {} ({}), will poll balance", status, body);
    }
    Ok(())
}

/// Wait for Irys to credit the balance (poll the balance API).
///
/// The Irys balance API returns JSON: `{"balance":"12345"}`.
pub async fn wait_for_irys_balance(
    client: &reqwest::Client,
    config: &IrysConfig,
    address: &str,
    min_balance_wei: u64,
    max_attempts: u32,
) -> Result<()> {
    for attempt in 1..=max_attempts {
        let url = format!(
            "{}/account/balance/ethereum?address={}",
            config.node_url, address
        );
        if let Ok(resp) = client.get(&url).send().await {
            if let Ok(text) = resp.text().await {
                // Irys returns JSON: {"balance":"<number>"} — parse accordingly
                let balance = parse_irys_balance(&text);
                if balance >= min_balance_wei {
                    info!(
                        "Irys balance confirmed: {} wei (attempt {})",
                        balance, attempt
                    );
                    return Ok(());
                }
                info!("Irys balance: {} wei (need {}), attempt {}", balance, min_balance_wei, attempt);
            }
        }
        if attempt < max_attempts {
            tokio::time::sleep(std::time::Duration::from_secs(5)).await;
        }
    }
    anyhow::bail!(
        "Irys balance not credited after {} attempts (expected >= {} wei)",
        max_attempts,
        min_balance_wei
    )
}

/// Parse the Irys balance response, which can be either:
/// - JSON: `{"balance":"12345"}`
/// - Plain number: `12345`
fn parse_irys_balance(text: &str) -> u64 {
    let trimmed = text.trim();
    // Try plain number first
    if let Ok(n) = trimmed.parse::<u64>() {
        return n;
    }
    // Try JSON {"balance": "..."} or {"balance": ...}
    if let Ok(json) = serde_json::from_str::<serde_json::Value>(trimmed) {
        if let Some(b) = json.get("balance") {
            if let Some(s) = b.as_str() {
                return s.parse().unwrap_or(0);
            }
            if let Some(n) = b.as_u64() {
                return n;
            }
        }
    }
    0
}

// ─── ANS-104 Data Item ───────────────────────────────────────────────────────

/// ANS-104 signature type for Ethereum signers.
const SIG_TYPE_ETHEREUM: u16 = 3;

/// Create a signed ANS-104 data item for upload to Irys.
pub async fn create_signed_data_item(
    data: &[u8],
    content_type: &str,
    signer: &PrivateKeySigner,
) -> Result<Vec<u8>> {
    let tags = vec![("Content-Type", content_type)];

    // Get uncompressed public key (65 bytes: 0x04 || x || y)
    let verifying_key = signer.credential().verifying_key();
    let point = verifying_key.to_encoded_point(false);
    let owner = point.as_bytes().to_vec();
    assert_eq!(owner.len(), 65, "Uncompressed public key must be 65 bytes");

    // AVro-encode tags
    let avro_tags = avro_encode_tags(&tags);

    // Compute deep hash for signing
    let sig_type_str = SIG_TYPE_ETHEREUM.to_string();
    let empty: &[u8] = &[];
    let deep_hash = deep_hash_list(&[
        b"dataitem",
        b"1",
        sig_type_str.as_bytes(),
        &owner,
        empty,      // no target
        empty,      // no anchor
        &avro_tags,
        data,
    ]);

    // Sign with EIP-191 personal_sign
    let signature = signer
        .sign_message(&deep_hash)
        .await
        .context("Failed to sign data item")?;
    let sig_bytes = signature_to_65_bytes(&signature);

    // Assemble the data item binary
    let mut item = Vec::with_capacity(2 + 65 + 65 + 2 + 16 + avro_tags.len() + data.len());

    // Signature type (2 bytes LE)
    item.extend_from_slice(&SIG_TYPE_ETHEREUM.to_le_bytes());
    // Signature (65 bytes)
    item.extend_from_slice(&sig_bytes);
    // Owner (65 bytes)
    item.extend_from_slice(&owner);
    // Target present (0 = no target)
    item.push(0);
    // Anchor present (0 = no anchor)
    item.push(0);
    // Number of tags (8 bytes LE)
    item.extend_from_slice(&(tags.len() as u64).to_le_bytes());
    // Number of tag bytes (8 bytes LE)
    item.extend_from_slice(&(avro_tags.len() as u64).to_le_bytes());
    // Tag bytes
    item.extend_from_slice(&avro_tags);
    // Data
    item.extend_from_slice(data);

    Ok(item)
}

/// Upload a signed ANS-104 data item to Irys and return the Arweave TX ID.
pub async fn upload_data_item(
    client: &reqwest::Client,
    config: &IrysConfig,
    signed_item: &[u8],
) -> Result<String> {
    let url = format!("{}/tx/ethereum", config.node_url);
    info!("Uploading {} bytes to Irys: {}", signed_item.len(), url);

    let resp = client
        .post(&url)
        .header("Content-Type", "application/octet-stream")
        .body(signed_item.to_vec())
        .send()
        .await
        .context("Failed to upload to Irys")?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        anyhow::bail!("Irys upload failed ({}): {}", status, body);
    }

    let receipt: IrysReceipt = resp.json().await.context("Failed to parse Irys receipt")?;
    info!("Irys upload successful, TX ID: {}", receipt.id);
    Ok(receipt.id)
}

// ─── Deep Hash (SHA-384) ──────────────────────────────────────────────────────

/// Compute the deep hash of a single blob: SHA384(SHA384("blob" + len) + SHA384(data))
fn deep_hash_blob(data: &[u8]) -> Vec<u8> {
    let mut tag_hasher = Sha384::new();
    tag_hasher.update(b"blob");
    tag_hasher.update(data.len().to_string().as_bytes());
    let tag_hash = tag_hasher.finalize();

    let mut data_hasher = Sha384::new();
    data_hasher.update(data);
    let data_hash = data_hasher.finalize();

    let mut final_hasher = Sha384::new();
    final_hasher.update(&tag_hash);
    final_hasher.update(&data_hash);
    final_hasher.finalize().to_vec()
}

/// Compute the deep hash of a list of chunks.
/// Starting accumulator = SHA384("list" + count), then fold: acc = SHA384(acc + deepHash(item))
fn deep_hash_list(chunks: &[&[u8]]) -> Vec<u8> {
    let mut tag_hasher = Sha384::new();
    tag_hasher.update(b"list");
    tag_hasher.update(chunks.len().to_string().as_bytes());
    let mut acc = tag_hasher.finalize().to_vec();

    for chunk in chunks {
        let chunk_hash = deep_hash_blob(chunk);
        let mut pair_hasher = Sha384::new();
        pair_hasher.update(&acc);
        pair_hasher.update(&chunk_hash);
        acc = pair_hasher.finalize().to_vec();
    }

    acc
}

// ─── AVro Tag Encoding ────────────────────────────────────────────────────────

/// Encode tags in AVro binary format for ANS-104 data items.
fn avro_encode_tags(tags: &[(&str, &str)]) -> Vec<u8> {
    let mut buf = Vec::new();
    if tags.is_empty() {
        write_varint(&mut buf, 0); // empty array
        return buf;
    }

    // Block count (zigzag-encoded as positive)
    write_varint(&mut buf, zigzag_encode(tags.len() as i64));

    for (name, value) in tags {
        let name_bytes = name.as_bytes();
        let value_bytes = value.as_bytes();
        // bytes field: zigzag(length), raw bytes
        write_varint(&mut buf, zigzag_encode(name_bytes.len() as i64));
        buf.extend_from_slice(name_bytes);
        write_varint(&mut buf, zigzag_encode(value_bytes.len() as i64));
        buf.extend_from_slice(value_bytes);
    }

    // Terminate array with 0
    write_varint(&mut buf, 0);
    buf
}

/// Zigzag encode a signed 64-bit integer for AVro.
fn zigzag_encode(n: i64) -> u64 {
    ((n << 1) ^ (n >> 63)) as u64
}

/// Write a variable-length integer (varint) in AVro format.
fn write_varint(buf: &mut Vec<u8>, mut val: u64) {
    loop {
        if val < 0x80 {
            buf.push(val as u8);
            return;
        }
        buf.push((val as u8 & 0x7F) | 0x80);
        val >>= 7;
    }
}

// ─── Signature Helpers ────────────────────────────────────────────────────────

/// Convert an alloy Signature to 65 bytes (r || s || v).
fn signature_to_65_bytes(sig: &alloy::primitives::Signature) -> [u8; 65] {
    let mut bytes = [0u8; 65];
    bytes[..32].copy_from_slice(&sig.r().to_be_bytes::<32>());
    bytes[32..64].copy_from_slice(&sig.s().to_be_bytes::<32>());
    bytes[64] = sig.v() as u8;
    bytes
}

// ─── Utilities ────────────────────────────────────────────────────────────────

/// Build the calldata for funding an Irys upload.
///
/// This returns a `TransactionRequest` that sends ETH to the Irys node's
/// funding address. The user signs this via MetaMask.
///
/// After the funding tx confirms, the frontend calls `upload_to_irys` to
/// actually upload the file bytes.
pub fn build_irys_fund_tx(
    irys_funding_address: &str,
    cost_wei: u64,
) -> (String, String) {
    // Irys funding is a plain ETH transfer to their deposit address
    let value_hex = format!("0x{:x}", cost_wei);
    (irys_funding_address.to_string(), value_hex)
}

/// Format wei as a human-readable ETH string (e.g. "0.001234").
pub fn format_wei_as_eth(wei: u64) -> String {
    let eth = wei as f64 / 1e18;
    if eth < 0.000001 {
        format!("{:.10}", eth)
    } else if eth < 0.01 {
        format!("{:.6}", eth)
    } else {
        format!("{:.4}", eth)
    }
}
