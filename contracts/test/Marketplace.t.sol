// SPDX-License-Identifier: LGPL-3.0
pragma solidity ^0.8.24;

import {DeployHelper} from "./helpers/DeployHelper.sol";
import {Marketplace} from "../src/Marketplace.sol";
import {AraContent} from "../src/AraContent.sol";

contract MarketplaceTest is DeployHelper {
    address public deployer = makeAddr("deployer");
    address public creator = makeAddr("creator");
    address public seeder1 = makeAddr("seeder1");
    address public seeder2 = makeAddr("seeder2");

    // Buyer with known private key (needed for EIP-712 signatures)
    uint256 public buyerPrivKey = 0xBEEF;
    address public buyer;

    bytes32 public contentHash = keccak256("game-file-data");
    string public metadataURI = "ipfs://QmTest123";
    uint256 public contentPrice = 0.1 ether;
    uint256 public fileSize = 1_000_000; // 1 MB
    bytes32 public contentId;

    function setUp() public {
        buyer = vm.addr(buyerPrivKey);

        vm.startPrank(deployer);
        _deployStack();
        vm.stopPrank();

        // Fund all participants
        token.mint(creator, 10_000 ether);
        token.mint(seeder1, 5_000 ether);
        token.mint(seeder2, 5_000 ether);
        vm.deal(buyer, 10 ether);

        // Creator stakes and publishes (unlimited edition, 10% royalty)
        vm.startPrank(creator);
        token.approve(address(staking), PUBLISHER_MIN);
        staking.stake(PUBLISHER_MIN);
        contentId = contentToken.publishContent(contentHash, metadataURI, contentPrice, fileSize, 0, 1000);
        vm.stopPrank();

        // Seeders stake for the content
        vm.startPrank(seeder1);
        token.approve(address(staking), SEEDER_MIN);
        staking.stake(SEEDER_MIN);
        staking.stakeForContent(contentId, SEEDER_MIN);
        vm.stopPrank();

        vm.startPrank(seeder2);
        token.approve(address(staking), SEEDER_MIN);
        staking.stake(SEEDER_MIN);
        staking.stakeForContent(contentId, SEEDER_MIN);
        vm.stopPrank();
    }

    // ======== Purchase tests ========

    function test_Purchase() public {
        uint256 creatorBalanceBefore = creator.balance;

        vm.prank(buyer);
        marketplace.purchase{value: contentPrice}(contentId);

        assertTrue(marketplace.hasPurchased(contentId, buyer));
        assertEq(marketplace.getPurchaserCount(contentId), 1);

        // Creator should receive 85%
        uint256 expectedCreatorPayment = (contentPrice * CREATOR_SHARE_BPS) / 10_000;
        assertEq(creator.balance - creatorBalanceBefore, expectedCreatorPayment);

        // Buyer reward should have 15%
        uint256 expectedReward = contentPrice - expectedCreatorPayment;
        assertEq(marketplace.getBuyerReward(contentId, buyer), expectedReward);
    }

    function test_PurchaseMintsERC1155Token() public {
        vm.prank(buyer);
        marketplace.purchase{value: contentPrice}(contentId);

        // Buyer should now hold 1 ERC-1155 token
        assertEq(contentToken.balanceOf(buyer, uint256(contentId)), 1);
        assertEq(contentToken.getTotalMinted(contentId), 1);
    }

    function test_RevertPurchaseInsufficientPayment() public {
        vm.prank(buyer);
        vm.expectRevert();
        marketplace.purchase{value: 0.01 ether}(contentId);
    }

    function test_RevertDoublePurchase() public {
        vm.startPrank(buyer);
        marketplace.purchase{value: contentPrice}(contentId);
        vm.expectRevert();
        marketplace.purchase{value: contentPrice}(contentId);
        vm.stopPrank();
    }

    function test_RevertPurchaseDelistedContent() public {
        vm.prank(creator);
        contentToken.delistContent(contentId);

        vm.prank(buyer);
        vm.expectRevert();
        marketplace.purchase{value: contentPrice}(contentId);
    }

    function test_OverpaymentRefund() public {
        uint256 overpayment = 1 ether;
        uint256 buyerBalanceBefore = buyer.balance;

        vm.prank(buyer);
        marketplace.purchase{value: overpayment}(contentId);

        assertEq(buyerBalanceBefore - buyer.balance, contentPrice);
    }

    // ======== Single claim tests (reward logic UNCHANGED) ========

    function test_ClaimDeliveryReward() public {
        vm.prank(buyer);
        marketplace.purchase{value: contentPrice}(contentId);

        uint256 rewardBefore = marketplace.getBuyerReward(contentId, buyer);
        assertTrue(rewardBefore > 0);

        uint256 ts = block.timestamp;
        bytes memory sig = _signReceipt(buyerPrivKey, contentId, seeder1, fileSize, ts);

        uint256 seeder1BalBefore = seeder1.balance;

        vm.prank(seeder1);
        marketplace.claimDeliveryReward(contentId, buyer, fileSize, ts, sig);

        assertEq(seeder1.balance - seeder1BalBefore, rewardBefore);
        assertEq(marketplace.getBuyerReward(contentId, buyer), 0);
    }

    function test_ClaimDeliveryRewardPartialBytes() public {
        vm.prank(buyer);
        marketplace.purchase{value: contentPrice}(contentId);

        uint256 rewardBefore = marketplace.getBuyerReward(contentId, buyer);
        uint256 halfFileSize = fileSize / 2;

        uint256 ts = block.timestamp;
        bytes memory sig = _signReceipt(buyerPrivKey, contentId, seeder1, halfFileSize, ts);

        uint256 seeder1BalBefore = seeder1.balance;

        vm.prank(seeder1);
        marketplace.claimDeliveryReward(contentId, buyer, halfFileSize, ts, sig);

        uint256 expectedPayout = (rewardBefore * halfFileSize) / fileSize;
        assertEq(seeder1.balance - seeder1BalBefore, expectedPayout);
        assertEq(marketplace.getBuyerReward(contentId, buyer), rewardBefore - expectedPayout);
    }

    function test_ClaimRevertInvalidSignature() public {
        vm.prank(buyer);
        marketplace.purchase{value: contentPrice}(contentId);

        uint256 wrongKey = 0xDEAD;
        uint256 ts = block.timestamp;
        bytes memory sig = _signReceipt(wrongKey, contentId, seeder1, fileSize, ts);

        vm.prank(seeder1);
        vm.expectRevert(Marketplace.NoRewardsToClaim.selector);
        marketplace.claimDeliveryReward(contentId, buyer, fileSize, ts, sig);
    }

    function test_ClaimRevertNotPurchased() public {
        uint256 fakeBuyerKey = 0xCAFE;
        uint256 ts = block.timestamp;
        bytes memory sig = _signReceipt(fakeBuyerKey, contentId, seeder1, fileSize, ts);

        vm.prank(seeder1);
        vm.expectRevert(Marketplace.NoRewardsToClaim.selector);
        marketplace.claimDeliveryReward(contentId, vm.addr(fakeBuyerKey), fileSize, ts, sig);
    }

    function test_ClaimReplayProtection() public {
        vm.prank(buyer);
        marketplace.purchase{value: contentPrice}(contentId);

        uint256 ts = block.timestamp;
        bytes memory sig = _signReceipt(buyerPrivKey, contentId, seeder1, fileSize, ts);

        vm.prank(seeder1);
        marketplace.claimDeliveryReward(contentId, buyer, fileSize, ts, sig);

        vm.prank(seeder1);
        vm.expectRevert(Marketplace.NoRewardsToClaim.selector);
        marketplace.claimDeliveryReward(contentId, buyer, fileSize, ts, sig);
    }

    // ======== Multi-seeder proportional claiming ========

    function test_TwoSeedersProportionalClaim() public {
        vm.prank(buyer);
        marketplace.purchase{value: contentPrice}(contentId);

        uint256 originalReward = marketplace.getBuyerReward(contentId, buyer);
        uint256 ts = block.timestamp;

        uint256 served1 = 600_000;
        bytes memory sig1 = _signReceipt(buyerPrivKey, contentId, seeder1, served1, ts);

        uint256 served2 = 400_000;
        bytes memory sig2 = _signReceipt(buyerPrivKey, contentId, seeder2, served2, ts);

        uint256 seeder1BalBefore = seeder1.balance;
        vm.prank(seeder1);
        marketplace.claimDeliveryReward(contentId, buyer, served1, ts, sig1);
        uint256 seeder1Payout = seeder1.balance - seeder1BalBefore;

        uint256 seeder2BalBefore = seeder2.balance;
        vm.prank(seeder2);
        marketplace.claimDeliveryReward(contentId, buyer, served2, ts, sig2);
        uint256 seeder2Payout = seeder2.balance - seeder2BalBefore;

        uint256 expectedSeeder1 = (originalReward * served1) / fileSize;
        uint256 expectedSeeder2 = (originalReward * served2) / fileSize;

        assertEq(seeder1Payout, expectedSeeder1);
        assertEq(seeder2Payout, expectedSeeder2);
        assertEq(marketplace.getBuyerReward(contentId, buyer), 0);
    }

    // ======== Batch claim tests ========

    function test_BatchClaim() public {
        uint256 buyer2PrivKey = 0xCAFE;
        address buyer2 = vm.addr(buyer2PrivKey);
        vm.deal(buyer2, 10 ether);

        vm.prank(buyer);
        marketplace.purchase{value: contentPrice}(contentId);
        vm.prank(buyer2);
        marketplace.purchase{value: contentPrice}(contentId);

        uint256 reward1 = marketplace.getBuyerReward(contentId, buyer);
        uint256 reward2 = marketplace.getBuyerReward(contentId, buyer2);

        uint256 ts = block.timestamp;
        bytes memory sig1 = _signReceipt(buyerPrivKey, contentId, seeder1, fileSize, ts);
        bytes memory sig2 = _signReceipt(buyer2PrivKey, contentId, seeder1, fileSize, ts);

        Marketplace.ClaimParams[] memory claims = new Marketplace.ClaimParams[](2);
        claims[0] = Marketplace.ClaimParams({
            contentId: contentId,
            buyer: buyer,
            bytesServed: fileSize,
            timestamp: ts,
            signature: sig1
        });
        claims[1] = Marketplace.ClaimParams({
            contentId: contentId,
            buyer: buyer2,
            bytesServed: fileSize,
            timestamp: ts,
            signature: sig2
        });

        uint256 seeder1BalBefore = seeder1.balance;

        vm.prank(seeder1);
        marketplace.claimDeliveryRewards(claims);

        assertEq(seeder1.balance - seeder1BalBefore, reward1 + reward2);
        assertEq(marketplace.getBuyerReward(contentId, buyer), 0);
        assertEq(marketplace.getBuyerReward(contentId, buyer2), 0);
    }

    function test_BatchClaimSkipsInvalid() public {
        vm.prank(buyer);
        marketplace.purchase{value: contentPrice}(contentId);

        uint256 ts = block.timestamp;
        bytes memory validSig = _signReceipt(buyerPrivKey, contentId, seeder1, fileSize, ts);
        bytes memory invalidSig = _signReceipt(0xDEAD, contentId, seeder1, fileSize, ts);

        Marketplace.ClaimParams[] memory claims = new Marketplace.ClaimParams[](2);
        claims[0] = Marketplace.ClaimParams({
            contentId: contentId,
            buyer: buyer,
            bytesServed: fileSize,
            timestamp: ts,
            signature: validSig
        });
        claims[1] = Marketplace.ClaimParams({
            contentId: contentId,
            buyer: buyer,
            bytesServed: fileSize,
            timestamp: ts,
            signature: invalidSig
        });

        uint256 seeder1BalBefore = seeder1.balance;

        vm.prank(seeder1);
        marketplace.claimDeliveryRewards(claims);

        assertGt(seeder1.balance - seeder1BalBefore, 0);
    }

    // ======== Resale tests ========

    function test_ListForResale() public {
        vm.prank(buyer);
        marketplace.purchase{value: contentPrice}(contentId);

        uint256 resalePrice = 0.2 ether;
        vm.prank(buyer);
        marketplace.listForResale(contentId, resalePrice);

        (uint256 listedPrice, bool active) = marketplace.listings(contentId, buyer);
        assertEq(listedPrice, resalePrice);
        assertTrue(active);
    }

    function test_CancelListing() public {
        vm.prank(buyer);
        marketplace.purchase{value: contentPrice}(contentId);

        vm.startPrank(buyer);
        marketplace.listForResale(contentId, 0.2 ether);
        marketplace.cancelListing(contentId);
        vm.stopPrank();

        (, bool active) = marketplace.listings(contentId, buyer);
        assertFalse(active);
    }

    function test_BuyResale() public {
        // 1. Buyer purchases
        vm.prank(buyer);
        marketplace.purchase{value: contentPrice}(contentId);

        // 2. Buyer lists for resale
        uint256 resalePrice = 0.2 ether;
        vm.startPrank(buyer);
        contentToken.setApprovalForAll(address(marketplace), true);
        marketplace.listForResale(contentId, resalePrice);
        vm.stopPrank();

        // 3. Second user buys the resale
        address buyer2 = makeAddr("buyer2");
        vm.deal(buyer2, 10 ether);

        uint256 buyerBalBefore = buyer.balance;
        uint256 creatorBalBefore = creator.balance;

        vm.prank(buyer2);
        marketplace.buyResale{value: resalePrice}(contentId, buyer);

        // Verify token transferred
        assertEq(contentToken.balanceOf(buyer, uint256(contentId)), 0);
        assertEq(contentToken.balanceOf(buyer2, uint256(contentId)), 1);

        // Verify buyer2 has hasPurchased + buyerReward set
        assertTrue(marketplace.hasPurchased(contentId, buyer2));
        uint256 expectedSeederReward = (resalePrice * RESALE_REWARD_BPS) / 10_000;
        assertEq(marketplace.getBuyerReward(contentId, buyer2), expectedSeederReward);

        // Verify creator received royalty (10% of 0.2 ETH = 0.02 ETH)
        (, uint256 royaltyAmount) = contentToken.royaltyInfo(uint256(contentId), resalePrice);
        assertEq(creator.balance - creatorBalBefore, royaltyAmount);

        // Verify seller received proceeds
        uint256 expectedSellerProceeds = resalePrice - royaltyAmount - expectedSeederReward;
        assertEq(buyer.balance - buyerBalBefore, expectedSellerProceeds);
    }

    function test_ResaleBuyerCanBeServedBySeeder() public {
        // Full resale + reward claiming lifecycle

        // 1. Original buyer purchases
        vm.prank(buyer);
        marketplace.purchase{value: contentPrice}(contentId);

        // 2. Original buyer lists for resale
        uint256 resalePrice = 0.2 ether;
        vm.startPrank(buyer);
        contentToken.setApprovalForAll(address(marketplace), true);
        marketplace.listForResale(contentId, resalePrice);
        vm.stopPrank();

        // 3. Second buyer purchases via resale
        uint256 buyer2PrivKey = 0xCAFE;
        address buyer2 = vm.addr(buyer2PrivKey);
        vm.deal(buyer2, 10 ether);

        vm.prank(buyer2);
        marketplace.buyResale{value: resalePrice}(contentId, buyer);

        // 4. Seeder claims reward from resale buyer
        uint256 buyer2Reward = marketplace.getBuyerReward(contentId, buyer2);
        assertTrue(buyer2Reward > 0);

        uint256 ts = block.timestamp;
        bytes memory sig = _signReceipt(buyer2PrivKey, contentId, seeder1, fileSize, ts);

        uint256 seeder1BalBefore = seeder1.balance;

        vm.prank(seeder1);
        marketplace.claimDeliveryReward(contentId, buyer2, fileSize, ts, sig);

        assertEq(seeder1.balance - seeder1BalBefore, buyer2Reward);
        assertEq(marketplace.getBuyerReward(contentId, buyer2), 0);
    }

    function test_RewardClaimableAfterSellerResells() public {
        // Original buyer purchases, seeder delivers, buyer resells.
        // Seeder should still be able to claim from original purchase.

        // 1. Buyer purchases
        vm.prank(buyer);
        marketplace.purchase{value: contentPrice}(contentId);

        uint256 originalReward = marketplace.getBuyerReward(contentId, buyer);

        // 2. Buyer resells
        vm.startPrank(buyer);
        contentToken.setApprovalForAll(address(marketplace), true);
        marketplace.listForResale(contentId, 0.2 ether);
        vm.stopPrank();

        address buyer2 = makeAddr("buyer2");
        vm.deal(buyer2, 10 ether);
        vm.prank(buyer2);
        marketplace.buyResale{value: 0.2 ether}(contentId, buyer);

        // 3. Original buyer no longer holds token
        assertEq(contentToken.balanceOf(buyer, uint256(contentId)), 0);

        // 4. But seeder can still claim from original buyer's purchase!
        assertTrue(marketplace.hasPurchased(contentId, buyer));

        uint256 ts = block.timestamp;
        bytes memory sig = _signReceipt(buyerPrivKey, contentId, seeder1, fileSize, ts);

        uint256 seeder1BalBefore = seeder1.balance;

        vm.prank(seeder1);
        marketplace.claimDeliveryReward(contentId, buyer, fileSize, ts, sig);

        assertEq(seeder1.balance - seeder1BalBefore, originalReward);
    }

    function test_RevertBuyResaleNoListing() public {
        address buyer2 = makeAddr("buyer2");
        vm.deal(buyer2, 10 ether);

        vm.prank(buyer2);
        vm.expectRevert(Marketplace.NoActiveListing.selector);
        marketplace.buyResale{value: 1 ether}(contentId, buyer);
    }

    function test_RevertBuyResaleSellerTransferredToken() public {
        // Buyer lists, then transfers token directly, then someone tries to buy the listing
        vm.prank(buyer);
        marketplace.purchase{value: contentPrice}(contentId);

        vm.startPrank(buyer);
        contentToken.setApprovalForAll(address(marketplace), true);
        marketplace.listForResale(contentId, 0.2 ether);

        // Transfer token away directly
        contentToken.safeTransferFrom(buyer, makeAddr("random"), uint256(contentId), 1, "");
        vm.stopPrank();

        address buyer2 = makeAddr("buyer2");
        vm.deal(buyer2, 10 ether);

        vm.prank(buyer2);
        vm.expectRevert(Marketplace.NotTokenOwner.selector);
        marketplace.buyResale{value: 0.2 ether}(contentId, buyer);
    }

    // ======== Full lifecycle ========

    function test_FullLifecycle() public {
        // 1. Purchase
        vm.prank(buyer);
        marketplace.purchase{value: contentPrice}(contentId);
        assertTrue(marketplace.hasPurchased(contentId, buyer));
        assertEq(contentToken.balanceOf(buyer, uint256(contentId)), 1);

        uint256 reward = marketplace.getBuyerReward(contentId, buyer);
        assertTrue(reward > 0);

        // 2. Seeder claims
        uint256 ts = block.timestamp;
        bytes memory sig = _signReceipt(buyerPrivKey, contentId, seeder1, fileSize, ts);

        uint256 balBefore = seeder1.balance;
        vm.prank(seeder1);
        marketplace.claimDeliveryReward(contentId, buyer, fileSize, ts, sig);

        assertEq(seeder1.balance - balBefore, reward);
        assertEq(marketplace.getBuyerReward(contentId, buyer), 0);
        assertEq(marketplace.totalRewardsClaimed(), reward);
    }

    // ======== Limited edition + purchase ========

    function test_LimitedEditionPurchase() public {
        // Publish limited edition (2 copies)
        vm.prank(creator);
        bytes32 limitedId =
            contentToken.publishContent(keccak256("limited-game"), "ipfs://QmLimited", contentPrice, fileSize, 2, 1000);

        address buyer2 = makeAddr("buyer2");
        address buyer3 = makeAddr("buyer3");
        vm.deal(buyer, 10 ether);
        vm.deal(buyer2, 10 ether);
        vm.deal(buyer3, 10 ether);

        // First two purchases succeed
        vm.prank(buyer);
        marketplace.purchase{value: contentPrice}(limitedId);
        vm.prank(buyer2);
        marketplace.purchase{value: contentPrice}(limitedId);

        assertEq(contentToken.getTotalMinted(limitedId), 2);

        // Third purchase should fail (sold out)
        vm.prank(buyer3);
        vm.expectRevert(AraContent.EditionSoldOut.selector);
        marketplace.purchase{value: contentPrice}(limitedId);
    }

    // ======== View functions ========

    function test_GetBuyerReward() public {
        vm.prank(buyer);
        marketplace.purchase{value: contentPrice}(contentId);

        uint256 expectedReward = contentPrice - (contentPrice * CREATOR_SHARE_BPS) / 10_000;
        assertEq(marketplace.getBuyerReward(contentId, buyer), expectedReward);
    }

    function test_CheckPurchase() public {
        assertFalse(marketplace.checkPurchase(contentId, buyer));

        vm.prank(buyer);
        marketplace.purchase{value: contentPrice}(contentId);

        assertTrue(marketplace.checkPurchase(contentId, buyer));
    }
}
