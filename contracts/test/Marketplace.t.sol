// SPDX-License-Identifier: LGPL-3.0
pragma solidity ^0.8.24;

import {Test, console} from "forge-std/Test.sol";
import {AraStaking} from "../src/AraStaking.sol";
import {ContentRegistry} from "../src/ContentRegistry.sol";
import {Marketplace} from "../src/Marketplace.sol";

contract MockAraToken3 {
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
        balanceOf[msg.sender] -= amount;
        balanceOf[to] += amount;
        return true;
    }

    function transferFrom(address from, address to, uint256 amount) external returns (bool) {
        require(allowance[from][msg.sender] >= amount);
        allowance[from][msg.sender] -= amount;
        balanceOf[from] -= amount;
        balanceOf[to] += amount;
        return true;
    }
}

contract MarketplaceTest is Test {
    AraStaking public staking;
    ContentRegistry public registry;
    Marketplace public marketplace;
    MockAraToken3 public token;

    address public deployer = makeAddr("deployer");
    address public creator = makeAddr("creator");
    address public seeder1 = makeAddr("seeder1");
    address public seeder2 = makeAddr("seeder2");

    // Buyer with known private key (needed for EIP-712 signatures)
    uint256 public buyerPrivKey = 0xBEEF;
    address public buyer;

    uint256 public constant PUBLISHER_MIN = 1000 ether;
    uint256 public constant SEEDER_MIN = 100 ether;
    uint256 public constant CREATOR_SHARE_BPS = 8500; // 85%

    bytes32 public contentHash = keccak256("game-file-data");
    string public metadataURI = "ipfs://QmTest123";
    uint256 public contentPrice = 0.1 ether;
    uint256 public fileSize = 1_000_000; // 1 MB
    bytes32 public contentId;

    function setUp() public {
        buyer = vm.addr(buyerPrivKey);

        vm.startPrank(deployer);
        token = new MockAraToken3();
        staking = new AraStaking(address(token), PUBLISHER_MIN, SEEDER_MIN);
        registry = new ContentRegistry(address(staking));
        marketplace = new Marketplace(address(registry), address(staking), CREATOR_SHARE_BPS);
        vm.stopPrank();

        // Fund all participants
        token.mint(creator, 10_000 ether);
        token.mint(seeder1, 5_000 ether);
        token.mint(seeder2, 5_000 ether);
        vm.deal(buyer, 10 ether);

        // Creator stakes and publishes (now with fileSize)
        vm.startPrank(creator);
        token.approve(address(staking), PUBLISHER_MIN);
        staking.stake(PUBLISHER_MIN);
        contentId = registry.publishContent(contentHash, metadataURI, contentPrice, fileSize);
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

    // --- Helpers ---

    /// Compute the EIP-712 DeliveryReceipt hash
    function _receiptHash(bytes32 cId, address seederAddr, uint256 bytesServedVal, uint256 ts)
        internal
        view
        returns (bytes32)
    {
        bytes32 structHash =
            keccak256(abi.encode(marketplace.RECEIPT_TYPE_HASH(), cId, seederAddr, bytesServedVal, ts));
        return keccak256(abi.encodePacked("\x19\x01", marketplace.DOMAIN_SEPARATOR(), structHash));
    }

    /// Sign a delivery receipt with a given private key
    function _signReceipt(uint256 privateKey, bytes32 cId, address seederAddr, uint256 bytesServedVal, uint256 ts)
        internal
        view
        returns (bytes memory)
    {
        bytes32 hash = _receiptHash(cId, seederAddr, bytesServedVal, ts);
        (uint8 v, bytes32 r, bytes32 s) = vm.sign(privateKey, hash);
        return abi.encodePacked(r, s, v);
    }

    // --- Purchase tests ---

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
        registry.delistContent(contentId);

        vm.prank(buyer);
        vm.expectRevert();
        marketplace.purchase{value: contentPrice}(contentId);
    }

    function test_OverpaymentRefund() public {
        uint256 overpayment = 1 ether;
        uint256 buyerBalanceBefore = buyer.balance;

        vm.prank(buyer);
        marketplace.purchase{value: overpayment}(contentId);

        // Buyer should be refunded the overpayment
        assertEq(buyerBalanceBefore - buyer.balance, contentPrice);
    }

    // --- Single claim tests ---

    function test_ClaimDeliveryReward() public {
        // 1. Buyer purchases
        vm.prank(buyer);
        marketplace.purchase{value: contentPrice}(contentId);

        uint256 rewardBefore = marketplace.getBuyerReward(contentId, buyer);
        assertTrue(rewardBefore > 0);

        // 2. Seeder1 claims with buyer-signed receipt (full file delivery)
        uint256 ts = block.timestamp;
        bytes memory sig = _signReceipt(buyerPrivKey, contentId, seeder1, fileSize, ts);

        uint256 seeder1BalBefore = seeder1.balance;

        vm.prank(seeder1);
        marketplace.claimDeliveryReward(contentId, buyer, fileSize, ts, sig);

        // Seeder1 should have received the full reward (bytesServed == fileSize)
        assertEq(seeder1.balance - seeder1BalBefore, rewardBefore);
        assertEq(marketplace.getBuyerReward(contentId, buyer), 0);
    }

    function test_ClaimDeliveryRewardPartialBytes() public {
        vm.prank(buyer);
        marketplace.purchase{value: contentPrice}(contentId);

        uint256 rewardBefore = marketplace.getBuyerReward(contentId, buyer);
        uint256 halfFileSize = fileSize / 2;

        // Seeder1 delivers half the file
        uint256 ts = block.timestamp;
        bytes memory sig = _signReceipt(buyerPrivKey, contentId, seeder1, halfFileSize, ts);

        uint256 seeder1BalBefore = seeder1.balance;

        vm.prank(seeder1);
        marketplace.claimDeliveryReward(contentId, buyer, halfFileSize, ts, sig);

        // Seeder1 should get 50% of the reward
        uint256 expectedPayout = (rewardBefore * halfFileSize) / fileSize;
        assertEq(seeder1.balance - seeder1BalBefore, expectedPayout);
        assertEq(marketplace.getBuyerReward(contentId, buyer), rewardBefore - expectedPayout);
    }

    function test_ClaimRevertInvalidSignature() public {
        vm.prank(buyer);
        marketplace.purchase{value: contentPrice}(contentId);

        // Use a WRONG private key to sign
        uint256 wrongKey = 0xDEAD;
        uint256 ts = block.timestamp;
        bytes memory sig = _signReceipt(wrongKey, contentId, seeder1, fileSize, ts);

        vm.prank(seeder1);
        vm.expectRevert(Marketplace.NoRewardsToClaim.selector);
        marketplace.claimDeliveryReward(contentId, buyer, fileSize, ts, sig);
    }

    function test_ClaimRevertNotPurchased() public {
        // buyer never purchased — create a signature anyway
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

        // First claim succeeds
        vm.prank(seeder1);
        marketplace.claimDeliveryReward(contentId, buyer, fileSize, ts, sig);

        // Second claim with same receipt reverts
        vm.prank(seeder1);
        vm.expectRevert(Marketplace.NoRewardsToClaim.selector);
        marketplace.claimDeliveryReward(contentId, buyer, fileSize, ts, sig);
    }

    // --- Multi-seeder proportional claiming ---

    function test_TwoSeedersProportionalClaim() public {
        vm.prank(buyer);
        marketplace.purchase{value: contentPrice}(contentId);

        uint256 originalReward = marketplace.getBuyerReward(contentId, buyer);
        uint256 ts = block.timestamp;

        // Seeder1 delivers 600K of 1M bytes
        uint256 served1 = 600_000;
        bytes memory sig1 = _signReceipt(buyerPrivKey, contentId, seeder1, served1, ts);

        // Seeder2 delivers 400K of 1M bytes
        uint256 served2 = 400_000;
        bytes memory sig2 = _signReceipt(buyerPrivKey, contentId, seeder2, served2, ts);

        // Seeder1 claims first
        uint256 seeder1BalBefore = seeder1.balance;
        vm.prank(seeder1);
        marketplace.claimDeliveryReward(contentId, buyer, served1, ts, sig1);
        uint256 seeder1Payout = seeder1.balance - seeder1BalBefore;

        // Seeder2 claims second — their share is computed from original reward, not remaining
        uint256 seeder2BalBefore = seeder2.balance;
        vm.prank(seeder2);
        marketplace.claimDeliveryReward(contentId, buyer, served2, ts, sig2);
        uint256 seeder2Payout = seeder2.balance - seeder2BalBefore;

        // Verify proportional payouts
        uint256 expectedSeeder1 = (originalReward * served1) / fileSize;
        uint256 expectedSeeder2 = (originalReward * served2) / fileSize;

        assertEq(seeder1Payout, expectedSeeder1);
        assertEq(seeder2Payout, expectedSeeder2);
        assertEq(marketplace.getBuyerReward(contentId, buyer), 0);
    }

    // --- Batch claim tests ---

    function test_BatchClaim() public {
        // Create a second buyer
        uint256 buyer2PrivKey = 0xCAFE;
        address buyer2 = vm.addr(buyer2PrivKey);
        vm.deal(buyer2, 10 ether);

        // Both buyers purchase
        vm.prank(buyer);
        marketplace.purchase{value: contentPrice}(contentId);
        vm.prank(buyer2);
        marketplace.purchase{value: contentPrice}(contentId);

        uint256 reward1 = marketplace.getBuyerReward(contentId, buyer);
        uint256 reward2 = marketplace.getBuyerReward(contentId, buyer2);

        // Seeder1 claims both in a single batch transaction
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

        // Should receive both rewards in one transfer
        assertEq(seeder1.balance - seeder1BalBefore, reward1 + reward2);
        assertEq(marketplace.getBuyerReward(contentId, buyer), 0);
        assertEq(marketplace.getBuyerReward(contentId, buyer2), 0);
    }

    function test_BatchClaimSkipsInvalid() public {
        vm.prank(buyer);
        marketplace.purchase{value: contentPrice}(contentId);

        uint256 ts = block.timestamp;
        bytes memory validSig = _signReceipt(buyerPrivKey, contentId, seeder1, fileSize, ts);
        bytes memory invalidSig = _signReceipt(0xDEAD, contentId, seeder1, fileSize, ts); // wrong signer

        Marketplace.ClaimParams[] memory claims = new Marketplace.ClaimParams[](2);
        // First claim: valid
        claims[0] = Marketplace.ClaimParams({
            contentId: contentId,
            buyer: buyer,
            bytesServed: fileSize,
            timestamp: ts,
            signature: validSig
        });
        // Second claim: invalid signature (will be skipped)
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

        // Only the valid claim should have paid out
        assertGt(seeder1.balance - seeder1BalBefore, 0);
    }

    function test_BatchClaimRevertsAllInvalid() public {
        // No purchases — all claims will fail
        uint256 ts = block.timestamp;
        bytes memory sig = _signReceipt(buyerPrivKey, contentId, seeder1, fileSize, ts);

        Marketplace.ClaimParams[] memory claims = new Marketplace.ClaimParams[](1);
        claims[0] = Marketplace.ClaimParams({
            contentId: contentId,
            buyer: buyer,
            bytesServed: fileSize,
            timestamp: ts,
            signature: sig
        });

        vm.prank(seeder1);
        vm.expectRevert(Marketplace.NoRewardsToClaim.selector);
        marketplace.claimDeliveryRewards(claims);
    }

    // --- Full lifecycle ---

    function test_FullLifecycle() public {
        // 1. Purchase
        vm.prank(buyer);
        marketplace.purchase{value: contentPrice}(contentId);
        assertTrue(marketplace.hasPurchased(contentId, buyer));

        uint256 reward = marketplace.getBuyerReward(contentId, buyer);
        assertTrue(reward > 0);

        // 2. Seeder claims with buyer-signed receipt
        uint256 ts = block.timestamp;
        bytes memory sig = _signReceipt(buyerPrivKey, contentId, seeder1, fileSize, ts);

        uint256 balBefore = seeder1.balance;
        vm.prank(seeder1);
        marketplace.claimDeliveryReward(contentId, buyer, fileSize, ts, sig);

        // 3. Verify payout
        assertEq(seeder1.balance - balBefore, reward);
        assertEq(marketplace.getBuyerReward(contentId, buyer), 0);
        assertEq(marketplace.totalRewardsClaimed(), reward);
    }

    // --- View functions ---

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

    // --- ContentRegistry fileSize tests ---

    function test_FileSizeStoredOnPublish() public {
        assertEq(registry.getFileSize(contentId), fileSize);
    }

    function test_RevertPublishZeroFileSize() public {
        vm.prank(creator);
        vm.expectRevert(ContentRegistry.ZeroFileSize.selector);
        registry.publishContent(keccak256("new-content"), metadataURI, contentPrice, 0);
    }

    function test_UpdateFileSize() public {
        uint256 newSize = 2_000_000;
        vm.prank(creator);
        registry.updateFileSize(contentId, newSize);
        assertEq(registry.getFileSize(contentId), newSize);
    }

    // --- Content update file tests (preserved from before) ---

    function test_UpdateContentFile() public {
        bytes32 newContentHash = keccak256("game-file-v2-data");

        vm.prank(creator);
        registry.updateContentFile(contentId, newContentHash);

        assertEq(registry.getContentHash(contentId), newContentHash);
        assertEq(registry.getCreator(contentId), creator);
        assertTrue(registry.isActive(contentId));
    }

    function test_UpdateContentFileEmitsEvent() public {
        bytes32 newContentHash = keccak256("game-file-v2-data");

        vm.prank(creator);
        vm.expectEmit(true, true, false, true);
        emit ContentRegistry.ContentFileUpdated(contentId, contentHash, newContentHash, creator);
        registry.updateContentFile(contentId, newContentHash);
    }

    function test_UpdateContentFileRevertNonCreator() public {
        bytes32 newContentHash = keccak256("game-file-v2-data");

        vm.prank(buyer);
        vm.expectRevert(ContentRegistry.NotContentCreator.selector);
        registry.updateContentFile(contentId, newContentHash);
    }

    function test_UpdateContentFileRevertDelisted() public {
        bytes32 newContentHash = keccak256("game-file-v2-data");

        vm.prank(creator);
        registry.delistContent(contentId);

        vm.prank(creator);
        vm.expectRevert(ContentRegistry.ContentNotActive.selector);
        registry.updateContentFile(contentId, newContentHash);
    }

    function test_UpdateContentFilePreservesPurchases() public {
        vm.prank(buyer);
        marketplace.purchase{value: contentPrice}(contentId);
        assertTrue(marketplace.hasPurchased(contentId, buyer));

        bytes32 newContentHash = keccak256("game-file-v2-data");
        vm.prank(creator);
        registry.updateContentFile(contentId, newContentHash);

        assertTrue(marketplace.hasPurchased(contentId, buyer));
    }
}
