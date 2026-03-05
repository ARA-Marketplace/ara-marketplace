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
}
