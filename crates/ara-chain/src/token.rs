use alloy::primitives::{Address, U256};
use alloy::providers::Provider;
use alloy::sol_types::SolCall;
use anyhow::Result;
use tracing::info;

use crate::contracts::IAraToken;

/// ARA token interaction client.
/// Handles balance queries, approvals, and transfers for the existing ARA ERC20 token.
pub struct TokenClient<P> {
    address: Address,
    provider: P,
}

impl<P: Provider + Clone> TokenClient<P> {
    pub fn new(address: Address, provider: P) -> Self {
        Self { address, provider }
    }

    /// Get ARA token balance for an address.
    pub async fn balance_of(&self, account: Address) -> Result<U256> {
        info!("Querying ARA balance for {}", account);
        let contract = IAraToken::new(self.address, &self.provider);
        let balance = contract.balanceOf(account).call().await?;
        Ok(balance)
    }

    /// Get the current allowance for a spender.
    pub async fn allowance(&self, owner: Address, spender: Address) -> Result<U256> {
        info!("Querying allowance: owner={}, spender={}", owner, spender);
        let contract = IAraToken::new(self.address, &self.provider);
        let allowance = contract.allowance(owner, spender).call().await?;
        Ok(allowance)
    }

    /// Get the token contract address.
    pub fn address(&self) -> Address {
        self.address
    }
}

// Calldata encoding — no provider needed.
impl<P> TokenClient<P> {
    /// Encode calldata for `approve(spender, amount)`.
    /// Frontend signs and broadcasts this transaction via WalletConnect.
    pub fn approve_calldata(spender: Address, amount: U256) -> Vec<u8> {
        IAraToken::approveCall { spender, amount }.abi_encode()
    }

    /// Encode calldata for `transfer(to, amount)`.
    pub fn transfer_calldata(to: Address, amount: U256) -> Vec<u8> {
        IAraToken::transferCall { to, amount }.abi_encode()
    }

    /// Encode calldata for `transferFrom(from, to, amount)`.
    pub fn transfer_from_calldata(from: Address, to: Address, amount: U256) -> Vec<u8> {
        IAraToken::transferFromCall { from, to, amount }.abi_encode()
    }
}
