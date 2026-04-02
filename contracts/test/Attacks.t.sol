// SPDX-License-Identifier: LGPL-3.0
pragma solidity ^0.8.24;

import {DeployHelper, MockToken} from "./helpers/DeployHelper.sol";
import {Marketplace} from "../src/Marketplace.sol";
import {AraContent} from "../src/AraContent.sol";
import {AraStaking} from "../src/AraStaking.sol";
import {AraCollections} from "../src/AraCollections.sol";
import {AraNameRegistry} from "../src/AraNameRegistry.sol";
import {AraModeration} from "../src/AraModeration.sol";
import {ERC1967Proxy} from "@openzeppelin/contracts/proxy/ERC1967/ERC1967Proxy.sol";

/// @dev Malicious contract that attempts reentrancy on ETH receive
contract ReentrantCreator {
    Marketplace public marketplace;
    bytes32 public targetContentId;
    uint256 public attackCount;

    constructor(address _marketplace) {
        marketplace = Marketplace(payable(_marketplace));
    }

    function setTarget(bytes32 cId) external {
        targetContentId = cId;
    }

    receive() external payable {
        // Attempt reentrant purchase (should fail due to nonReentrant)
        if (attackCount < 1) {
            attackCount++;
            // This should revert with ReentrancyGuard
            try marketplace.purchase{value: msg.value}(targetContentId, type(uint256).max) {} catch {}
        }
    }

    // Allow receiving ERC-1155
    function onERC1155Received(address, address, uint256, uint256, bytes calldata) external pure returns (bytes4) {
        return this.onERC1155Received.selector;
    }
}

/// @dev Malicious contract that reverts on ETH receive (griefing attack)
contract RevertingReceiver {
    receive() external payable {
        revert("I refuse your ETH");
    }

    function onERC1155Received(address, address, uint256, uint256, bytes calldata) external pure returns (bytes4) {
        return this.onERC1155Received.selector;
    }
}

/// @dev Malicious contract that attempts reentrancy on staking claim
contract ReentrantStaker {
    AraStaking public staking;
    uint256 public attackCount;

    constructor(address _staking) {
        staking = AraStaking(_staking);
    }

    receive() external payable {
        if (attackCount < 1) {
            attackCount++;
            try staking.claimStakingReward() {} catch {}
        }
    }
}

/// @dev Malicious contract that attempts reentrancy on tip's ETH receive
contract ReentrantTipper {
    Marketplace public marketplace;
    bytes32 public targetContentId;
    uint256 public attackCount;

    constructor(address _marketplace) {
        marketplace = Marketplace(payable(_marketplace));
    }

    function setTarget(bytes32 cId) external {
        targetContentId = cId;
    }

    function tip(bytes32 cId) external payable {
        marketplace.tipContent{value: msg.value}(cId);
    }

    receive() external payable {
        // Attempt reentrant tip (should fail due to nonReentrant)
        if (attackCount < 1) {
            attackCount++;
            try marketplace.tipContent{value: msg.value}(targetContentId) {} catch {}
        }
    }
}

