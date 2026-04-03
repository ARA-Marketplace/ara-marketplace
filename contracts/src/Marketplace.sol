// SPDX-License-Identifier: BUSL-1.1
pragma solidity ^0.8.24;

import {AraContent} from "./AraContent.sol";
import {AraStaking} from "./AraStaking.sol";
import {IERC20} from "@openzeppelin/contracts/token/ERC20/IERC20.sol";
import {SafeERC20} from "@openzeppelin/contracts/token/ERC20/utils/SafeERC20.sol";
import {ReentrancyGuard} from "@openzeppelin/contracts/utils/ReentrancyGuard.sol";
import {Initializable} from "@openzeppelin/contracts/proxy/utils/Initializable.sol";
import {UUPSUpgradeable} from "@openzeppelin/contracts/proxy/utils/UUPSUpgradeable.sol";

/// @title Marketplace
/// @notice Handles content purchases (ETH payments) and per-receipt seeder reward claiming.
///         When a buyer purchases content:
///           - Creator receives creatorShareBps% of the payment
///           - Remaining is held as a per-buyer reward claimable by seeders
///
///         Seeders claim rewards by submitting buyer-signed EIP-712 delivery receipts.
///         Each receipt proves "I delivered X bytes of content Y to buyer Z."
///         Reward is proportional: (bytesServed / fileSize) x buyerReward.
///         No "distribute" step -- each seeder independently claims what they've earned.
///
///         Also supports marketplace-facilitated resales: sellers list tokens, buyers pay ETH,
///         creator gets royalty, seeders get reward share, seller gets the rest.
contract Marketplace is Initializable, UUPSUpgradeable, ReentrancyGuard {
    using SafeERC20 for IERC20;

    AraContent public contentToken;
    AraStaking public staking;

    /// @notice Creator's share in basis points (8500 = 85%)
    uint256 public creatorShareBps;
    uint256 public constant BPS_DENOMINATOR = 10_000;

    /// @notice Resale seeder reward in basis points (e.g., 500 = 5%)
    uint256 public resaleRewardBps;

    /// @notice contentId => buyer => purchased
    mapping(bytes32 => mapping(address => bool)) public hasPurchased;

    /// @notice contentId => list of buyer addresses
    mapping(bytes32 => address[]) public purchasers;

    /// @notice Per-buyer reward amount set at purchase time (immutable after purchase)
    mapping(bytes32 => mapping(address => uint256)) public buyerReward;

    /// @notice Total rewards paid out from a buyer's purchase
    mapping(bytes32 => mapping(address => uint256)) public buyerRewardPaid;

    /// @notice Replay protection: contentId => buyer => seeder => bytes claimed
    mapping(bytes32 => mapping(address => mapping(address => uint256))) public bytesClaimed;

    /// @notice Total ETH claimed as rewards (lifetime)
    uint256 public totalRewardsClaimed;

    /// @notice Total ETH paid to creators (lifetime)
    uint256 public totalCreatorPayments;

    /// @notice Contract owner
    address public owner;

    // --- EIP-712 ---
    bytes32 private constant DOMAIN_TYPE_HASH =
        keccak256("EIP712Domain(string name,string version,uint256 chainId,address verifyingContract)");

    /// @dev DeliveryReceipt(bytes32 contentId,address seederEthAddress,uint256 bytesServed,uint256 timestamp)
    ///      Buyer signs this to prove they received content from a specific seeder.
    bytes32 public constant RECEIPT_TYPE_HASH =
        keccak256("DeliveryReceipt(bytes32 contentId,address seederEthAddress,uint256 bytesServed,uint256 timestamp)");

    bytes32 public DOMAIN_SEPARATOR;

    // --- Resale ---

    struct Listing {
        uint256 price;
        bool active;
    }

    /// @notice contentId => seller => listing
    mapping(bytes32 => mapping(address => Listing)) public listings;

    // === V2: Passive staker rewards ===

    /// @notice Staker reward share of primary purchases in basis points (250 = 2.5%)
    uint256 public stakerRewardBps;

    /// @notice Staker reward share of resale purchases in basis points (100 = 1%)
    uint256 public resaleStakerRewardBps;

    /// @notice Total ETH forwarded to staking contract for passive rewards (lifetime)
    uint256 public totalStakerRewardsForwarded;

    // === V3: Multi-token payment support ===

    /// @notice Whitelisted ERC-20 tokens that can be used for purchases
    mapping(address => bool) public supportedTokens;

    /// @notice Per-buyer reward token: contentId => buyer => token address used for purchase
    mapping(bytes32 => mapping(address => address)) public buyerRewardToken;

    // --- Structs ---

    struct ClaimParams {
        bytes32 contentId;
        address buyer;
        uint256 bytesServed;
        uint256 timestamp;
        bytes signature; // 65-byte ECDSA over EIP-712 DeliveryReceipt hash
    }

    // --- Events ---
    event ContentPurchased(
        bytes32 indexed contentId,
        address indexed buyer,
        uint256 pricePaid,
        uint256 creatorPayment,
        uint256 rewardAmount
    );
    event DeliveryRewardClaimed(
        bytes32 indexed contentId,
        address indexed seeder,
        address buyer,
        uint256 amount,
        uint256 bytesServed
    );
    event RewardsClaimed(address indexed seeder, uint256 totalEthAmount, uint256 totalTokenAmount, uint256 receiptCount);
    event ContentTipped(
        bytes32 indexed contentId,
        address indexed tipper,
        uint256 tipAmount,
        uint256 creatorPayment,
        uint256 rewardAmount
    );
    event CreatorShareUpdated(uint256 oldBps, uint256 newBps);
    event StakerRewardForwarded(bytes32 indexed contentId, uint256 amount);
    event ContentListed(bytes32 indexed contentId, address indexed seller, uint256 price);
    event ListingCancelled(bytes32 indexed contentId, address indexed seller);
    event ResalePurchased(
        bytes32 indexed contentId,
        address indexed buyer,
        address indexed seller,
        uint256 price,
        uint256 royaltyAmount,
        uint256 seederReward
    );

    // --- Errors ---
    error AlreadyPurchased();
    error ContentNotActive();
    error InsufficientPayment(uint256 sent, uint256 required);
    error NoRewardsToClaim();
    error TransferFailed();
    error OnlyOwner();
    error InvalidCreatorShare();
    error InvalidSignature();
    error NotPurchased();
    error AlreadyClaimed();
    error NoActiveListing();
    error NotTokenOwner();
    error NotApproved();
    error EditionSoldOut();
    error UnsupportedToken();
    error TokenMismatch();
    error ZeroAddress();
    error PriceTooLow();
    error ExcessiveFees();

    /// @notice Minimum resale price to prevent dust listings that bypass royalties via rounding
    uint256 public constant MIN_RESALE_PRICE = 1000;

    // === V4: Security hardening ===
    address public pendingOwner;

    modifier onlyOwner() {
        if (msg.sender != owner) revert OnlyOwner();
        _;
    }

    /// @custom:oz-upgrades-unsafe-allow constructor
    constructor() {
        _disableInitializers();
    }

    /// @notice Initialize the contract (called once via proxy)
    function initialize(address _contentToken, address _staking, uint256 _creatorShareBps, uint256 _resaleRewardBps)
        external
        initializer
    {
        if (_creatorShareBps > BPS_DENOMINATOR) revert InvalidCreatorShare();
        contentToken = AraContent(_contentToken);
        staking = AraStaking(_staking);
        creatorShareBps = _creatorShareBps;
        resaleRewardBps = _resaleRewardBps;
        owner = msg.sender;
        DOMAIN_SEPARATOR = keccak256(
            abi.encode(DOMAIN_TYPE_HASH, keccak256("AraMarketplace"), keccak256("1"), block.chainid, address(this))
        );
    }

    // ============================================================
    //                     PRIMARY PURCHASES
    // ============================================================

    /// @notice Purchase content with ETH. Creator gets their share, rest is held for seeders.
    ///         Only works for content priced in ETH (paymentToken = address(0)).
    /// @param contentId The content to purchase
    function purchase(bytes32 contentId, uint256 maxPrice) external payable nonReentrant {
        if (hasPurchased[contentId][msg.sender]) revert AlreadyPurchased();
        if (!contentToken.isActive(contentId)) revert ContentNotActive();
        // Ensure content is priced in ETH, not an ERC-20 token
        if (contentToken.getPaymentToken(contentId) != address(0)) revert TokenMismatch();

        uint256 price = contentToken.getPrice(contentId);
        if (price > maxPrice) revert InsufficientPayment(maxPrice, price);
        if (msg.value < price) revert InsufficientPayment(msg.value, price);

        hasPurchased[contentId][msg.sender] = true;
        purchasers[contentId].push(msg.sender);

        // Split: creator 85%, stakers 2.5%, seeders 12.5%
        uint256 creatorPayment = (price * creatorShareBps) / BPS_DENOMINATOR;
        uint256 stakerReward = (price * stakerRewardBps) / BPS_DENOMINATOR;
        uint256 rewardAmount = price - creatorPayment - stakerReward;

        // Pay creator (or split among collaborators)
        _payCreatorETH(contentId, creatorPayment);

        totalCreatorPayments += creatorPayment;

        // Forward staker reward to staking contract (if stakers exist)
        if (stakerReward > 0) {
            if (staking.totalStaked() > 0) {
                staking.addReward{value: stakerReward}();
                totalStakerRewardsForwarded += stakerReward;
                emit StakerRewardForwarded(contentId, stakerReward);
            } else {
                // No stakers — add to seeder pool instead
                rewardAmount += stakerReward;
            }
        }

        // Hold reward for seeder claiming (immutable after this point)
        buyerReward[contentId][msg.sender] = rewardAmount;

        // Mint ERC-1155 token to buyer
        contentToken.mint(msg.sender, contentId);

        // Refund overpayment
        if (msg.value > price) {
            (bool refunded,) = payable(msg.sender).call{value: msg.value - price}("");
            if (!refunded) revert TransferFailed();
        }

        emit ContentPurchased(contentId, msg.sender, price, creatorPayment, rewardAmount);
    }

    // ============================================================
    //                     ERC-20 TOKEN PURCHASES (V3)
    // ============================================================

    /// @notice Purchase content with an ERC-20 token.
    ///         The content must be priced in `token`, and `token` must be whitelisted.
    ///         Buyer must approve this contract for `amount` of `token` first.
    /// @param contentId The content to purchase
    /// @param token The ERC-20 token to pay with
    /// @param amount The amount of tokens to pay (must match content price)
    function purchaseWithToken(bytes32 contentId, address token, uint256 amount, uint256 maxPrice) external nonReentrant {
        if (hasPurchased[contentId][msg.sender]) revert AlreadyPurchased();
        if (!contentToken.isActive(contentId)) revert ContentNotActive();
        if (!supportedTokens[token]) revert UnsupportedToken();

        // Verify the content is priced in this token
        address expectedToken = contentToken.getPaymentToken(contentId);
        if (expectedToken != token) revert TokenMismatch();

        uint256 price = contentToken.getPrice(contentId);
        if (price > maxPrice) revert InsufficientPayment(maxPrice, price);
        if (amount < price) revert InsufficientPayment(amount, price);

        hasPurchased[contentId][msg.sender] = true;
        purchasers[contentId].push(msg.sender);

        // Split: creator 85%, stakers 2.5%, seeders 12.5%
        uint256 creatorPayment = (price * creatorShareBps) / BPS_DENOMINATOR;
        uint256 stakerReward = (price * stakerRewardBps) / BPS_DENOMINATOR;
        uint256 rewardAmount = price - creatorPayment - stakerReward;

        // Transfer tokens from buyer to this contract
        IERC20(token).safeTransferFrom(msg.sender, address(this), price);

        // Pay creator (or split among collaborators)
        _payCreatorToken(contentId, token, creatorPayment);
        totalCreatorPayments += creatorPayment;

        // Forward staker reward to staking contract as token reward
        if (stakerReward > 0) {
            if (staking.totalStaked() > 0) {
                // Approve staking contract to pull tokens
                IERC20(token).forceApprove(address(staking), stakerReward);
                staking.addTokenReward(token, stakerReward);
                totalStakerRewardsForwarded += stakerReward;
                emit StakerRewardForwarded(contentId, stakerReward);
            } else {
                // No stakers — add to seeder pool
                rewardAmount += stakerReward;
            }
        }

        // Hold token reward for seeder claiming
        buyerReward[contentId][msg.sender] = rewardAmount;
        buyerRewardToken[contentId][msg.sender] = token;

        // Mint ERC-1155 token to buyer
        contentToken.mint(msg.sender, contentId);

        emit ContentPurchased(contentId, msg.sender, price, creatorPayment, rewardAmount);
    }

    // ============================================================
    //                     TIPPING
    // ============================================================

    /// @notice Tip a content creator. The tip is split using the same formula as purchases:
    ///         creator gets creatorShareBps%, stakers get stakerRewardBps%, seeders get the rest.
    ///         Works on both free and paid content. Does NOT mint an edition token.
    /// @param contentId The content whose creator to tip
    function tipContent(bytes32 contentId) external payable nonReentrant {
        if (msg.value == 0) revert InsufficientPayment(0, 1);
        if (!contentToken.isActive(contentId)) revert ContentNotActive();

        uint256 tipAmount = msg.value;

        // Same split as primary purchases
        uint256 creatorPayment = (tipAmount * creatorShareBps) / BPS_DENOMINATOR;
        uint256 stakerReward = (tipAmount * stakerRewardBps) / BPS_DENOMINATOR;
        uint256 rewardAmount = tipAmount - creatorPayment - stakerReward;

        // Pay creator (or split among collaborators)
        _payCreatorETH(contentId, creatorPayment);
        totalCreatorPayments += creatorPayment;

        // Forward staker reward to staking contract (if stakers exist)
        if (stakerReward > 0) {
            if (staking.totalStaked() > 0) {
                staking.addReward{value: stakerReward}();
                totalStakerRewardsForwarded += stakerReward;
                emit StakerRewardForwarded(contentId, stakerReward);
            } else {
                // No stakers — add to seeder pool instead
                rewardAmount += stakerReward;
            }
        }

        // Add to seeder reward pool (additive — supports multiple tips)
        buyerReward[contentId][msg.sender] += rewardAmount;

        emit ContentTipped(contentId, msg.sender, tipAmount, creatorPayment, rewardAmount);
    }

    // ============================================================
    //                     REWARD CLAIMING
    // ============================================================

    /// @notice Claim reward for delivering content to a single buyer.
    ///         The calling seeder submits a buyer-signed delivery receipt.
    ///         Pays out in whatever token the buyer purchased with (ETH or ERC-20).
    function claimDeliveryReward(
        bytes32 contentId,
        address buyer,
        uint256 bytesServed,
        uint256 timestamp,
        bytes calldata signature
    ) external nonReentrant {
        uint256 payout = _verifyAndCalculateClaim(contentId, buyer, bytesServed, timestamp, signature);
        if (payout == 0) revert NoRewardsToClaim();

        totalRewardsClaimed += payout;

        // Pay in the token the buyer used for purchase (address(0) = ETH)
        address rewardToken = buyerRewardToken[contentId][buyer];
        if (rewardToken == address(0)) {
            (bool sent,) = payable(msg.sender).call{value: payout}("");
            if (!sent) revert TransferFailed();
        } else {
            IERC20(rewardToken).safeTransfer(msg.sender, payout);
        }

        emit DeliveryRewardClaimed(contentId, msg.sender, buyer, payout, bytesServed);
        if (rewardToken == address(0)) {
            emit RewardsClaimed(msg.sender, payout, 0, 1);
        } else {
            emit RewardsClaimed(msg.sender, 0, payout, 1);
        }
    }

    /// @notice Batch claim: submit multiple delivery receipts in one transaction.
    ///         All receipts must be for the calling seeder (msg.sender).
    ///         Invalid or already-claimed receipts are silently skipped.
    ///         Supports mixed ETH and token rewards — each claim pays in its original token.
    function claimDeliveryRewards(ClaimParams[] calldata claims) external nonReentrant {
        uint256 ethPayout = 0;
        uint256 tokenPayout = 0;
        uint256 validCount = 0;

        for (uint256 i = 0; i < claims.length; i++) {
            uint256 payout = _verifyAndCalculateClaim(
                claims[i].contentId,
                claims[i].buyer,
                claims[i].bytesServed,
                claims[i].timestamp,
                claims[i].signature
            );
            if (payout > 0) {
                validCount++;
                totalRewardsClaimed += payout;

                address rewardToken = buyerRewardToken[claims[i].contentId][claims[i].buyer];
                if (rewardToken == address(0)) {
                    ethPayout += payout;
                } else {
                    // Pay token immediately (can't aggregate different tokens)
                    IERC20(rewardToken).safeTransfer(msg.sender, payout);
                    tokenPayout += payout;
                }

                emit DeliveryRewardClaimed(
                    claims[i].contentId, msg.sender, claims[i].buyer, payout, claims[i].bytesServed
                );
            }
        }

        if (validCount == 0) revert NoRewardsToClaim();

        // Pay aggregated ETH
        if (ethPayout > 0) {
            (bool sent,) = payable(msg.sender).call{value: ethPayout}("");
            if (!sent) revert TransferFailed();
        }

        emit RewardsClaimed(msg.sender, ethPayout, tokenPayout, validCount);
    }

    // ============================================================
    //                     RESALE MARKETPLACE
    // ============================================================

    /// @notice List a content token for resale.
    ///         Seller must call contentToken.setApprovalForAll(marketplace, true) first.
    function listForResale(bytes32 contentId, uint256 price) external {
        if (contentToken.balanceOf(msg.sender, uint256(contentId)) == 0) revert NotTokenOwner();
        if (price < MIN_RESALE_PRICE) revert PriceTooLow();
        listings[contentId][msg.sender] = Listing({price: price, active: true});
        emit ContentListed(contentId, msg.sender, price);
    }

    /// @notice Cancel a resale listing
    function cancelListing(bytes32 contentId) external {
        if (!listings[contentId][msg.sender].active) revert NoActiveListing();
        listings[contentId][msg.sender].active = false;
        emit ListingCancelled(contentId, msg.sender);
    }

    /// @notice Buy a content token from a reseller.
    ///         Payment split: creator royalty + seeder reward + seller proceeds.
    function buyResale(bytes32 contentId, address seller, uint256 maxPrice) external payable nonReentrant {
        Listing storage listing = listings[contentId][seller];
        if (!listing.active) revert NoActiveListing();
        if (listing.price > maxPrice) revert InsufficientPayment(maxPrice, listing.price);
        if (msg.value < listing.price) revert InsufficientPayment(msg.value, listing.price);

        uint256 tokenId = uint256(contentId);

        // Verify seller still owns the token and has approved marketplace
        if (contentToken.balanceOf(seller, tokenId) == 0) revert NotTokenOwner();
        if (!contentToken.isApprovedForAll(seller, address(this))) revert NotApproved();

        uint256 price = listing.price;
        listing.active = false;

        // 1. Creator royalty (ERC-2981)
        (, uint256 royaltyAmount) = contentToken.royaltyInfo(tokenId, price);

        // 2. Staker reward (1% of resale)
        uint256 stakerReward = (price * resaleStakerRewardBps) / BPS_DENOMINATOR;

        // 3. Seeder reward (4% of resale)
        uint256 seederReward = (price * resaleRewardBps) / BPS_DENOMINATOR;

        // 4. Seller gets the rest (explicit guard for clarity, also caught by Solidity 0.8 checked math)
        if (royaltyAmount + stakerReward + seederReward > price) revert ExcessiveFees();
        uint256 sellerProceeds = price - royaltyAmount - stakerReward - seederReward;

        // Transfer token from seller to buyer
        contentToken.safeTransferFrom(seller, msg.sender, tokenId, 1, "");

        // Forward staker reward to staking contract (if stakers exist)
        if (stakerReward > 0) {
            if (staking.totalStaked() > 0) {
                staking.addReward{value: stakerReward}();
                totalStakerRewardsForwarded += stakerReward;
                emit StakerRewardForwarded(contentId, stakerReward);
            } else {
                // No stakers — add to seeder pool instead
                seederReward += stakerReward;
            }
        }

        // Set up reward tracking for this buyer (same mechanism as primary purchase)
        hasPurchased[contentId][msg.sender] = true;
        purchasers[contentId].push(msg.sender);
        buyerReward[contentId][msg.sender] = seederReward;

        // Pay creator royalty (split among collaborators if they exist)
        if (royaltyAmount > 0) {
            _payCreatorETH(contentId, royaltyAmount);
            totalCreatorPayments += royaltyAmount;
        }

        // Pay seller
        if (sellerProceeds > 0) {
            (bool sentSeller,) = payable(seller).call{value: sellerProceeds}("");
            if (!sentSeller) revert TransferFailed();
        }

        // Refund overpayment
        if (msg.value > price) {
            (bool refunded,) = payable(msg.sender).call{value: msg.value - price}("");
            if (!refunded) revert TransferFailed();
        }

        emit ResalePurchased(contentId, msg.sender, seller, price, royaltyAmount, seederReward);
    }

    // ============================================================
    //                     VIEW FUNCTIONS
    // ============================================================

    /// @notice Check if an address has purchased specific content
    function checkPurchase(bytes32 contentId, address buyer) external view returns (bool) {
        return hasPurchased[contentId][buyer];
    }

    /// @notice Get number of purchasers for a content
    function getPurchaserCount(bytes32 contentId) external view returns (uint256) {
        return purchasers[contentId].length;
    }

    /// @notice Get remaining (unclaimed) reward for a buyer's purchase
    function getBuyerReward(bytes32 contentId, address buyer) external view returns (uint256) {
        return buyerReward[contentId][buyer] - buyerRewardPaid[contentId][buyer];
    }

    // ============================================================
    //                     ADMIN
    // ============================================================

    /// @notice Update the creator share percentage
    function setCreatorShare(uint256 newCreatorShareBps) external onlyOwner {
        if (newCreatorShareBps > BPS_DENOMINATOR) revert InvalidCreatorShare();
        emit CreatorShareUpdated(creatorShareBps, newCreatorShareBps);
        creatorShareBps = newCreatorShareBps;
    }

    /// @notice Update the resale seeder reward percentage
    function setResaleRewardBps(uint256 newResaleRewardBps) external onlyOwner {
        if (newResaleRewardBps > BPS_DENOMINATOR) revert InvalidCreatorShare();
        resaleRewardBps = newResaleRewardBps;
    }

    /// @notice Update the staker reward percentage for primary purchases
    function setStakerRewardBps(uint256 newStakerRewardBps) external onlyOwner {
        if (newStakerRewardBps + creatorShareBps > BPS_DENOMINATOR) revert InvalidCreatorShare();
        stakerRewardBps = newStakerRewardBps;
    }

    /// @notice Update the staker reward percentage for resale purchases
    function setResaleStakerRewardBps(uint256 newBps) external onlyOwner {
        if (newBps > BPS_DENOMINATOR) revert InvalidCreatorShare();
        resaleStakerRewardBps = newBps;
    }

    /// @notice V2 initializer for upgrade — sets staker reward BPS
    function initializeV2(uint256 _stakerRewardBps, uint256 _resaleStakerRewardBps, uint256 _resaleRewardBps)
        external
        onlyOwner
        reinitializer(2)
    {
        if (_stakerRewardBps + creatorShareBps > BPS_DENOMINATOR) revert InvalidCreatorShare();
        if (_resaleStakerRewardBps + _resaleRewardBps > BPS_DENOMINATOR) revert InvalidCreatorShare();
        stakerRewardBps = _stakerRewardBps;
        resaleStakerRewardBps = _resaleStakerRewardBps;
        resaleRewardBps = _resaleRewardBps;
    }

    /// @notice Add or remove an ERC-20 token from the supported tokens whitelist
    function setSupportedToken(address token, bool supported) external onlyOwner {
        supportedTokens[token] = supported;
    }

    /// @notice Propose a new owner (two-step transfer)
    function transferOwnership(address newOwner) external onlyOwner {
        if (newOwner == address(0)) revert ZeroAddress();
        pendingOwner = newOwner;
    }

    /// @notice Accept ownership (must be called by the pending owner)
    function acceptOwnership() external {
        if (msg.sender != pendingOwner) revert OnlyOwner();
        owner = msg.sender;
        pendingOwner = address(0);
    }

    receive() external payable {}

    // ============================================================
    //                     INTERNAL HELPERS
    // (Identical to pre-ERC-1155 Marketplace — reward logic untouched)
    // ============================================================

    /// @dev Verify a delivery receipt and calculate the payout.
    ///      Returns 0 if invalid (signature mismatch, not purchased, already claimed, etc.).
    ///      Updates state (bytesClaimed, buyerRewardPaid) on success.
    function _verifyAndCalculateClaim(
        bytes32 contentId,
        address buyer,
        uint256 bytesServed,
        uint256 timestamp,
        bytes calldata signature
    ) internal returns (uint256) {
        // 1. Verify buyer signed this receipt (EIP-712)
        {
            bytes32 structHash = keccak256(abi.encode(RECEIPT_TYPE_HASH, contentId, msg.sender, bytesServed, timestamp));
            bytes32 digest = keccak256(abi.encodePacked("\x19\x01", DOMAIN_SEPARATOR, structHash));
            if (_ecrecover(digest, signature) != buyer) return 0;
        }

        // 2. Verify buyer purchased or tipped (tippers have buyerReward > 0)
        if (!hasPurchased[contentId][buyer] && buyerReward[contentId][buyer] == 0) return 0;

        // 3. bytesServed must be positive
        if (bytesServed == 0) return 0;

        // 4. Replay protection: one claim per (content, buyer, seeder)
        if (bytesClaimed[contentId][buyer][msg.sender] > 0) return 0;
        bytesClaimed[contentId][buyer][msg.sender] = bytesServed;

        // 4. Calculate proportional share and record payout
        return _calculateShare(contentId, buyer, bytesServed);
    }

    /// @dev Calculate the proportional reward share and update buyerRewardPaid.
    function _calculateShare(bytes32 contentId, address buyer, uint256 bytesServed) internal returns (uint256) {
        uint256 fSize = contentToken.getFileSize(contentId);
        if (fSize == 0) return 0;

        uint256 original = buyerReward[contentId][buyer];
        if (original == 0) return 0;

        uint256 share = (original * bytesServed) / fSize;
        uint256 remaining = original - buyerRewardPaid[contentId][buyer];
        if (share > remaining) share = remaining;
        if (share == 0) return 0;

        buyerRewardPaid[contentId][buyer] += share;
        return share;
    }

    /// @dev Pay the creator's ETH share, splitting among collaborators if they exist.
    ///      If no collaborators, pays the single creator address.
    function _payCreatorETH(bytes32 contentId, uint256 amount) internal {
        AraContent.Collaborator[] memory collabs = contentToken.getCollaborators(contentId);
        if (collabs.length == 0) {
            // Original path: single creator, no extra gas
            address creator = contentToken.getCreator(contentId);
            (bool sent,) = payable(creator).call{value: amount}("");
            if (!sent) revert TransferFailed();
        } else {
            uint256 totalPaid;
            for (uint256 i = 0; i < collabs.length; i++) {
                uint256 share;
                if (i == collabs.length - 1) {
                    share = amount - totalPaid; // dust goes to last collaborator
                } else {
                    share = (amount * collabs[i].shareBps) / BPS_DENOMINATOR;
                }
                (bool sent,) = payable(collabs[i].wallet).call{value: share}("");
                if (!sent) revert TransferFailed();
                totalPaid += share;
            }
        }
    }

    /// @dev Pay the creator's ERC-20 token share, splitting among collaborators if they exist.
    function _payCreatorToken(bytes32 contentId, address token, uint256 amount) internal {
        AraContent.Collaborator[] memory collabs = contentToken.getCollaborators(contentId);
        if (collabs.length == 0) {
            address creator = contentToken.getCreator(contentId);
            IERC20(token).safeTransfer(creator, amount);
        } else {
            uint256 totalPaid;
            for (uint256 i = 0; i < collabs.length; i++) {
                uint256 share;
                if (i == collabs.length - 1) {
                    share = amount - totalPaid;
                } else {
                    share = (amount * collabs[i].shareBps) / BPS_DENOMINATOR;
                }
                IERC20(token).safeTransfer(collabs[i].wallet, share);
                totalPaid += share;
            }
        }
    }

    /// @dev Recover the signer of an EIP-712 hash from a 65-byte signature.
    ///      Returns address(0) on invalid input.
    ///      Rejects malleable signatures per EIP-2 (s must be in lower half of secp256k1 order).
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
        // Reject malleable signatures: s must be in the lower half of secp256k1 order
        if (uint256(s) > 0x7FFFFFFFFFFFFFFFFFFFFFFFFFFFFFFF5D576E7357A4501DDFE92F46681B20A0) return address(0);
        // Some wallets return v as 0/1; normalize to 27/28
        if (v < 27) v += 27;
        if (v != 27 && v != 28) return address(0);
        return ecrecover(hash, v, r, s);
    }

    /// @dev Reserved storage for future upgrades
    uint256[50] private __gap;

    function _authorizeUpgrade(address) internal override onlyOwner {}
}
