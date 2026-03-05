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

        // V2: Passive staker rewards
        function totalStaked() external view returns (uint256);
        function totalUserStake(address user) external view returns (uint256);
        function earned(address account) external view returns (uint256);
        function claimStakingReward() external;
        function totalStakerRewardsDeposited() external view returns (uint256);
        function totalStakerRewardsClaimed() external view returns (uint256);

        // V3: Multi-token rewards
        function addTokenReward(address token, uint256 amount) external;
        function earnedToken(address account, address token) external view returns (uint256);
        function claimTokenReward(address token) external;

        event Staked(address indexed user, uint256 amount);
        event Unstaked(address indexed user, uint256 amount);
        event ContentStakeAdded(address indexed user, bytes32 indexed contentId, uint256 amount);
        event ContentStakeRemoved(address indexed user, bytes32 indexed contentId, uint256 amount);
        event StakerRewardClaimed(address indexed user, uint256 amount);
        event TokenRewardDeposited(address indexed token, uint256 amount, uint256 newRewardPerToken);
        event TokenRewardClaimed(address indexed user, address indexed token, uint256 amount);
    }

    #[sol(rpc)]
    interface IAraContent {
        struct Collaborator {
            address wallet;
            uint256 shareBps;
        }

        function publishContent(bytes32 contentHash, string metadataURI, uint256 priceWei, uint256 fileSize, uint256 maxSupply, uint96 royaltyBps) external returns (bytes32 contentId);
        function publishContentWithToken(bytes32 contentHash, string metadataURI, uint256 price, uint256 fileSize, uint256 maxSupply, uint96 royaltyBps, address paymentToken) external returns (bytes32 contentId);
        function publishContentWithCollaborators(bytes32 contentHash, string metadataURI, uint256 priceWei, uint256 fileSize, uint256 maxSupply, uint96 royaltyBps, Collaborator[] collaborators) external returns (bytes32 contentId);
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
        function getPaymentToken(bytes32 contentId) external view returns (address);
        function hasCollaborators(bytes32 contentId) external view returns (bool);
        function getCollaborators(bytes32 contentId) external view returns (Collaborator[]);
        function getCollaboratorCount(bytes32 contentId) external view returns (uint256);
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

        function purchase(bytes32 contentId, uint256 maxPrice) external payable;
        function purchaseWithToken(bytes32 contentId, address token, uint256 amount) external;
        function claimDeliveryReward(bytes32 contentId, address buyer, uint256 bytesServed, uint256 timestamp, bytes signature) external;
        function claimDeliveryRewards(ClaimParams[] claims) external;
        function hasPurchased(bytes32 contentId, address buyer) external view returns (bool);
        function buyerReward(bytes32 contentId, address buyer) external view returns (uint256);
        function buyerRewardPaid(bytes32 contentId, address buyer) external view returns (uint256);
        function getBuyerReward(bytes32 contentId, address buyer) external view returns (uint256);
        function totalRewardsClaimed() external view returns (uint256);
        function totalStakerRewardsForwarded() external view returns (uint256);
        function stakerRewardBps() external view returns (uint256);
        function supportedTokens(address token) external view returns (bool);
        function listings(bytes32 contentId, address seller) external view returns (uint256 price, bool active);
        function listForResale(bytes32 contentId, uint256 price) external;
        function cancelListing(bytes32 contentId) external;
        function buyResale(bytes32 contentId, address seller, uint256 maxPrice) external payable;
        function setSupportedToken(address token, bool supported) external;

        event ContentPurchased(bytes32 indexed contentId, address indexed buyer, uint256 pricePaid, uint256 creatorPayment, uint256 rewardAmount);
        event DeliveryRewardClaimed(bytes32 indexed contentId, address indexed seeder, address buyer, uint256 amount, uint256 bytesServed);
        event RewardsClaimed(address indexed seeder, uint256 totalEthAmount, uint256 totalTokenAmount, uint256 receiptCount);
        event ContentListed(bytes32 indexed contentId, address indexed seller, uint256 price);
        event ListingCancelled(bytes32 indexed contentId, address indexed seller);
        event ResalePurchased(bytes32 indexed contentId, address indexed buyer, address indexed seller, uint256 price, uint256 royaltyAmount, uint256 seederReward);
    }

    #[sol(rpc)]
    interface IAraCollections {
        function createCollection(string name, string description, string bannerUri) external returns (uint256 collectionId);
        function updateCollection(uint256 collectionId, string name, string description, string bannerUri) external;
        function deleteCollection(uint256 collectionId) external;
        function addItem(uint256 collectionId, bytes32 contentId) external;
        function removeItem(uint256 collectionId, bytes32 contentId) external;
        function collections(uint256 collectionId) external view returns (address creator, string name, string description, string bannerUri, uint256 createdAt, bool active);
        function getCollectionItems(uint256 collectionId) external view returns (bytes32[]);
        function getCreatorCollections(address creator) external view returns (uint256[]);
        function getCollectionItemCount(uint256 collectionId) external view returns (uint256);
        function contentCollection(bytes32 contentId) external view returns (uint256);
        function nextCollectionId() external view returns (uint256);

        event CollectionCreated(uint256 indexed collectionId, address indexed creator, string name);
        event CollectionUpdated(uint256 indexed collectionId, string name, string description, string bannerUri);
        event CollectionDeleted(uint256 indexed collectionId);
        event ItemAddedToCollection(uint256 indexed collectionId, bytes32 indexed contentId);
        event ItemRemovedFromCollection(uint256 indexed collectionId, bytes32 indexed contentId);
    }

    #[sol(rpc)]
    interface IAraModeration {
        function flagContent(bytes32 contentId, uint8 reason, bool isEmergency) external;
        function vote(bytes32 contentId, bool uphold) external;
        function resolveFlag(bytes32 contentId) external;
        function appeal(bytes32 contentId) external;
        function setNsfw(bytes32 contentId, bool isNsfw) external;
        function voteNsfw(bytes32 contentId) external;
        function getProposalStatus(bytes32 contentId) external view returns (uint8);
        function getProposalDetail(bytes32 contentId) external view returns (
            address flagger, uint8 reason, bool isEmergency, uint256 flagCount,
            uint256 votingDeadline, uint256 upholdWeight, uint256 dismissWeight,
            uint8 status, bool appealed
        );
        function isNsfw(bytes32 contentId) external view returns (bool);
        function isPurged(bytes32 contentId) external view returns (bool);
        function hasFlagged(bytes32 contentId, address user) external view returns (bool);
        function hasVoted(bytes32 contentId, address user) external view returns (bool);

        event ContentFlagged(bytes32 indexed contentId, address indexed flagger, uint8 reason, bool isEmergency);
        event VoteCast(bytes32 indexed contentId, address indexed voter, bool uphold, uint256 weight);
        event FlagResolved(bytes32 indexed contentId, uint8 outcome, uint256 upholdWeight, uint256 dismissWeight);
        event ContentPurged(bytes32 indexed contentId, address indexed resolvedBy);
        event NsfwTagSet(bytes32 indexed contentId, address indexed setter, bool isNsfw);
    }

    #[sol(rpc)]
    interface IAraNameRegistry {
        function registerName(string name) external;
        function removeName() external;
        function getName(address user) external view returns (string);
        function getNames(address[] users) external view returns (string[]);
        function getAddress(string name) external view returns (address);
        function addressToName(address user) external view returns (string);

        event NameRegistered(address indexed user, string name);
        event NameRemoved(address indexed user, string oldName);
    }
}
