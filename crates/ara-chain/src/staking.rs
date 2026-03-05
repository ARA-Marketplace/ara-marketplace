use alloy::primitives::{Address, FixedBytes, U256};
use alloy::providers::Provider;
use alloy::sol_types::SolCall;
use anyhow::Result;
use tracing::info;

use crate::contracts::IAraStaking;

/// Wrapper for AraStaking contract interactions.
/// Handles staking, eligibility checks, and content-specific stake management.
pub struct StakingClient<P> {
    address: Address,
    provider: P,
}

impl<P: Provider + Clone> StakingClient<P> {
    pub fn new(address: Address, provider: P) -> Self {
        Self { address, provider }
    }

    // --- Read operations (direct RPC calls) ---

    /// Get the user's general staked balance.
    pub async fn staked_balance(&self, user: Address) -> Result<U256> {
        info!("Querying staked balance for {}", user);
        let contract = IAraStaking::new(self.address, &self.provider);
        let result = contract.stakedBalance(user).call().await?;
        Ok(result)
    }

    /// Get the user's content-specific stake.
    pub async fn content_stake(
        &self,
        user: Address,
        content_id: FixedBytes<32>,
    ) -> Result<U256> {
        info!("Querying content stake for {} on {}", user, content_id);
        let contract = IAraStaking::new(self.address, &self.provider);
        let result = contract.contentStake(user, content_id).call().await?;
        Ok(result)
    }

    /// Check if user is eligible to publish content.
    pub async fn is_eligible_publisher(&self, user: Address) -> Result<bool> {
        info!("Checking publisher eligibility for {}", user);
        let contract = IAraStaking::new(self.address, &self.provider);
        let result = contract.isEligiblePublisher(user).call().await?;
        Ok(result)
    }

    /// Get the minimum ARA stake required to be an eligible seeder.
    pub async fn seeder_min_stake(&self) -> Result<U256> {
        let contract = IAraStaking::new(self.address, &self.provider);
        let result = contract.seederMinStake().call().await?;
        Ok(result)
    }

    /// Check if user is eligible to seed specific content.
    pub async fn is_eligible_seeder(
        &self,
        user: Address,
        content_id: FixedBytes<32>,
    ) -> Result<bool> {
        info!("Checking seeder eligibility for {} on {}", user, content_id);
        let contract = IAraStaking::new(self.address, &self.provider);
        let result = contract.isEligibleSeeder(user, content_id).call().await?;
        Ok(result)
    }

    /// Get the staking contract address.
    pub fn address(&self) -> Address {
        self.address
    }

    // --- V2: Passive staker rewards ---

    /// Get total ARA staked across all users.
    pub async fn total_staked(&self) -> Result<U256> {
        let contract = IAraStaking::new(self.address, &self.provider);
        let result = contract.totalStaked().call().await?;
        Ok(result)
    }

    /// Get a user's total stake (general + content-allocated).
    pub async fn total_user_stake(&self, user: Address) -> Result<U256> {
        let contract = IAraStaking::new(self.address, &self.provider);
        let result = contract.totalUserStake(user).call().await?;
        Ok(result)
    }

    /// Get the unclaimed passive staker reward for a user (in ETH wei).
    pub async fn earned(&self, user: Address) -> Result<U256> {
        info!("Querying earned staker reward for {}", user);
        let contract = IAraStaking::new(self.address, &self.provider);
        let result = contract.earned(user).call().await?;
        Ok(result)
    }

    // --- V3: Multi-token rewards ---

    /// Get the unclaimed ERC-20 token reward for a user.
    pub async fn earned_token(
        &self,
        user: Address,
        token: Address,
    ) -> Result<U256> {
        info!("Querying earned token reward for {} (token={})", user, token);
        let contract = IAraStaking::new(self.address, &self.provider);
        let result = contract.earnedToken(user, token).call().await?;
        Ok(result)
    }
}

// Calldata encoding — no provider needed.
impl<P> StakingClient<P> {
    /// Encode calldata for `stake(amount)`.
    pub fn stake_calldata(amount: U256) -> Vec<u8> {
        IAraStaking::stakeCall { amount }.abi_encode()
    }

    /// Encode calldata for `unstake(amount)`.
    pub fn unstake_calldata(amount: U256) -> Vec<u8> {
        IAraStaking::unstakeCall { amount }.abi_encode()
    }

    /// Encode calldata for `stakeForContent(contentId, amount)`.
    pub fn stake_for_content_calldata(
        content_id: FixedBytes<32>,
        amount: U256,
    ) -> Vec<u8> {
        IAraStaking::stakeForContentCall {
            contentId: content_id,
            amount,
        }
        .abi_encode()
    }

    /// Encode calldata for `unstakeFromContent(contentId, amount)`.
    pub fn unstake_from_content_calldata(
        content_id: FixedBytes<32>,
        amount: U256,
    ) -> Vec<u8> {
        IAraStaking::unstakeFromContentCall {
            contentId: content_id,
            amount,
        }
        .abi_encode()
    }

    /// Encode calldata for `claimStakingReward()`.
    pub fn claim_staking_reward_calldata() -> Vec<u8> {
        IAraStaking::claimStakingRewardCall {}.abi_encode()
    }

    /// Encode calldata for `claimTokenReward(token)`.
    /// Claim accrued ERC-20 staker rewards for a specific token.
    pub fn claim_token_reward_calldata(token: Address) -> Vec<u8> {
        IAraStaking::claimTokenRewardCall { token }.abi_encode()
    }
}
