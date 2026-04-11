use alloy::primitives::{Address, FixedBytes, U256};
use anyhow::Result;

use ara_chain::marketplace::MarketplaceClient;
use ara_chain::token::TokenClient;

use crate::client::AraClient;
use crate::types::{
    format_token_amount, format_wei, hex_encode, PurchasePrepareResult, TransactionRequest,
};

/// Marketplace operations: purchase, resale, claim delivery rewards.
pub struct MarketplaceOps<'a> {
    pub(crate) client: &'a AraClient,
}

impl MarketplaceOps<'_> {
    /// Prepare a purchase transaction for ETH-priced content.
    pub async fn prepare_purchase(
        &self,
        content_id: &str,
    ) -> Result<PurchasePrepareResult> {
        let eth = &self.client.config.ethereum;
        let marketplace_addr: Address = eth.marketplace_address.parse()?;

        // Look up from DB
        let db = self.client.db.lock().await;
        let (title, price_wei_str, payment_token_str) = db.conn().query_row(
            "SELECT COALESCE(title,''), price_wei, COALESCE(payment_token,'')
             FROM content WHERE content_id = ?1 AND active = 1",
            rusqlite::params![content_id],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?, row.get::<_, String>(2)?)),
        )?;
        drop(db);

        let price_wei: U256 = price_wei_str.parse()?;
        let content_id_bytes: FixedBytes<32> = content_id
            .strip_prefix("0x")
            .unwrap_or(content_id)
            .parse()?;

        let is_token = !payment_token_str.is_empty()
            && payment_token_str != "0x0000000000000000000000000000000000000000";

        let (price_display, price_unit, transactions) = if is_token {
            let token_addr: Address = payment_token_str.parse()?;
            let token_cfg = eth.supported_tokens.iter()
                .find(|t| t.address.eq_ignore_ascii_case(&payment_token_str));
            let decimals = token_cfg.map(|t| t.decimals).unwrap_or(18);
            let symbol = token_cfg.map(|t| t.symbol.as_str()).unwrap_or("TOKEN");
            let display = format_token_amount(price_wei, decimals);

            let approve = TokenClient::<()>::approve_calldata(marketplace_addr, price_wei);
            let purchase = MarketplaceClient::<()>::purchase_with_token_calldata(
                content_id_bytes, token_addr, price_wei, price_wei,
            );

            (display.clone(), symbol.to_string(), vec![
                TransactionRequest {
                    to: format!("{token_addr:#x}"),
                    data: hex_encode(&approve),
                    value: "0x0".to_string(),
                    description: format!("Approve {} {} for marketplace", display, symbol),
                },
                TransactionRequest {
                    to: format!("{marketplace_addr:#x}"),
                    data: hex_encode(&purchase),
                    value: "0x0".to_string(),
                    description: format!("Purchase \"{}\" for {} {}", title, display, symbol),
                },
            ])
        } else {
            let display = format_wei(price_wei);
            let calldata = MarketplaceClient::<()>::purchase_calldata(content_id_bytes, price_wei);

            (display.clone(), "ETH".to_string(), vec![TransactionRequest {
                to: format!("{marketplace_addr:#x}"),
                data: hex_encode(&calldata),
                value: format!("0x{:x}", price_wei),
                description: format!("Purchase \"{}\" for {} ETH", title, display),
            }])
        };

        Ok(PurchasePrepareResult {
            content_id: content_id.to_string(),
            title,
            price_display,
            price_unit,
            transactions,
        })
    }

    /// Check on-chain if a buyer has purchased specific content.
    pub async fn has_purchased(
        &self,
        content_id: FixedBytes<32>,
        buyer: Address,
    ) -> Result<bool> {
        let chain = self.client.chain_client()?;
        chain.marketplace.has_purchased(content_id, buyer).await
    }

    /// Prepare a list-for-resale transaction.
    pub fn prepare_list_for_resale(
        &self,
        content_id: FixedBytes<32>,
        price_wei: U256,
    ) -> Result<Vec<TransactionRequest>> {
        let eth = &self.client.config.ethereum;
        let marketplace_addr: Address = eth.marketplace_address.parse()?;
        let registry_addr: Address = eth.registry_address.parse()?;

        // Approve marketplace to transfer the NFT + list it
        let approve = ara_chain::content_token::ContentTokenClient::<()>::set_approval_for_all_calldata(
            marketplace_addr, true,
        );
        let list = MarketplaceClient::<()>::list_for_resale_calldata(content_id, price_wei);

        Ok(vec![
            TransactionRequest {
                to: format!("{registry_addr:#x}"),
                data: hex_encode(&approve),
                value: "0x0".to_string(),
                description: "Approve marketplace for NFT transfer".to_string(),
            },
            TransactionRequest {
                to: format!("{marketplace_addr:#x}"),
                data: hex_encode(&list),
                value: "0x0".to_string(),
                description: format!("List for resale at {} ETH", format_wei(price_wei)),
            },
        ])
    }

    /// Prepare a cancel-listing transaction.
    pub fn prepare_cancel_listing(
        &self,
        content_id: FixedBytes<32>,
    ) -> Result<Vec<TransactionRequest>> {
        let eth = &self.client.config.ethereum;
        let marketplace_addr: Address = eth.marketplace_address.parse()?;
        let calldata = MarketplaceClient::<()>::cancel_listing_calldata(content_id);

        Ok(vec![TransactionRequest {
            to: format!("{marketplace_addr:#x}"),
            data: hex_encode(&calldata),
            value: "0x0".to_string(),
            description: "Cancel resale listing".to_string(),
        }])
    }

    /// Prepare a buy-resale transaction.
    pub fn prepare_buy_resale(
        &self,
        content_id: FixedBytes<32>,
        seller: Address,
        price_wei: U256,
    ) -> Result<Vec<TransactionRequest>> {
        let eth = &self.client.config.ethereum;
        let marketplace_addr: Address = eth.marketplace_address.parse()?;
        let calldata = MarketplaceClient::<()>::buy_resale_calldata(content_id, seller, price_wei);

        Ok(vec![TransactionRequest {
            to: format!("{marketplace_addr:#x}"),
            data: hex_encode(&calldata),
            value: format!("0x{:x}", price_wei),
            description: format!("Buy resale for {} ETH", format_wei(price_wei)),
        }])
    }

    /// Get unclaimed delivery reward for a content/buyer pair.
    pub async fn get_buyer_reward(
        &self,
        content_id: FixedBytes<32>,
        buyer: Address,
    ) -> Result<U256> {
        let chain = self.client.chain_client()?;
        chain.marketplace.get_buyer_reward(content_id, buyer).await
    }

    /// Confirm a purchase: insert into local DB.
    pub async fn confirm_purchase(
        &self,
        content_id: &str,
        buyer: &str,
        price_paid_wei: &str,
        tx_hash: &str,
    ) -> Result<()> {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs() as i64;

        let db = self.client.db.lock().await;
        db.upsert_purchase(content_id, buyer, price_paid_wei, tx_hash, now)?;
        let _ = db.increment_total_minted(content_id);
        Ok(())
    }

    /// Confirm listing for resale: insert into local DB.
    pub async fn confirm_list_for_resale(
        &self,
        content_id: &str,
        seller: &str,
        price_wei: &str,
    ) -> Result<()> {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs() as i64;

        let db = self.client.db.lock().await;
        db.upsert_resale_listing(content_id, seller, price_wei, now)?;
        Ok(())
    }

    /// Confirm cancel listing: deactivate in local DB.
    pub async fn confirm_cancel_listing(
        &self,
        content_id: &str,
        seller: &str,
    ) -> Result<()> {
        let db = self.client.db.lock().await;
        db.deactivate_resale_listing(content_id, seller)?;
        Ok(())
    }

    /// Get active resale listings for a content item from local DB.
    pub async fn get_resale_listings(
        &self,
        content_id: &str,
    ) -> Result<Vec<crate::types::ResaleListing>> {
        let db = self.client.db.lock().await;
        let rows = db.get_active_resale_listings(content_id)?;

        Ok(rows
            .into_iter()
            .map(|(cid, seller, price_wei, _listed_at)| {
                let wei: U256 = price_wei.parse().unwrap_or(U256::ZERO);
                crate::types::ResaleListing {
                    content_id: cid,
                    seller,
                    price_wei: price_wei.clone(),
                    price_display: format_wei(wei),
                }
            })
            .collect())
    }

    /// Prepare a tip transaction. Tips flow through the same 85/2.5/12.5 split as purchases
    /// (85% creator, 2.5% stakers, 12.5% seeder reward pool).
    ///
    /// Unlike `prepare_purchase`, tipping does **NOT** mint an edition token — tipping
    /// expresses support, not ownership. A tipper can still purchase the content separately.
    /// Works on both free (price=0) and paid content.
    pub fn prepare_tip(
        &self,
        content_id: FixedBytes<32>,
        tip_wei: U256,
    ) -> Result<Vec<TransactionRequest>> {
        anyhow::ensure!(!tip_wei.is_zero(), "Tip amount must be greater than 0");

        let eth = &self.client.config.ethereum;
        let marketplace_addr: Address = eth.marketplace_address.parse()?;
        let calldata = MarketplaceClient::<()>::tip_content_calldata(content_id);

        Ok(vec![TransactionRequest {
            to: format!("{marketplace_addr:#x}"),
            data: format!("0x{}", alloy::hex::encode(&calldata)),
            value: format!("0x{:x}", tip_wei),
            description: format!("Tip {} ETH to content creator", format_wei(tip_wei)),
        }])
    }
}
