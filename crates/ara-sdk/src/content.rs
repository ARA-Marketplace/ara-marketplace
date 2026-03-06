use alloy::primitives::{Address, FixedBytes, U256};
use anyhow::Result;

use ara_chain::content_token::ContentTokenClient;

use crate::client::AraClient;
use crate::types::{
    format_token_amount, hex_encode, parse_amount, ContentInfo, PublishPrepareResult,
    TransactionRequest,
};

/// Content operations: publish, update, delist, query.
pub struct ContentOps<'a> {
    pub(crate) client: &'a AraClient,
}

impl ContentOps<'_> {
    /// Prepare a publish transaction. Returns calldata for signing.
    ///
    /// `payment_token`: Optional ERC-20 address. If `None`, content is priced in ETH.
    pub async fn prepare_publish(
        &self,
        content_hash: FixedBytes<32>,
        metadata_uri: String,
        price: &str,
        file_size: u64,
        max_supply: u64,
        royalty_bps: u32,
        payment_token: Option<Address>,
    ) -> Result<PublishPrepareResult> {
        // SECURITY: Reject oversized metadata_uri to prevent on-chain DoS
        const MAX_METADATA_LEN: usize = 100_000; // 100 KB
        if metadata_uri.len() > MAX_METADATA_LEN {
            anyhow::bail!("metadata_uri too large ({} bytes, max {} bytes)", metadata_uri.len(), MAX_METADATA_LEN);
        }

        let eth = &self.client.config.ethereum;
        let registry_addr: Address = eth.registry_address.parse()?;

        // Determine decimals for parsing
        let decimals = payment_token
            .and_then(|addr| {
                eth.supported_tokens
                    .iter()
                    .find(|t| t.address.eq_ignore_ascii_case(&format!("{addr:#x}")))
                    .map(|t| t.decimals)
            })
            .unwrap_or(18);
        let price_wei = parse_amount(price, decimals)?;

        // Check publisher eligibility if chain is available
        if let Some(ref signer) = self.client.signer {
            if let Ok(chain) = self.client.chain_client() {
                let eligible = chain
                    .staking
                    .is_eligible_publisher(signer.address())
                    .await?;
                if !eligible {
                    anyhow::bail!("Insufficient ARA stake to publish");
                }
            }
        }

        // Build calldata
        let calldata = if let Some(token_addr) = payment_token {
            ContentTokenClient::<()>::publish_content_with_token_calldata(
                content_hash,
                metadata_uri.clone(),
                price_wei,
                U256::from(file_size),
                U256::from(max_supply),
                royalty_bps as u128,
                token_addr,
            )
        } else {
            ContentTokenClient::<()>::publish_content_calldata(
                content_hash,
                metadata_uri.clone(),
                price_wei,
                U256::from(file_size),
                U256::from(max_supply),
                royalty_bps as u128,
            )
        };

        let price_unit = payment_token
            .and_then(|addr| {
                eth.supported_tokens
                    .iter()
                    .find(|t| t.address.eq_ignore_ascii_case(&format!("{addr:#x}")))
                    .map(|t| t.symbol.clone())
            })
            .unwrap_or_else(|| "ETH".to_string());

        Ok(PublishPrepareResult {
            content_hash: format!("0x{}", alloy::hex::encode(content_hash.as_slice())),
            metadata_uri,
            transactions: vec![TransactionRequest {
                to: format!("{registry_addr:#x}"),
                data: hex_encode(&calldata),
                value: "0x0".to_string(),
                description: format!("Publish content for {} {}", price, price_unit),
            }],
        })
    }

    /// Prepare an update content transaction (change price and/or metadata).
    pub fn prepare_update(
        &self,
        content_id: FixedBytes<32>,
        new_price_wei: U256,
        new_metadata_uri: String,
    ) -> Result<Vec<TransactionRequest>> {
        let eth = &self.client.config.ethereum;
        let registry_addr: Address = eth.registry_address.parse()?;

        let calldata = ContentTokenClient::<()>::update_content_calldata(
            content_id,
            new_price_wei,
            new_metadata_uri,
        );

        Ok(vec![TransactionRequest {
            to: format!("{registry_addr:#x}"),
            data: hex_encode(&calldata),
            value: "0x0".to_string(),
            description: "Update content metadata".to_string(),
        }])
    }

    /// Prepare a delist content transaction.
    pub fn prepare_delist(
        &self,
        content_id: FixedBytes<32>,
    ) -> Result<Vec<TransactionRequest>> {
        let eth = &self.client.config.ethereum;
        let registry_addr: Address = eth.registry_address.parse()?;

        let calldata = ContentTokenClient::<()>::delist_content_calldata(content_id);

        Ok(vec![TransactionRequest {
            to: format!("{registry_addr:#x}"),
            data: hex_encode(&calldata),
            value: "0x0".to_string(),
            description: "Delist content".to_string(),
        }])
    }

    /// Query content details from the local database.
    pub async fn get_content(&self, content_id: &str) -> Result<Option<ContentInfo>> {
        let db = self.client.db.lock().await;
        let conn = db.conn();

        let result = conn.query_row(
            "SELECT content_id, content_hash, creator, title, description,
                    content_type, price_wei, active, COALESCE(metadata_uri,''),
                    COALESCE(categories,'[]'), COALESCE(max_supply,0),
                    COALESCE(total_minted,0), COALESCE(payment_token,'')
             FROM content WHERE content_id = ?1",
            rusqlite::params![content_id],
            |row| {
                let price_wei_str: String = row.get(6)?;
                let cats_json: String = row.get(9)?;
                let pt: String = row.get(12)?;
                Ok((price_wei_str, cats_json, pt, row.get(0)?, row.get(1)?,
                    row.get(2)?, row.get::<_, Option<String>>(3)?.unwrap_or_default(),
                    row.get::<_, Option<String>>(4)?.unwrap_or_default(),
                    row.get::<_, Option<String>>(5)?.unwrap_or_default(),
                    row.get::<_, i32>(7)? != 0, row.get::<_, String>(8)?,
                    row.get::<_, i64>(10)?, row.get::<_, i64>(11)?))
            },
        );

        match result {
            Ok((price_wei_str, cats_json, pt, content_id, content_hash, creator,
                title, description, content_type, active, metadata_uri,
                max_supply, total_minted)) => {
                let price_wei: U256 = price_wei_str.parse().unwrap_or(U256::ZERO);
                let is_token = !pt.is_empty() && pt != "0x0000000000000000000000000000000000000000";
                let token_cfg = if is_token {
                    self.client.config.ethereum.supported_tokens.iter()
                        .find(|t| t.address.eq_ignore_ascii_case(&pt))
                } else {
                    None
                };
                let decimals = token_cfg.map(|t| t.decimals).unwrap_or(18);
                let symbol = token_cfg.map(|t| t.symbol.clone()).unwrap_or_else(|| "ETH".to_string());

                Ok(Some(ContentInfo {
                    content_id,
                    content_hash,
                    creator,
                    title,
                    description,
                    content_type,
                    price_wei: price_wei_str,
                    price_display: format_token_amount(price_wei, decimals),
                    price_unit: symbol,
                    active,
                    metadata_uri,
                    categories: serde_json::from_str(&cats_json).unwrap_or_default(),
                    max_supply,
                    total_minted,
                    payment_token: if is_token { Some(pt) } else { None },
                }))
            }
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    /// Search content in the local database.
    pub async fn search(&self, query: &str, limit: u32) -> Result<Vec<ContentInfo>> {
        let db = self.client.db.lock().await;
        let conn = db.conn();

        let pattern = if query.is_empty() {
            "%".to_string()
        } else {
            format!("%{query}%")
        };

        let mut stmt = conn.prepare(
            "SELECT content_id, content_hash, creator, title, description,
                    content_type, price_wei, active, COALESCE(metadata_uri,''),
                    COALESCE(categories,'[]'), COALESCE(max_supply,0),
                    COALESCE(total_minted,0), COALESCE(payment_token,'')
             FROM content WHERE active = 1
             AND (title LIKE ?1 OR description LIKE ?1 OR content_type LIKE ?1)
             ORDER BY created_at DESC LIMIT ?2",
        )?;

        let supported_tokens = &self.client.config.ethereum.supported_tokens;

        let rows = stmt.query_map(rusqlite::params![&pattern, limit], |row| {
            let price_wei_str: String = row.get(6)?;
            let cats_json: String = row.get(9)?;
            let pt: String = row.get(12)?;
            let price_wei: U256 = price_wei_str.parse().unwrap_or(U256::ZERO);
            let is_token = !pt.is_empty() && pt != "0x0000000000000000000000000000000000000000";
            let token_cfg = if is_token {
                supported_tokens.iter().find(|t| t.address.eq_ignore_ascii_case(&pt))
            } else {
                None
            };
            let decimals = token_cfg.map(|t| t.decimals).unwrap_or(18);
            let symbol = token_cfg.map(|t| t.symbol.clone()).unwrap_or_else(|| "ETH".to_string());

            Ok(ContentInfo {
                content_id: row.get(0)?,
                content_hash: row.get(1)?,
                creator: row.get(2)?,
                title: row.get::<_, Option<String>>(3)?.unwrap_or_default(),
                description: row.get::<_, Option<String>>(4)?.unwrap_or_default(),
                content_type: row.get::<_, Option<String>>(5)?.unwrap_or_default(),
                price_wei: price_wei_str,
                price_display: format_token_amount(price_wei, decimals),
                price_unit: symbol,
                active: row.get::<_, i32>(7)? != 0,
                metadata_uri: row.get(8)?,
                categories: serde_json::from_str(&cats_json).unwrap_or_default(),
                max_supply: row.get(10)?,
                total_minted: row.get(11)?,
                payment_token: if is_token { Some(pt) } else { None },
            })
        })?;

        Ok(rows.filter_map(|r| r.ok()).collect())
    }

    /// Query on-chain content details (price, creator, active status).
    pub async fn get_on_chain_info(
        &self,
        content_id: FixedBytes<32>,
    ) -> Result<(Address, U256, bool)> {
        let chain = self.client.chain_client()?;
        let creator = chain.registry.get_creator(content_id).await?;
        let price = chain.registry.get_price(content_id).await?;
        let active = chain.registry.is_active(content_id).await?;
        Ok((creator, price, active))
    }
}
