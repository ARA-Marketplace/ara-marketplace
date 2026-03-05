use alloy::primitives::{Address, FixedBytes, U256};
use anyhow::Result;

use ara_chain::staking::StakingClient;
use ara_chain::token::TokenClient;

use crate::client::AraClient;
use crate::types::{
    format_token_amount, format_wei, hex_encode, parse_amount, StakeInfo,
    TokenRewardInfo, TransactionRequest,
};

/// Staking operations: stake, unstake, claim rewards.
pub struct StakingOps<'a> {
    pub(crate) client: &'a AraClient,
}

impl StakingOps<'_> {
    /// Prepare stake ARA transactions (approve + stake).
    pub fn prepare_stake(&self, amount: &str) -> Result<Vec<TransactionRequest>> {
        let amount_wei = parse_amount(amount, 18)?;
        let eth = &self.client.config.ethereum;
        let token_addr: Address = eth.ara_token_address.parse()?;
        let staking_addr: Address = eth.staking_address.parse()?;

        let approve = TokenClient::<()>::approve_calldata(staking_addr, amount_wei);
        let stake = StakingClient::<()>::stake_calldata(amount_wei);

        Ok(vec![
            TransactionRequest {
                to: format!("{token_addr:#x}"),
                data: hex_encode(&approve),
                value: "0x0".to_string(),
                description: format!("Approve {} ARA for staking", amount),
            },
            TransactionRequest {
                to: format!("{staking_addr:#x}"),
                data: hex_encode(&stake),
                value: "0x0".to_string(),
                description: format!("Stake {} ARA", amount),
            },
        ])
    }

    /// Prepare unstake ARA transaction.
    pub fn prepare_unstake(&self, amount: &str) -> Result<Vec<TransactionRequest>> {
        let amount_wei = parse_amount(amount, 18)?;
        let eth = &self.client.config.ethereum;
        let staking_addr: Address = eth.staking_address.parse()?;

        let calldata = StakingClient::<()>::unstake_calldata(amount_wei);

        Ok(vec![TransactionRequest {
            to: format!("{staking_addr:#x}"),
            data: hex_encode(&calldata),
            value: "0x0".to_string(),
            description: format!("Unstake {} ARA", amount),
        }])
    }

    /// Prepare stake-for-content transactions (approve + stakeForContent).
    pub fn prepare_stake_for_content(
        &self,
        content_id: FixedBytes<32>,
        amount: &str,
    ) -> Result<Vec<TransactionRequest>> {
        let amount_wei = parse_amount(amount, 18)?;
        let eth = &self.client.config.ethereum;
        let token_addr: Address = eth.ara_token_address.parse()?;
        let staking_addr: Address = eth.staking_address.parse()?;

        let approve = TokenClient::<()>::approve_calldata(staking_addr, amount_wei);
        let stake = StakingClient::<()>::stake_for_content_calldata(content_id, amount_wei);

        Ok(vec![
            TransactionRequest {
                to: format!("{token_addr:#x}"),
                data: hex_encode(&approve),
                value: "0x0".to_string(),
                description: format!("Approve {} ARA for content staking", amount),
            },
            TransactionRequest {
                to: format!("{staking_addr:#x}"),
                data: hex_encode(&stake),
                value: "0x0".to_string(),
                description: format!("Stake {} ARA for content", amount),
            },
        ])
    }

    /// Prepare claim ETH staking reward transaction.
    pub async fn prepare_claim_eth_reward(
        &self,
        user: Address,
    ) -> Result<Vec<TransactionRequest>> {
        let eth = &self.client.config.ethereum;
        let staking_addr: Address = eth.staking_address.parse()?;

        let chain = self.client.chain_client()?;
        let earned: U256 = chain.staking.earned(user).await?;

        if earned.is_zero() {
            anyhow::bail!("No ETH staking rewards to claim");
        }

        let calldata = StakingClient::<()>::claim_staking_reward_calldata();

        Ok(vec![TransactionRequest {
            to: format!("{staking_addr:#x}"),
            data: hex_encode(&calldata),
            value: "0x0".to_string(),
            description: format!("Claim {} ETH staking rewards", format_wei(earned)),
        }])
    }

    /// Prepare claim ERC-20 token staking reward transaction.
    pub async fn prepare_claim_token_reward(
        &self,
        user: Address,
        token_address: Address,
    ) -> Result<Vec<TransactionRequest>> {
        let eth = &self.client.config.ethereum;
        let staking_addr: Address = eth.staking_address.parse()?;

        let chain = self.client.chain_client()?;
        let earned: U256 = chain.staking.earned_token(user, token_address).await?;

        if earned.is_zero() {
            anyhow::bail!("No token staking rewards to claim");
        }

        let token_cfg = eth.supported_tokens.iter()
            .find(|t| t.address.eq_ignore_ascii_case(&format!("{token_address:#x}")));
        let symbol = token_cfg.map(|t| t.symbol.as_str()).unwrap_or("TOKEN");
        let decimals = token_cfg.map(|t| t.decimals).unwrap_or(18);

        let calldata = StakingClient::<()>::claim_token_reward_calldata(token_address);

        Ok(vec![TransactionRequest {
            to: format!("{staking_addr:#x}"),
            data: hex_encode(&calldata),
            value: "0x0".to_string(),
            description: format!("Claim {} {} staking rewards", format_token_amount(earned, decimals), symbol),
        }])
    }

    /// Get staking info for a user from on-chain.
    pub async fn get_stake_info(&self, user: Address) -> Result<StakeInfo> {
        let chain = self.client.chain_client()?;
        let total_staked: U256 = chain.staking.total_staked().await?;
        let general: U256 = chain.staking.staked_balance(user).await?;
        let total_user: U256 = chain.staking.total_user_stake(user).await?;
        let earned: U256 = chain.staking.earned(user).await?;

        Ok(StakeInfo {
            total_staked_wei: total_staked.to_string(),
            general_balance_wei: general.to_string(),
            eth_reward_earned_wei: earned.to_string(),
            total_user_stake_wei: total_user.to_string(),
        })
    }

    /// Get token reward info for a user for a specific token.
    pub async fn get_token_reward(
        &self,
        user: Address,
        token_address: Address,
    ) -> Result<TokenRewardInfo> {
        let eth = &self.client.config.ethereum;
        let chain = self.client.chain_client()?;
        let earned: U256 = chain.staking.earned_token(user, token_address).await?;

        let token_cfg = eth.supported_tokens.iter()
            .find(|t| t.address.eq_ignore_ascii_case(&format!("{token_address:#x}")));
        let symbol = token_cfg.map(|t| t.symbol.clone()).unwrap_or_else(|| "TOKEN".to_string());
        let decimals = token_cfg.map(|t| t.decimals).unwrap_or(18);

        Ok(TokenRewardInfo {
            token_address: format!("{token_address:#x}"),
            symbol,
            earned_raw: earned.to_string(),
            earned_display: format_token_amount(earned, decimals),
        })
    }
}
