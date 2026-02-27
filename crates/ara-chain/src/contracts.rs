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
        function seederMinStake() external view returns (uint256);

        event Staked(address indexed user, uint256 amount);
        event Unstaked(address indexed user, uint256 amount);
        event ContentStakeAdded(address indexed user, bytes32 indexed contentId, uint256 amount);
        event ContentStakeRemoved(address indexed user, bytes32 indexed contentId, uint256 amount);
    }

    #[sol(rpc)]
    interface IAraContent {
        function publishContent(bytes32 contentHash, string metadataURI, uint256 priceWei, uint256 fileSize, uint256 maxSupply, uint96 royaltyBps) external returns (bytes32 contentId);
        function updateContent(bytes32 contentId, uint256 newPriceWei, string newMetadataURI) external;
        function updateContentFile(bytes32 contentId, bytes32 newContentHash) external;
        function updateFileSize(bytes32 contentId, uint256 newFileSize) external;
        function delistContent(bytes32 contentId) external;
        function getContentCount() external view returns (uint256);
        function getContentHash(bytes32 contentId) external view returns (bytes32);
        function getPrice(bytes32 contentId) external view returns (uint256);
        function getCreator(bytes32 contentId) external view returns (address);
        function getFileSize(bytes32 contentId) external view returns (uint256);
        function isActive(bytes32 contentId) external view returns (bool);
        function getMaxSupply(bytes32 contentId) external view returns (uint256);
        function getTotalMinted(bytes32 contentId) external view returns (uint256);
        function balanceOf(address account, uint256 id) external view returns (uint256);
        function setApprovalForAll(address operator, bool approved) external;
        function isApprovedForAll(address account, address operator) external view returns (bool);

        event ContentPublished(bytes32 indexed contentId, address indexed creator, bytes32 contentHash, string metadataURI, uint256 priceWei, uint256 fileSize, uint256 maxSupply);
        event ContentUpdated(bytes32 indexed contentId, uint256 newPriceWei, string newMetadataURI);
        event ContentFileUpdated(bytes32 indexed contentId, bytes32 oldHash, bytes32 newHash, address indexed creator);
        event ContentDelisted(bytes32 indexed contentId);
    }

    #[sol(rpc)]
    interface IMarketplace {
        struct ClaimParams {
            bytes32 contentId;
            address buyer;
            uint256 bytesServed;
            uint256 timestamp;
            bytes signature;
        }

        function purchase(bytes32 contentId) external payable;
        function claimDeliveryReward(bytes32 contentId, address buyer, uint256 bytesServed, uint256 timestamp, bytes signature) external;
        function claimDeliveryRewards(ClaimParams[] claims) external;
        function hasPurchased(bytes32 contentId, address buyer) external view returns (bool);
        function buyerReward(bytes32 contentId, address buyer) external view returns (uint256);
        function buyerRewardPaid(bytes32 contentId, address buyer) external view returns (uint256);
        function getBuyerReward(bytes32 contentId, address buyer) external view returns (uint256);
        function totalRewardsClaimed() external view returns (uint256);
        function listings(bytes32 contentId, address seller) external view returns (uint256 price, bool active);
        function listForResale(bytes32 contentId, uint256 price) external;
        function cancelListing(bytes32 contentId) external;
        function buyResale(bytes32 contentId, address seller) external payable;

        event ContentPurchased(bytes32 indexed contentId, address indexed buyer, uint256 pricePaid, uint256 creatorPayment, uint256 rewardAmount);
        event DeliveryRewardClaimed(bytes32 indexed contentId, address indexed seeder, address buyer, uint256 amount, uint256 bytesServed);
        event RewardsClaimed(address indexed seeder, uint256 totalAmount, uint256 receiptCount);
        event ContentListed(bytes32 indexed contentId, address indexed seller, uint256 price);
        event ListingCancelled(bytes32 indexed contentId, address indexed seller);
        event ResalePurchased(bytes32 indexed contentId, address indexed buyer, address indexed seller, uint256 price, uint256 royaltyAmount, uint256 seederReward);
    }
}
