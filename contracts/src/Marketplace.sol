// SPDX-License-Identifier: LGPL-3.0
pragma solidity ^0.8.24;

import {ContentRegistry} from "./ContentRegistry.sol";
import {AraStaking} from "./AraStaking.sol";

/// @title Marketplace
/// @notice Handles content purchases (ETH payments) and seeder reward distribution.
///         When a buyer purchases content:
///           - Creator receives creatorShareBps% of the payment
///           - Remaining goes to the reward pool for that content
///         A trusted reporter distributes pool rewards to seeders proportional to
///         their ARA stake and bytes served.
contract Marketplace {
    ContentRegistry public immutable registry;
    AraStaking public immutable staking;

    /// @notice Creator's share in basis points (8500 = 85%)
    uint256 public creatorShareBps;
    uint256 public constant BPS_DENOMINATOR = 10_000;

    /// @notice contentId => buyer => purchased
    mapping(bytes32 => mapping(address => bool)) public hasPurchased;

    /// @notice contentId => list of buyer addresses
    mapping(bytes32 => address[]) public purchasers;

    /// @notice Accumulated ETH reward pool per content
    mapping(bytes32 => uint256) public rewardPool;

    /// @notice Claimable ETH rewards per seeder (accumulated across all content)
    mapping(address => uint256) public claimableRewards;

    /// @notice Total ETH distributed as rewards (lifetime)
    uint256 public totalRewardsDistributed;

    /// @notice Total ETH paid to creators (lifetime)
    uint256 public totalCreatorPayments;

    /// @notice Authorized reward reporter address
    address public reporter;

    /// @notice Contract owner
    address public owner;

    event ContentPurchased(
        bytes32 indexed contentId,
        address indexed buyer,
        uint256 pricePaid,
        uint256 creatorPayment,
        uint256 poolContribution
    );
    event RewardsDistributed(bytes32 indexed contentId, address[] seeders, uint256[] amounts, uint256 totalAmount);
    event RewardClaimed(address indexed seeder, uint256 amount);
    event ReporterUpdated(address oldReporter, address newReporter);
    event CreatorShareUpdated(uint256 oldBps, uint256 newBps);

    error AlreadyPurchased();
    error ContentNotActive();
    error InsufficientPayment(uint256 sent, uint256 required);
    error ArrayLengthMismatch();
    error NoRewardsToDistribute();
    error SeederNotEligible(address seeder);
    error NoRewardsToClaim();
    error TransferFailed();
    error OnlyOwner();
    error OnlyReporter();
    error ZeroWeight();
    error InvalidCreatorShare();

    modifier onlyOwner() {
        if (msg.sender != owner) revert OnlyOwner();
        _;
    }

    modifier onlyReporter() {
        if (msg.sender != reporter) revert OnlyReporter();
        _;
    }

    constructor(address _registry, address _staking, uint256 _creatorShareBps) {
        if (_creatorShareBps > BPS_DENOMINATOR) revert InvalidCreatorShare();
        registry = ContentRegistry(_registry);
        staking = AraStaking(_staking);
        creatorShareBps = _creatorShareBps;
        owner = msg.sender;
        reporter = msg.sender; // Owner is initial reporter
    }

    /// @notice Purchase content with ETH. Payment is split between creator and reward pool.
    /// @param contentId The content to purchase
    function purchase(bytes32 contentId) external payable {
        if (hasPurchased[contentId][msg.sender]) revert AlreadyPurchased();
        if (!registry.isActive(contentId)) revert ContentNotActive();

        uint256 price = registry.getPrice(contentId);
        if (msg.value < price) revert InsufficientPayment(msg.value, price);

        hasPurchased[contentId][msg.sender] = true;
        purchasers[contentId].push(msg.sender);

        // Split payment: creator gets their share, rest goes to reward pool
        uint256 creatorPayment = (price * creatorShareBps) / BPS_DENOMINATOR;
        uint256 poolContribution = price - creatorPayment;

        // Pay creator
        address creator = registry.getCreator(contentId);
        (bool sent,) = payable(creator).call{value: creatorPayment}("");
        if (!sent) revert TransferFailed();

        totalCreatorPayments += creatorPayment;

        // Add to reward pool
        rewardPool[contentId] += poolContribution;

        // Refund overpayment
        if (msg.value > price) {
            (bool refunded,) = payable(msg.sender).call{value: msg.value - price}("");
            if (!refunded) revert TransferFailed();
        }

        emit ContentPurchased(contentId, msg.sender, price, creatorPayment, poolContribution);
    }

    /// @notice Distribute reward pool for a content to its seeders.
    ///         Called by the authorized reporter after aggregating off-chain metrics.
    /// @param contentId The content whose reward pool to distribute
    /// @param seeders Array of seeder addresses
    /// @param weights Array of proportional weights (ARA_staked * bytes_served)
    function distributeRewards(bytes32 contentId, address[] calldata seeders, uint256[] calldata weights)
        external
        onlyReporter
    {
        if (seeders.length != weights.length) revert ArrayLengthMismatch();
        if (rewardPool[contentId] == 0) revert NoRewardsToDistribute();

        uint256 totalWeight = 0;
        for (uint256 i = 0; i < weights.length; i++) {
            if (!staking.isEligibleSeeder(seeders[i], contentId)) {
                revert SeederNotEligible(seeders[i]);
            }
            totalWeight += weights[i];
        }
        if (totalWeight == 0) revert ZeroWeight();

        uint256 poolAmount = rewardPool[contentId];
        uint256[] memory amounts = new uint256[](seeders.length);
        uint256 distributed = 0;

        for (uint256 i = 0; i < seeders.length; i++) {
            amounts[i] = (poolAmount * weights[i]) / totalWeight;
            claimableRewards[seeders[i]] += amounts[i];
            distributed += amounts[i];
        }

        // Any dust from rounding stays in the pool
        rewardPool[contentId] = poolAmount - distributed;
        totalRewardsDistributed += distributed;

        emit RewardsDistributed(contentId, seeders, amounts, distributed);
    }

    /// @notice Claim all accumulated ETH rewards.
    function claimRewards() external {
        uint256 amount = claimableRewards[msg.sender];
        if (amount == 0) revert NoRewardsToClaim();

        claimableRewards[msg.sender] = 0;
        (bool sent,) = payable(msg.sender).call{value: amount}("");
        if (!sent) revert TransferFailed();

        emit RewardClaimed(msg.sender, amount);
    }

    /// @notice Check if an address has purchased specific content
    function checkPurchase(bytes32 contentId, address buyer) external view returns (bool) {
        return hasPurchased[contentId][buyer];
    }

    /// @notice Get number of purchasers for a content
    function getPurchaserCount(bytes32 contentId) external view returns (uint256) {
        return purchasers[contentId].length;
    }

    /// @notice Set the authorized reward reporter
    function setReporter(address newReporter) external onlyOwner {
        emit ReporterUpdated(reporter, newReporter);
        reporter = newReporter;
    }

    /// @notice Update the creator share percentage
    function setCreatorShare(uint256 newCreatorShareBps) external onlyOwner {
        if (newCreatorShareBps > BPS_DENOMINATOR) revert InvalidCreatorShare();
        emit CreatorShareUpdated(creatorShareBps, newCreatorShareBps);
        creatorShareBps = newCreatorShareBps;
    }

    /// @notice Transfer ownership
    function transferOwnership(address newOwner) external onlyOwner {
        owner = newOwner;
    }

    receive() external payable {}
}
