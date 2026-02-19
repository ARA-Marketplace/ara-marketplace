use alloy::sol;

// Generate type-safe contract bindings from Solidity interfaces.
// These are compiled from the Foundry project's ABI output.

sol! {
    #[sol(rpc)]
    interface IAraToken {
        function balanceOf(address account) external view returns (uint256);
        function transfer(address to, uint256 amount) external returns (bool);
        function approve(address spender, uint256 amount) external returns (bool);
        function transferFrom(address from, address to, uint256 amount) external returns (bool);
        function allowance(address owner, address spender) external view returns (uint256);
    }

    #[sol(rpc)]
    interface IAraStaking {
        function stake(uint256 amount) external;
        function unstake(uint256 amount) external;
        function stakeForContent(bytes32 contentId, uint256 amount) external;
        function unstakeFromContent(bytes32 contentId, uint256 amount) external;
        function stakedBalance(address user) external view returns (uint256);
        function contentStake(address user, bytes32 contentId) external view returns (uint256);
        function isEligiblePublisher(address user) external view returns (bool);
        function isEligibleSeeder(address user, bytes32 contentId) external view returns (bool);

        event Staked(address indexed user, uint256 amount);
        event Unstaked(address indexed user, uint256 amount);
        event ContentStakeAdded(address indexed user, bytes32 indexed contentId, uint256 amount);
        event ContentStakeRemoved(address indexed user, bytes32 indexed contentId, uint256 amount);
    }

    #[sol(rpc)]
    interface IContentRegistry {
        function publishContent(bytes32 contentHash, string metadataURI, uint256 priceWei) external returns (bytes32 contentId);
        function updateContent(bytes32 contentId, uint256 newPriceWei, string newMetadataURI) external;
        function delistContent(bytes32 contentId) external;
        function getContentCount() external view returns (uint256);
        function getContentHash(bytes32 contentId) external view returns (bytes32);
        function getPrice(bytes32 contentId) external view returns (uint256);
        function getCreator(bytes32 contentId) external view returns (address);
        function isActive(bytes32 contentId) external view returns (bool);

        event ContentPublished(bytes32 indexed contentId, address indexed creator, bytes32 contentHash, string metadataURI, uint256 priceWei);
        event ContentUpdated(bytes32 indexed contentId, uint256 newPriceWei, string newMetadataURI);
        event ContentDelisted(bytes32 indexed contentId);
    }

    #[sol(rpc)]
    interface IMarketplace {
        function purchase(bytes32 contentId) external payable;
        function distributeRewards(bytes32 contentId, address[] seeders, uint256[] weights) external;
        function claimRewards() external;
        function hasPurchased(bytes32 contentId, address buyer) external view returns (bool);
        function rewardPool(bytes32 contentId) external view returns (uint256);
        function claimableRewards(address seeder) external view returns (uint256);

        event ContentPurchased(bytes32 indexed contentId, address indexed buyer, uint256 pricePaid, uint256 creatorPayment, uint256 poolContribution);
        event RewardsDistributed(bytes32 indexed contentId, address[] seeders, uint256[] amounts, uint256 totalAmount);
        event RewardClaimed(address indexed seeder, uint256 amount);
    }
}
