// SPDX-License-Identifier: LGPL-3.0
pragma solidity ^0.8.24;

import {AraContent} from "./AraContent.sol";
import {AraStaking} from "./AraStaking.sol";
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
contract Marketplace is Initializable, UUPSUpgradeable {
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
    event RewardsClaimed(address indexed seeder, uint256 totalAmount, uint256 receiptCount);
    event CreatorShareUpdated(uint256 oldBps, uint256 newBps);
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
    /// @param contentId The content to purchase
    function purchase(bytes32 contentId) external payable {
        if (hasPurchased[contentId][msg.sender]) revert AlreadyPurchased();
        if (!contentToken.isActive(contentId)) revert ContentNotActive();

        uint256 price = contentToken.getPrice(contentId);
        if (msg.value < price) revert InsufficientPayment(msg.value, price);

        hasPurchased[contentId][msg.sender] = true;
        purchasers[contentId].push(msg.sender);

        // Split: creator gets their share, rest held for seeder claiming
        uint256 creatorPayment = (price * creatorShareBps) / BPS_DENOMINATOR;
        uint256 rewardAmount = price - creatorPayment;

        // Pay creator immediately
        address creator = contentToken.getCreator(contentId);
        (bool sent,) = payable(creator).call{value: creatorPayment}("");
        if (!sent) revert TransferFailed();

        totalCreatorPayments += creatorPayment;

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
    //                     REWARD CLAIMING
    // (Identical logic to pre-ERC-1155 Marketplace — untouched)
    // ============================================================

    /// @notice Claim reward for delivering content to a single buyer.
    ///         The calling seeder submits a buyer-signed delivery receipt.
    function claimDeliveryReward(
        bytes32 contentId,
        address buyer,
        uint256 bytesServed,
        uint256 timestamp,
        bytes calldata signature
    ) external {
        uint256 payout = _verifyAndCalculateClaim(contentId, buyer, bytesServed, timestamp, signature);
        if (payout == 0) revert NoRewardsToClaim();

        totalRewardsClaimed += payout;

        (bool sent,) = payable(msg.sender).call{value: payout}("");
        if (!sent) revert TransferFailed();

        emit DeliveryRewardClaimed(contentId, msg.sender, buyer, payout, bytesServed);
        emit RewardsClaimed(msg.sender, payout, 1);
    }

    /// @notice Batch claim: submit multiple delivery receipts in one transaction.
    ///         All receipts must be for the calling seeder (msg.sender).
    ///         Invalid or already-claimed receipts are silently skipped.
    function claimDeliveryRewards(ClaimParams[] calldata claims) external {
        uint256 totalPayout = 0;
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
                totalPayout += payout;
                validCount++;
                emit DeliveryRewardClaimed(
                    claims[i].contentId, msg.sender, claims[i].buyer, payout, claims[i].bytesServed
                );
            }
        }

        if (totalPayout == 0) revert NoRewardsToClaim();

        totalRewardsClaimed += totalPayout;

        (bool sent,) = payable(msg.sender).call{value: totalPayout}("");
        if (!sent) revert TransferFailed();

        emit RewardsClaimed(msg.sender, totalPayout, validCount);
    }

    // ============================================================
    //                     RESALE MARKETPLACE
    // ============================================================

    /// @notice List a content token for resale.
    ///         Seller must call contentToken.setApprovalForAll(marketplace, true) first.
    function listForResale(bytes32 contentId, uint256 price) external {
        if (contentToken.balanceOf(msg.sender, uint256(contentId)) == 0) revert NotTokenOwner();
        if (price == 0) revert InsufficientPayment(0, 1);
        listings[contentId][msg.sender] = Listing({price: price, active: true});
        emit ContentListed(contentId, msg.sender, price);
    }

    /// @notice Cancel a resale listing
    function cancelListing(bytes32 contentId) external {
        listings[contentId][msg.sender].active = false;
        emit ListingCancelled(contentId, msg.sender);
    }

    /// @notice Buy a content token from a reseller.
    ///         Payment split: creator royalty + seeder reward + seller proceeds.
    function buyResale(bytes32 contentId, address seller) external payable {
        Listing storage listing = listings[contentId][seller];
        if (!listing.active) revert NoActiveListing();
        if (msg.value < listing.price) revert InsufficientPayment(msg.value, listing.price);

        uint256 tokenId = uint256(contentId);

        // Verify seller still owns the token and has approved marketplace
        if (contentToken.balanceOf(seller, tokenId) == 0) revert NotTokenOwner();
        if (!contentToken.isApprovedForAll(seller, address(this))) revert NotApproved();

        uint256 price = listing.price;
        listing.active = false;

        // 1. Creator royalty (ERC-2981)
        (address royaltyReceiver, uint256 royaltyAmount) = contentToken.royaltyInfo(tokenId, price);

        // 2. Seeder reward
        uint256 seederReward = (price * resaleRewardBps) / BPS_DENOMINATOR;

        // 3. Seller gets the rest
        uint256 sellerProceeds = price - royaltyAmount - seederReward;

        // Transfer token from seller to buyer
        contentToken.safeTransferFrom(seller, msg.sender, tokenId, 1, "");

        // Set up reward tracking for this buyer (same mechanism as primary purchase)
        hasPurchased[contentId][msg.sender] = true;
        purchasers[contentId].push(msg.sender);
        buyerReward[contentId][msg.sender] = seederReward;

        // Pay creator royalty
        if (royaltyAmount > 0) {
            (bool sentRoyalty,) = payable(royaltyReceiver).call{value: royaltyAmount}("");
            if (!sentRoyalty) revert TransferFailed();
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

    /// @notice Update the resale reward percentage
    function setResaleRewardBps(uint256 newResaleRewardBps) external onlyOwner {
        if (newResaleRewardBps > BPS_DENOMINATOR) revert InvalidCreatorShare();
        resaleRewardBps = newResaleRewardBps;
    }

    /// @notice Transfer ownership
    function transferOwnership(address newOwner) external onlyOwner {
        owner = newOwner;
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

        // 2. Verify buyer actually purchased this content
        if (!hasPurchased[contentId][buyer]) return 0;

        // 3. Replay protection: one claim per (content, buyer, seeder)
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

    function _authorizeUpgrade(address) internal override onlyOwner {}
}
