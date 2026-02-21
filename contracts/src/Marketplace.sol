// SPDX-License-Identifier: LGPL-3.0
pragma solidity ^0.8.24;

import {ContentRegistry} from "./ContentRegistry.sol";
import {AraStaking} from "./AraStaking.sol";

/// @title Marketplace
/// @notice Handles content purchases (ETH payments) and seeder reward distribution.
///         When a buyer purchases content:
///           - Creator receives creatorShareBps% of the payment
///           - Remaining goes to the reward pool for that content
///
///         Two paths to distribute rewards:
///           1. Creator fast path: content creator (or global reporter) calls distributeRewards()
///              anytime with off-chain receipt aggregation. No on-chain proof required.
///           2. Trustless fallback: after distributionWindow has elapsed since the last purchase,
///              any eligible seeder can call publicDistributeWithProofs() by submitting
///              buyer-signed EIP-712 delivery receipts. The contract verifies each signature
///              on-chain — no trust in any party required.
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

    /// @notice Authorized reward reporter address (global fallback, always allowed)
    address public reporter;

    /// @notice Contract owner
    address public owner;

    /// @notice Timestamp of most recent purchase per content (for distribution window)
    mapping(bytes32 => uint256) public lastPurchaseTime;

    /// @notice Time after last purchase before trustless public distribution unlocks
    uint256 public distributionWindow;

    /// @notice Replay protection: keccak256(contentId, seederEthAddress, buyerAddress, timestamp) => used
    mapping(bytes32 => bool) public usedReceipts;

    // --- EIP-712 ---
    bytes32 private constant DOMAIN_TYPE_HASH =
        keccak256("EIP712Domain(string name,string version,uint256 chainId,address verifyingContract)");

    /// @dev DeliveryReceipt(bytes32 contentId,address seederEthAddress,uint256 timestamp)
    ///      Buyer signs this to prove they received content from a specific seeder.
    bytes32 public constant RECEIPT_TYPE_HASH =
        keccak256("DeliveryReceipt(bytes32 contentId,address seederEthAddress,uint256 timestamp)");

    bytes32 public immutable DOMAIN_SEPARATOR;

    // --- Structs for publicDistributeWithProofs ---

    struct SignedReceipt {
        uint256 timestamp;
        bytes signature; // 65-byte ECDSA over EIP-712 DeliveryReceipt hash
    }

    struct SeederBundle {
        address seeder; // Seeder's Ethereum address
        SignedReceipt[] receipts; // Buyer-signed proofs that this seeder served the content
    }

    // --- Events ---
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
    event DistributionWindowUpdated(uint256 oldWindow, uint256 newWindow);

    // --- Errors ---
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
    error DistributionWindowNotOpen();
    error NotEligibleSeeder();

    modifier onlyOwner() {
        if (msg.sender != owner) revert OnlyOwner();
        _;
    }

    constructor(address _registry, address _staking, uint256 _creatorShareBps) {
        if (_creatorShareBps > BPS_DENOMINATOR) revert InvalidCreatorShare();
        registry = ContentRegistry(_registry);
        staking = AraStaking(_staking);
        creatorShareBps = _creatorShareBps;
        owner = msg.sender;
        reporter = msg.sender; // Owner is initial reporter
        distributionWindow = 30 days;
        DOMAIN_SEPARATOR = keccak256(
            abi.encode(DOMAIN_TYPE_HASH, keccak256("AraMarketplace"), keccak256("1"), block.chainid, address(this))
        );
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
        lastPurchaseTime[contentId] = block.timestamp;

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
    ///         Callable by the global reporter OR the content's creator.
    ///         Uses off-chain aggregated weights (e.g. receipt counts × stake). No on-chain proof required.
    /// @param contentId The content whose reward pool to distribute
    /// @param seeders Array of seeder addresses
    /// @param weights Array of proportional weights
    function distributeRewards(bytes32 contentId, address[] calldata seeders, uint256[] calldata weights) external {
        address creator = registry.getCreator(contentId);
        if (msg.sender != reporter && msg.sender != creator) revert OnlyReporter();

        if (seeders.length != weights.length) revert ArrayLengthMismatch();
        if (rewardPool[contentId] == 0) revert NoRewardsToDistribute();

        for (uint256 i = 0; i < seeders.length; i++) {
            if (!staking.isEligibleSeeder(seeders[i], contentId)) {
                revert SeederNotEligible(seeders[i]);
            }
        }

        _distribute(contentId, seeders, weights);
    }

    /// @notice Trustless reward distribution with on-chain buyer receipt verification.
    ///         Callable by any eligible seeder after distributionWindow has elapsed
    ///         since the last purchase (i.e. creator has had enough time to distribute
    ///         voluntarily but hasn't).
    ///
    ///         Each bundle contains buyer-signed EIP-712 DeliveryReceipt structs proving
    ///         a buyer received content from a seeder. The contract:
    ///           1. Verifies each buyer signature (ecrecover)
    ///           2. Confirms the buyer actually purchased the content
    ///           3. Marks the receipt as used (replay protection)
    ///           4. Tallies verified receipt counts as weights
    ///           5. Distributes the reward pool proportionally
    ///
    /// @param contentId The content whose reward pool to distribute
    /// @param bundles Per-seeder receipt bundles
    function publicDistributeWithProofs(bytes32 contentId, SeederBundle[] calldata bundles) external {
        if (lastPurchaseTime[contentId] == 0) revert NoRewardsToDistribute();
        if (block.timestamp <= lastPurchaseTime[contentId] + distributionWindow) revert DistributionWindowNotOpen();
        if (!staking.isEligibleSeeder(msg.sender, contentId)) revert NotEligibleSeeder();
        if (rewardPool[contentId] == 0) revert NoRewardsToDistribute();

        address[] memory seeders = new address[](bundles.length);
        uint256[] memory weights = new uint256[](bundles.length);

        for (uint256 i = 0; i < bundles.length; i++) {
            // Skip ineligible seeders rather than reverting — valid receipts from
            // other seeders should still be processed.
            if (!staking.isEligibleSeeder(bundles[i].seeder, contentId)) continue;
            seeders[i] = bundles[i].seeder;

            for (uint256 j = 0; j < bundles[i].receipts.length; j++) {
                SignedReceipt calldata r = bundles[i].receipts[j];

                // Compute EIP-712 hash for DeliveryReceipt(contentId, seederEthAddress, timestamp)
                bytes32 structHash =
                    keccak256(abi.encode(RECEIPT_TYPE_HASH, contentId, bundles[i].seeder, r.timestamp));
                bytes32 eip712Hash = keccak256(abi.encodePacked("\x19\x01", DOMAIN_SEPARATOR, structHash));

                // Recover buyer from signature
                address buyer = _ecrecover(eip712Hash, r.signature);
                if (buyer == address(0)) continue; // invalid signature
                if (!hasPurchased[contentId][buyer]) continue; // not a verified buyer

                // Replay protection key includes buyer to allow one receipt per buyer per seeder
                bytes32 receiptKey = keccak256(abi.encode(contentId, bundles[i].seeder, buyer, r.timestamp));
                if (usedReceipts[receiptKey]) continue;

                usedReceipts[receiptKey] = true;
                weights[i]++;
            }
        }

        _distribute(contentId, seeders, weights);
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

    /// @notice Update the distribution window duration
    function setDistributionWindow(uint256 newWindow) external onlyOwner {
        emit DistributionWindowUpdated(distributionWindow, newWindow);
        distributionWindow = newWindow;
    }

    /// @notice Transfer ownership
    function transferOwnership(address newOwner) external onlyOwner {
        owner = newOwner;
    }

    receive() external payable {}

    // --- Internal helpers ---

    /// @dev Proportionally distribute pooled rewards to seeders by weight.
    ///      Seeders with zero weight are skipped. Rounding dust stays in the pool.
    function _distribute(bytes32 contentId, address[] memory seeders, uint256[] memory weights) internal {
        uint256 totalWeight = 0;
        for (uint256 i = 0; i < weights.length; i++) {
            totalWeight += weights[i];
        }
        if (totalWeight == 0) revert ZeroWeight();

        uint256 poolAmount = rewardPool[contentId];
        uint256[] memory amounts = new uint256[](seeders.length);
        uint256 distributed = 0;

        for (uint256 i = 0; i < seeders.length; i++) {
            if (weights[i] == 0) continue;
            amounts[i] = (poolAmount * weights[i]) / totalWeight;
            claimableRewards[seeders[i]] += amounts[i];
            distributed += amounts[i];
        }

        // Rounding dust stays in the pool
        rewardPool[contentId] = poolAmount - distributed;
        totalRewardsDistributed += distributed;

        emit RewardsDistributed(contentId, seeders, amounts, distributed);
    }

    /// @dev Recover the signer of an EIP-712 hash from a 65-byte signature.
    ///      Returns address(0) on invalid input.
    function _ecrecover(bytes32 hash, bytes calldata sig) internal pure returns (address) {
        if (sig.length != 65) return address(0);
        bytes32 r;
        bytes32 s;
        uint8 v;
        assembly {
            r := calldataload(sig.offset)
            s := calldataload(add(sig.offset, 32))
            v := byte(0, calldataload(add(sig.offset, 64)))
        }
        // Some wallets return v as 0/1; normalize to 27/28
        if (v < 27) v += 27;
        if (v != 27 && v != 28) return address(0);
        return ecrecover(hash, v, r, s);
    }
}