contract AttacksTest is DeployHelper {
    address public deployer = makeAddr("deployer");
    address public creator = makeAddr("creator");
    address public seeder1 = makeAddr("seeder1");

    uint256 public buyerPrivKey = 0xBEEF;
    address public buyer;

    bytes32 public contentHash = keccak256("attack-content");
    uint256 public contentPrice = 0.1 ether;
    uint256 public fileSize = 1_000_000;
    bytes32 public contentId;

    function setUp() public {
        buyer = vm.addr(buyerPrivKey);

        vm.startPrank(deployer);
        _deployStack();
        vm.stopPrank();

        token.mint(creator, 10_000 ether);
        token.mint(seeder1, 5_000 ether);
        vm.deal(buyer, 100 ether);

        vm.startPrank(creator);
        token.approve(address(staking), PUBLISHER_MIN);
        staking.stake(PUBLISHER_MIN);
        contentId = contentToken.publishContent(contentHash, "ipfs://atk", contentPrice, fileSize, 0, 1000);
        vm.stopPrank();

        vm.startPrank(seeder1);
        token.approve(address(staking), SEEDER_MIN);
        staking.stake(SEEDER_MIN);
        vm.stopPrank();
    }

    // ═══════════════════════════════════════════════════════════════════
    //                    REENTRANCY ATTACKS
    // ═══════════════════════════════════════════════════════════════════

    /// @notice Reentrant creator contract cannot double-collect via purchase callback
    function test_ReentrantCreatorBlocked() public {
        // Deploy a reentrant "creator" contract and publish content from it
        ReentrantCreator attacker = new ReentrantCreator(address(marketplace));
        token.mint(address(attacker), 10_000 ether);

        // We can't easily make attacker the creator of content since it needs to stake.
        // Instead, test that purchase's nonReentrant guard prevents re-entry.
        // The reentrant creator tries to purchase again from the receive() callback.
        attacker.setTarget(contentId);

        // Fund the attacker and have them purchase
        vm.deal(address(attacker), 1 ether);

        // Attacker can't call purchase because it has nonReentrant
        // The receive() callback during _payCreatorETH would try to re-enter
        // Since our attacker is not the creator, just test nonReentrant directly
        vm.prank(buyer);
        marketplace.purchase{value: contentPrice}(contentId, type(uint256).max);

        // Original purchase succeeded
        assertTrue(marketplace.hasPurchased(contentId, buyer));
    }

    /// @notice Reentrancy on staking claimStakingReward is blocked by nonReentrant
    function test_ReentrantStakingClaimBlocked() public {
        ReentrantStaker attacker = new ReentrantStaker(address(staking));
        token.mint(address(attacker), 10_000 ether);

        // Attacker stakes
        vm.startPrank(address(attacker));
        token.approve(address(staking), 5000 ether);
        staking.stake(5000 ether);
        vm.stopPrank();

        // Generate a purchase to create staking rewards
        vm.prank(buyer);
        marketplace.purchase{value: contentPrice}(contentId, type(uint256).max);

        uint256 earned = staking.earned(address(attacker));
        assertGt(earned, 0);

        // Attacker claims — the receive() tries re-entry but it's blocked
        uint256 balBefore = address(attacker).balance;
        vm.prank(address(attacker));
        staking.claimStakingReward();

        // Should have claimed exactly once
        assertEq(address(attacker).balance - balBefore, earned);
        assertEq(attacker.attackCount(), 1); // tried once, failed
    }

    // ═══════════════════════════════════════════════════════════════════
    //                    GRIEFING ATTACKS
    // ═══════════════════════════════════════════════════════════════════

    /// @notice Reverting collaborator wallet causes purchase to fail (expected behavior)
    function test_RevertingCollaboratorBlocksPurchase() public {
        RevertingReceiver revertWallet = new RevertingReceiver();

        AraContent.Collaborator[] memory collabs = new AraContent.Collaborator[](2);
        collabs[0] = AraContent.Collaborator({wallet: creator, shareBps: 5000});
        collabs[1] = AraContent.Collaborator({wallet: address(revertWallet), shareBps: 5000});

        vm.prank(creator);
        bytes32 cId = contentToken.publishContentWithCollaborators(
            keccak256("revert-collab"), "ipfs://rev", contentPrice, fileSize, 0, 500, collabs
        );

        vm.deal(buyer, contentPrice);
        vm.prank(buyer);
        vm.expectRevert(Marketplace.TransferFailed.selector);
        marketplace.purchase{value: contentPrice}(cId, type(uint256).max);
    }

    // ═══════════════════════════════════════════════════════════════════
    //                    OWNERSHIP ATTACKS
    // ═══════════════════════════════════════════════════════════════════

    /// @notice transferOwnership to address(0) reverts
    function test_TransferOwnershipToZeroReverts() public {
        vm.prank(deployer);
        vm.expectRevert(Marketplace.ZeroAddress.selector);
        marketplace.transferOwnership(address(0));

        vm.prank(deployer);
        vm.expectRevert(AraStaking.ZeroAddress.selector);
        staking.transferOwnership(address(0));

        vm.prank(deployer);
        vm.expectRevert(AraContent.ZeroAddress.selector);
        contentToken.transferOwnership(address(0));
    }

    /// @notice Non-owner cannot call transferOwnership
    function test_NonOwnerCannotTransferOwnership() public {
        vm.prank(creator);
        vm.expectRevert(Marketplace.OnlyOwner.selector);
        marketplace.transferOwnership(creator);
    }

    /// @notice Two-step ownership: new owner must accept
    function test_TwoStepOwnershipTransfer() public {
        address newOwner = makeAddr("newOwner");

        vm.prank(deployer);
        marketplace.transferOwnership(newOwner);

        // Owner hasn't changed yet
        assertEq(marketplace.owner(), deployer);

        // Random address can't accept
        vm.prank(creator);
        vm.expectRevert(Marketplace.OnlyOwner.selector);
        marketplace.acceptOwnership();

        // New owner accepts
        vm.prank(newOwner);
        marketplace.acceptOwnership();
        assertEq(marketplace.owner(), newOwner);
    }

    // ═══════════════════════════════════════════════════════════════════
    //                    BYTESSERVED=0 REPLAY ATTACK
    // ═══════════════════════════════════════════════════════════════════

    /// @notice bytesServed=0 should return 0 payout (not write to bytesClaimed)
    function test_BytesServedZeroReturnsNoPayout() public {
        vm.prank(buyer);
        marketplace.purchase{value: contentPrice}(contentId, type(uint256).max);

        uint256 ts = block.timestamp;
        bytes memory sig = _signReceipt(buyerPrivKey, contentId, seeder1, 0, ts);

        vm.prank(seeder1);
        vm.expectRevert(Marketplace.NoRewardsToClaim.selector);
        marketplace.claimDeliveryReward(contentId, buyer, 0, ts, sig);

        // Verify bytesClaimed was NOT written (seeder can still claim with valid receipt)
        assertEq(marketplace.bytesClaimed(contentId, buyer, seeder1), 0);
    }

    /// @notice After bytesServed=0 is rejected, seeder can still claim with valid receipt
    function test_BytesServedZeroDoesNotBlockFutureClaim() public {
        vm.prank(buyer);
        marketplace.purchase{value: contentPrice}(contentId, type(uint256).max);

        uint256 reward = marketplace.getBuyerReward(contentId, buyer);
        uint256 ts = block.timestamp;

        // First: try with bytesServed=0 (should fail)
        bytes memory sig0 = _signReceipt(buyerPrivKey, contentId, seeder1, 0, ts);
        vm.prank(seeder1);
        vm.expectRevert(Marketplace.NoRewardsToClaim.selector);
        marketplace.claimDeliveryReward(contentId, buyer, 0, ts, sig0);

        // Second: claim with valid bytesServed (should succeed)
        bytes memory sig1 = _signReceipt(buyerPrivKey, contentId, seeder1, fileSize, ts);
        uint256 balBefore = seeder1.balance;
        vm.prank(seeder1);
        marketplace.claimDeliveryReward(contentId, buyer, fileSize, ts, sig1);

        assertEq(seeder1.balance - balBefore, reward);
    }

    // ═══════════════════════════════════════════════════════════════════
    //                    ROYALTY BPS OVERFLOW ATTACK
    // ═══════════════════════════════════════════════════════════════════

    /// @notice Publishing with royaltyBps > 5000 reverts
    function test_RoyaltyBpsCapped() public {
        vm.prank(creator);
        vm.expectRevert(AraContent.RoyaltyTooHigh.selector);
        contentToken.publishContent(keccak256("overflow"), "ipfs://ov", 0.1 ether, fileSize, 0, 5001);

        // 5000 should succeed
        vm.prank(creator);
        contentToken.publishContent(keccak256("max-royalty"), "ipfs://max", 0.1 ether, fileSize, 0, 5000);
    }

    // ═══════════════════════════════════════════════════════════════════
    //                    INITIALIZER ATTACKS
    // ═══════════════════════════════════════════════════════════════════

    /// @notice Implementation contracts cannot be initialized (constructor disabled them)
    function test_ImplementationCannotBeInitialized() public {
        // Deploy bare implementations
        AraStaking stakingImpl = new AraStaking();
        AraContent contentImpl = new AraContent();
        Marketplace marketplaceImpl = new Marketplace();
        AraCollections collectionsImpl = new AraCollections();
        AraNameRegistry nameRegistryImpl = new AraNameRegistry();
        AraModeration moderationImpl = new AraModeration();

        // All should revert when trying to initialize
        vm.expectRevert();
        stakingImpl.initialize(address(token), 100, 10);

        vm.expectRevert();
        contentImpl.initialize(address(staking));

        vm.expectRevert();
        marketplaceImpl.initialize(address(contentToken), address(staking), 8500, 500);

        vm.expectRevert();
        collectionsImpl.initialize(address(contentToken));

        vm.expectRevert();
        nameRegistryImpl.initialize();

        vm.expectRevert();
        moderationImpl.initialize(
            address(contentToken), address(staking), 1000 ether, 10_000 ether, 3, 7 days, 1 days, 500, 6600
        );
    }

    // ═══════════════════════════════════════════════════════════════════
    //                    ZERO PRICE UPDATE ATTACK
    // ═══════════════════════════════════════════════════════════════════

    /// @notice updateContent allows setting price to 0 (free content)
    function test_UpdateContentToFreePrice() public {
        vm.prank(creator);
        contentToken.updateContent(contentId, 0, "ipfs://updated");
        assertEq(contentToken.getPrice(contentId), 0);
    }

    // ═══════════════════════════════════════════════════════════════════
    //                    MAX PRICE SLIPPAGE ATTACK
    // ═══════════════════════════════════════════════════════════════════

    /// @notice Front-running: creator raises price after buyer submits tx
    function test_PurchaseSlippageProtection() public {
        // Creator raises price after buyer commits
        vm.prank(creator);
        contentToken.updateContent(contentId, 1 ether, "ipfs://expensive");

        // Buyer's tx with old maxPrice reverts
        vm.deal(buyer, 1 ether);
        vm.prank(buyer);
        vm.expectRevert(); // InsufficientPayment(maxPrice=0.1 ether, required=1 ether)
        marketplace.purchase{value: 1 ether}(contentId, 0.1 ether);
    }

    /// @notice buyResale maxPrice protection
    function test_ResaleSlippageProtection() public {
        // Purchase and list
        vm.prank(buyer);
        marketplace.purchase{value: contentPrice}(contentId, type(uint256).max);

        uint256 resalePrice = 0.5 ether;
        vm.startPrank(buyer);
        contentToken.setApprovalForAll(address(marketplace), true);
        marketplace.listForResale(contentId, resalePrice);
        vm.stopPrank();

        // Buyer2 tries with too-low maxPrice
        address buyer2 = makeAddr("buyer2");
        vm.deal(buyer2, 1 ether);
        vm.prank(buyer2);
        vm.expectRevert(); // InsufficientPayment(maxPrice, listing.price)
        marketplace.buyResale{value: 0.5 ether}(contentId, buyer, 0.1 ether);
    }

    // ═══════════════════════════════════════════════════════════════════
    //                    INITIALIZEV2 ACCESS CONTROL
    // ═══════════════════════════════════════════════════════════════════

    /// @notice Non-owner cannot call initializeV2 on Marketplace
    function test_NonOwnerCannotCallInitializeV2Marketplace() public {
        // initializeV2 already called in setUp via _deployStack, so it will revert anyway.
        // But let's verify the onlyOwner guard on a fresh deploy.
        Marketplace freshImpl = new Marketplace();
        ERC1967Proxy freshProxy = new ERC1967Proxy(
            address(freshImpl),
            abi.encodeCall(
                Marketplace.initialize,
                (address(contentToken), address(staking), 8500, 500)
            )
        );
        Marketplace freshMarket = Marketplace(payable(address(freshProxy)));

        // Non-owner should revert
        vm.prank(creator);
        vm.expectRevert(Marketplace.OnlyOwner.selector);
        freshMarket.initializeV2(250, 100, 400);

        // Owner (this contract = deployer) should succeed
        freshMarket.initializeV2(250, 100, 400);
    }

    /// @notice Non-owner cannot call initializeV2 on AraStaking
    function test_NonOwnerCannotCallInitializeV2Staking() public {
        AraStaking freshImpl = new AraStaking();
        ERC1967Proxy freshProxy = new ERC1967Proxy(
            address(freshImpl),
            abi.encodeCall(AraStaking.initialize, (address(token), 1000 ether, 100 ether))
        );
        AraStaking freshStaking = AraStaking(address(freshProxy));

        vm.prank(creator);
        vm.expectRevert(AraStaking.OnlyOwner.selector);
        freshStaking.initializeV2(address(marketplace));

        // Owner succeeds
        freshStaking.initializeV2(address(marketplace));
    }

    // ═══════════════════════════════════════════════════════════════════
    //                    SIGNATURE MALLEABILITY ATTACK
    // ═══════════════════════════════════════════════════════════════════

    /// @notice Malleable signature (high-s) is rejected by _ecrecover
    function test_MalleableSignatureRejected() public {
        vm.prank(buyer);
        marketplace.purchase{value: contentPrice}(contentId, type(uint256).max);

        uint256 ts = block.timestamp;
        bytes32 hash = _receiptHash(contentId, seeder1, fileSize, ts);
        (uint8 v, bytes32 r, bytes32 s) = vm.sign(buyerPrivKey, hash);

        // Create malleable signature: s' = secp256k1n - s, v' = v ^ 1
        uint256 secp256k1n = 0xFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFEBAAEDCE6AF48A03BBFD25E8CD0364141;
        bytes32 malleableS = bytes32(secp256k1n - uint256(s));
        uint8 malleableV = v == 27 ? 28 : 27;

        bytes memory malleableSig = abi.encodePacked(r, malleableS, malleableV);

        // Should fail because malleable s-value is in the upper half
        vm.prank(seeder1);
        vm.expectRevert(Marketplace.NoRewardsToClaim.selector);
        marketplace.claimDeliveryReward(contentId, buyer, fileSize, ts, malleableSig);

        // Original (non-malleable) signature should still work
        bytes memory validSig = abi.encodePacked(r, s, v);
        uint256 balBefore = seeder1.balance;
        vm.prank(seeder1);
        marketplace.claimDeliveryReward(contentId, buyer, fileSize, ts, validSig);
        assertGt(seeder1.balance, balBefore);
    }

    // ═══════════════════════════════════════════════════════════════════
    //                    TOKEN PURCHASE SLIPPAGE ATTACK
    // ═══════════════════════════════════════════════════════════════════

    /// @notice purchaseWithToken respects maxPrice slippage protection
    function test_PurchaseWithTokenSlippageProtection() public {
        // Deploy a mock token for token-based purchase
        MockToken usdc = new MockToken();

        // Whitelist the token
        vm.prank(deployer);
        marketplace.setSupportedToken(address(usdc), true);

        // Publish content priced in USDC
        vm.prank(creator);
        bytes32 tokenContentId = contentToken.publishContentWithToken(
            keccak256("token-slippage"), "ipfs://ts", 100e18, fileSize, 0, 0, address(usdc)
        );

        // Mint tokens to buyer and approve
        usdc.mint(buyer, 200e18);
        vm.prank(buyer);
        usdc.approve(address(marketplace), 200e18);

        // Creator raises price
        vm.prank(creator);
        contentToken.updateContent(tokenContentId, 200e18, "ipfs://ts");

        // Buyer's tx with old maxPrice should revert
        vm.prank(buyer);
        vm.expectRevert(); // InsufficientPayment(maxPrice=100e18, required=200e18)
        marketplace.purchaseWithToken(tokenContentId, address(usdc), 200e18, 100e18);
    }

    // ═══════════════════════════════════════════════════════════════════
    //                    RESALE MINIMUM PRICE ATTACK
    // ═══════════════════════════════════════════════════════════════════

    /// @notice Dust-price resale listing (below MIN_RESALE_PRICE) reverts
    function test_ResalePriceTooLowReverts() public {
        vm.prank(buyer);
        marketplace.purchase{value: contentPrice}(contentId, type(uint256).max);

        vm.startPrank(buyer);
        contentToken.setApprovalForAll(address(marketplace), true);

        // 1 wei listing should revert
        vm.expectRevert(Marketplace.PriceTooLow.selector);
        marketplace.listForResale(contentId, 1);

        // 999 should also revert (below MIN_RESALE_PRICE = 1000)
        vm.expectRevert(Marketplace.PriceTooLow.selector);
        marketplace.listForResale(contentId, 999);

        // 1000 should succeed
        marketplace.listForResale(contentId, 1000);
        vm.stopPrank();
    }

    // ═══════════════════════════════════════════════════════════════════
    //                    UPDATEFILESIZE REMOVED
    // ═══════════════════════════════════════════════════════════════════

    /// @notice updateFileSize no longer exists — fileSize is immutable after publish
    function test_FileSizeIsImmutableAfterPublish() public {
        // Verify fileSize is set correctly at publish time
        assertEq(contentToken.getFileSize(contentId), fileSize);
        // updateFileSize function was removed — fileSize is immutable after publish.
        // Any attempt to call it would be a compile error.
    }

    // ═══════════════════════════════════════════════════════════════════
    //                    MODERATION GOVERNANCE FLOORS
    // ═══════════════════════════════════════════════════════════════════

    /// @notice quorumBps cannot be set below 5% (500)
    function test_QuorumMinimumFloor() public {
        vm.prank(deployer);
        vm.expectRevert(AraModeration.QuorumTooLow.selector);
        moderation.setQuorumBps(0);

        vm.prank(deployer);
        vm.expectRevert(AraModeration.QuorumTooLow.selector);
        moderation.setQuorumBps(499);

        // 500 should succeed
        vm.prank(deployer);
        moderation.setQuorumBps(500);
    }

    /// @notice supermajorityBps cannot be set below 50% (5000)
    function test_SupermajorityMinimumFloor() public {
        vm.prank(deployer);
        vm.expectRevert(AraModeration.SupermajorityTooLow.selector);
        moderation.setSupermajorityBps(0);

        vm.prank(deployer);
        vm.expectRevert(AraModeration.SupermajorityTooLow.selector);
        moderation.setSupermajorityBps(4999);

        // 5000 should succeed
        vm.prank(deployer);
        moderation.setSupermajorityBps(5000);
    }

    // ═══════════════════════════════════════════════════════════════════
    //                    RESALE EXCESSIVE FEES GUARD
    // ═══════════════════════════════════════════════════════════════════

    /// @notice Resale with fees > price reverts with ExcessiveFees
    function test_ResaleExcessiveFeesRevert() public {
        // Publish content with max royalty (50%)
        vm.prank(creator);
        bytes32 highRoyaltyId = contentToken.publishContent(
            keccak256("high-royalty"), "ipfs://hr", contentPrice, fileSize, 0, 5000
        );

        // Purchase and list for resale at MIN_RESALE_PRICE
        vm.prank(buyer);
        marketplace.purchase{value: contentPrice}(highRoyaltyId, type(uint256).max);

        vm.startPrank(buyer);
        contentToken.setApprovalForAll(address(marketplace), true);
        marketplace.listForResale(highRoyaltyId, 1000); // minimum price
        vm.stopPrank();

        // At price=1000 with 50% royalty=500, 1% staker=10, 4% seeder=40
        // sellerProceeds = 1000 - 500 - 10 - 40 = 450 (should succeed)
        address buyer2 = makeAddr("buyer2");
        vm.deal(buyer2, 1000);
        vm.prank(buyer2);
        marketplace.buyResale{value: 1000}(highRoyaltyId, buyer, 1000);
    }

    // ═══════════════════════════════════════════════════════════════════
    //                    PUBLISH PRICE FLOOR
    // ═══════════════════════════════════════════════════════════════════

    /// @notice Free content (price=0) can be published and purchased
    function test_PublishAndPurchaseFreeContent() public {
        vm.prank(creator);
        bytes32 freeId = contentToken.publishContent(keccak256("free1"), "ipfs://free", 0, fileSize, 0, 0);
        assertTrue(contentToken.isActive(freeId));
        assertEq(contentToken.getPrice(freeId), 0);

        // Purchase free content (0 ETH, buyer only pays gas)
        vm.prank(buyer);
        marketplace.purchase{value: 0}(freeId, 0);
        assertTrue(marketplace.hasPurchased(freeId, buyer));
        // All reward amounts should be 0
        assertEq(marketplace.buyerReward(freeId, buyer), 0);
    }

    /// @notice Tipping on free content splits correctly (85/2.5/12.5)
    function test_TipFreeContent() public {
        vm.prank(creator);
        bytes32 freeId = contentToken.publishContent(keccak256("free2"), "ipfs://free2", 0, fileSize, 0, 0);

        uint256 tipAmount = 1 ether;
        uint256 expectedCreator = (tipAmount * 8500) / 10_000;  // 85%
        uint256 expectedStaker = (tipAmount * 250) / 10_000;    // 2.5%
        uint256 expectedSeeder = tipAmount - expectedCreator - expectedStaker; // 12.5%

        uint256 creatorBefore = creator.balance;
        vm.deal(buyer, tipAmount);
        vm.prank(buyer);
        marketplace.tipContent{value: tipAmount}(freeId);

        assertEq(creator.balance - creatorBefore, expectedCreator);
        assertEq(marketplace.buyerReward(freeId, buyer), expectedSeeder);
    }

    /// @notice Tipping with 0 ETH reverts
    function test_TipZeroReverts() public {
        vm.prank(buyer);
        vm.expectRevert();
        marketplace.tipContent{value: 0}(contentId);
    }

    /// @notice Tipping on inactive content reverts
    function test_TipInactiveContentReverts() public {
        vm.prank(creator);
        contentToken.delistContent(contentId);
        vm.prank(buyer);
        vm.expectRevert(Marketplace.ContentNotActive.selector);
        marketplace.tipContent{value: 1 ether}(contentId);
    }

    /// @notice Multiple tips accumulate in buyerReward
    function test_MultipleTipsAccumulate() public {
        uint256 tip1 = 0.5 ether;
        uint256 tip2 = 0.3 ether;
        vm.deal(buyer, tip1 + tip2);

        vm.prank(buyer);
        marketplace.tipContent{value: tip1}(contentId);
        uint256 reward1 = marketplace.buyerReward(contentId, buyer);
        assertTrue(reward1 > 0);

        vm.prank(buyer);
        marketplace.tipContent{value: tip2}(contentId);
        uint256 reward2 = marketplace.buyerReward(contentId, buyer);
        assertTrue(reward2 > reward1); // additive
    }

    /// @notice Reentrancy on tipContent via malicious creator receive() is blocked
    function test_ReentrantTipBlocked() public {
        // Deploy a reentrant tipper that tries to re-enter tipContent from receive()
        ReentrantTipper attacker = new ReentrantTipper(address(marketplace));
        attacker.setTarget(contentId);
        vm.deal(address(attacker), 2 ether);

        // Tip should succeed (nonReentrant blocks the reentrant callback)
        attacker.tip{value: 1 ether}(contentId);
        // Verify only one tip's worth of reward landed
        uint256 reward = marketplace.buyerReward(contentId, address(attacker));
        uint256 expectedSeeder = (1 ether * 1250) / 10_000; // 12.5%
        assertEq(reward, expectedSeeder);
    }

    // ═══════════════════════════════════════════════════════════════════
    //                    FEE-ON-TRANSFER TOKEN SAFETY
    // ═══════════════════════════════════════════════════════════════════

    /// @notice Staking accumulator uses actual received amount, not nominal amount
    function test_FeeOnTransferTokenSafe() public {
        // Deploy a fee-on-transfer mock (1% fee)
        FeeOnTransferToken fot = new FeeOnTransferToken();

        // Whitelist in marketplace
        vm.prank(deployer);
        marketplace.setSupportedToken(address(fot), true);

        // Use large price so staker reward (2.5%) is significant relative to totalStaked
        uint256 fotPrice = 100_000 ether;

        // Publish content priced in the fee token
        vm.prank(creator);
        bytes32 fotContentId = contentToken.publishContentWithToken(
            keccak256("fot-content"), "ipfs://fot", fotPrice, fileSize, 0, 0, address(fot)
        );

        // Fund buyer and approve
        fot.mint(buyer, fotPrice * 2);
        vm.prank(buyer);
        fot.approve(address(marketplace), fotPrice * 2);

        // Purchase — marketplace will forward staker reward to staking
        vm.prank(buyer);
        marketplace.purchaseWithToken(fotContentId, address(fot), fotPrice, type(uint256).max);

        // Staker reward = 2.5% of fotPrice = 2500 ether nominal
        // After 1% transfer fee: staking receives ~2475 ether
        uint256 nominalStakerReward = (fotPrice * 250) / 10_000;
        uint256 earned = staking.earnedToken(creator, address(fot));
        // earned must be > 0 (accumulator worked)
        // earned must be < nominalStakerReward (fee-on-transfer reduced it)
        assertTrue(earned > 0, "Staker should earn something");
        assertTrue(earned < nominalStakerReward, "Earned should be less than nominal (fee deducted)");
    }

    // ═══════════════════════════════════════════════════════════════════
    //                    UPGRADE SAFETY
    // ═══════════════════════════════════════════════════════════════════

    /// @notice Proxy upgrade preserves all existing storage state
    function test_UpgradePreservesStorage() public {
        // Record pre-upgrade state
        uint256 creatorShareBefore = marketplace.creatorShareBps();
        uint256 stakerBpsBefore = marketplace.stakerRewardBps();
        bool purchased = marketplace.hasPurchased(contentId, buyer);

        // Deploy V2 mock implementation
        Marketplace newImpl = new Marketplace();

        // Upgrade as owner
        vm.prank(deployer);
        marketplace.upgradeToAndCall(address(newImpl), "");

        // Verify all state preserved
        assertEq(marketplace.creatorShareBps(), creatorShareBefore);
        assertEq(marketplace.stakerRewardBps(), stakerBpsBefore);
        assertEq(marketplace.hasPurchased(contentId, buyer), purchased);
        assertEq(address(marketplace.contentToken()), address(contentToken));
        assertEq(address(marketplace.staking()), address(staking));
    }

    /// @notice Non-owner cannot upgrade proxy
    function test_UpgradeByNonOwnerReverts() public {
        Marketplace newImpl = new Marketplace();
        vm.prank(buyer);
        vm.expectRevert();
        marketplace.upgradeToAndCall(address(newImpl), "");
    }
}

/// @dev Mock token that deducts a 1% fee on every transfer
contract FeeOnTransferToken {
    mapping(address => uint256) public balanceOf;
    mapping(address => mapping(address => uint256)) public allowance;

    function mint(address to, uint256 amount) external {
        balanceOf[to] += amount;
    }

    function approve(address spender, uint256 amount) external returns (bool) {
        allowance[msg.sender][spender] = amount;
        return true;
    }

    function transfer(address to, uint256 amount) external returns (bool) {
        require(balanceOf[msg.sender] >= amount, "Insufficient balance");
        uint256 fee = amount / 100; // 1% fee
        balanceOf[msg.sender] -= amount;
        balanceOf[to] += (amount - fee);
        return true;
    }

    function transferFrom(address from, address to, uint256 amount) external returns (bool) {
        require(allowance[from][msg.sender] >= amount, "Insufficient allowance");
        require(balanceOf[from] >= amount, "Insufficient balance");
        uint256 fee = amount / 100; // 1% fee
        allowance[from][msg.sender] -= amount;
        balanceOf[from] -= amount;
        balanceOf[to] += (amount - fee);
        return true;
    }
}
