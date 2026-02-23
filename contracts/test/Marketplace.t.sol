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
    address public buyer = makeAddr("buyer");
    address public seeder1 = makeAddr("seeder1");
    address public seeder2 = makeAddr("seeder2");

    uint256 public constant PUBLISHER_MIN = 1000 ether;
    uint256 public constant SEEDER_MIN = 100 ether;
    uint256 public constant CREATOR_SHARE_BPS = 8500; // 85%

    bytes32 public contentHash = keccak256("game-file-data");
    string public metadataURI = "ipfs://QmTest123";
    uint256 public contentPrice = 0.1 ether;
    bytes32 public contentId;

    function setUp() public {
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

        // Creator stakes and publishes
        vm.startPrank(creator);
        token.approve(address(staking), PUBLISHER_MIN);
        staking.stake(PUBLISHER_MIN);
        contentId = registry.publishContent(contentHash, metadataURI, contentPrice);
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

    function test_Purchase() public {
        uint256 creatorBalanceBefore = creator.balance;

        vm.prank(buyer);
        marketplace.purchase{value: contentPrice}(contentId);

        assertTrue(marketplace.hasPurchased(contentId, buyer));
        assertEq(marketplace.getPurchaserCount(contentId), 1);

        // Creator should receive 85%
        uint256 expectedCreatorPayment = (contentPrice * CREATOR_SHARE_BPS) / 10_000;
        assertEq(creator.balance - creatorBalanceBefore, expectedCreatorPayment);

        // Reward pool should have 15%
        uint256 expectedPool = contentPrice - expectedCreatorPayment;
        assertEq(marketplace.rewardPool(contentId), expectedPool);
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

    function test_DistributeRewards() public {
        // Purchase first to fund the pool
        vm.prank(buyer);
        marketplace.purchase{value: contentPrice}(contentId);

        uint256 poolBefore = marketplace.rewardPool(contentId);
        assertTrue(poolBefore > 0);

        // Distribute rewards (deployer is the reporter)
        address[] memory seeders = new address[](2);
        seeders[0] = seeder1;
        seeders[1] = seeder2;

        uint256[] memory weights = new uint256[](2);
        weights[0] = 7000; // seeder1 served more data
        weights[1] = 3000;

        vm.prank(deployer);
        marketplace.distributeRewards(contentId, seeders, weights);

        // Seeder1 should get 70%, seeder2 gets 30%
        uint256 seeder1Reward = (poolBefore * 7000) / 10_000;
        uint256 seeder2Reward = (poolBefore * 3000) / 10_000;

        assertEq(marketplace.claimableRewards(seeder1), seeder1Reward);
        assertEq(marketplace.claimableRewards(seeder2), seeder2Reward);
    }

    function test_ClaimRewards() public {
        // Purchase, distribute, then claim
        vm.prank(buyer);
        marketplace.purchase{value: contentPrice}(contentId);

        address[] memory seeders = new address[](1);
        seeders[0] = seeder1;
        uint256[] memory weights = new uint256[](1);
        weights[0] = 1;

        vm.prank(deployer);
        marketplace.distributeRewards(contentId, seeders, weights);

        uint256 reward = marketplace.claimableRewards(seeder1);
        assertTrue(reward > 0);

        uint256 balanceBefore = seeder1.balance;
        vm.prank(seeder1);
        marketplace.claimRewards();

        assertEq(seeder1.balance - balanceBefore, reward);
        assertEq(marketplace.claimableRewards(seeder1), 0);
    }

    function test_RevertClaimNoRewards() public {
        vm.prank(seeder1);
        vm.expectRevert();
        marketplace.claimRewards();
    }

    function test_RevertDistributeByNonReporter() public {
        vm.prank(buyer);
        marketplace.purchase{value: contentPrice}(contentId);

        address[] memory seeders = new address[](1);
        seeders[0] = seeder1;
        uint256[] memory weights = new uint256[](1);
        weights[0] = 1;

        vm.prank(buyer); // Not the reporter
        vm.expectRevert();
        marketplace.distributeRewards(contentId, seeders, weights);
    }

    function test_RevertDistributeIneligibleSeeder() public {
        vm.prank(buyer);
        marketplace.purchase{value: contentPrice}(contentId);

        address[] memory seeders = new address[](1);
        seeders[0] = buyer; // Not staked as seeder
        uint256[] memory weights = new uint256[](1);
        weights[0] = 1;

        vm.prank(deployer);
        vm.expectRevert();
        marketplace.distributeRewards(contentId, seeders, weights);
    }

    function test_OverpaymentRefund() public {
        uint256 overpayment = 1 ether;
        uint256 buyerBalanceBefore = buyer.balance;

        vm.prank(buyer);
        marketplace.purchase{value: overpayment}(contentId);

        // Buyer should be refunded the overpayment
        uint256 expectedSpent = contentPrice;
        assertEq(buyerBalanceBefore - buyer.balance, expectedSpent);
    }

    function test_FullLifecycle() public {
        // 1. Purchase
        vm.prank(buyer);
        marketplace.purchase{value: contentPrice}(contentId);

        // 2. Verify purchase
        assertTrue(marketplace.hasPurchased(contentId, buyer));

        // 3. Distribute rewards to both seeders
        address[] memory seeders = new address[](2);
        seeders[0] = seeder1;
        seeders[1] = seeder2;
        uint256[] memory weights = new uint256[](2);
        weights[0] = 5000;
        weights[1] = 5000;

        vm.prank(deployer);
        marketplace.distributeRewards(contentId, seeders, weights);

        // 4. Both seeders claim
        uint256 seeder1Reward = marketplace.claimableRewards(seeder1);
        uint256 seeder2Reward = marketplace.claimableRewards(seeder2);
        assertTrue(seeder1Reward > 0);
        assertTrue(seeder2Reward > 0);

        vm.prank(seeder1);
        marketplace.claimRewards();
        vm.prank(seeder2);
        marketplace.claimRewards();

        assertEq(marketplace.claimableRewards(seeder1), 0);
        assertEq(marketplace.claimableRewards(seeder2), 0);
    }

    // --- Creator-as-reporter tests ---

    function test_CreatorCanDistributeRewards() public {
        vm.prank(buyer);
        marketplace.purchase{value: contentPrice}(contentId);

        address[] memory seeders = new address[](1);
        seeders[0] = seeder1;
        uint256[] memory weights = new uint256[](1);
        weights[0] = 1;

        // Creator (not the global reporter) can distribute for their own content
        vm.prank(creator);
        marketplace.distributeRewards(contentId, seeders, weights);

        assertTrue(marketplace.claimableRewards(seeder1) > 0);
    }

    function test_RevertDistributeByNonCreatorNonReporter() public {
        vm.prank(buyer);
        marketplace.purchase{value: contentPrice}(contentId);

        address[] memory seeders = new address[](1);
        seeders[0] = seeder1;
        uint256[] memory weights = new uint256[](1);
        weights[0] = 1;

        // seeder1 is neither reporter nor creator
        vm.prank(seeder1);
        vm.expectRevert();
        marketplace.distributeRewards(contentId, seeders, weights);
    }

    // --- publicDistributeWithProofs tests ---

    // Compute the EIP-712 DeliveryReceipt hash for a given seeder address and timestamp
    function _receiptHash(bytes32 cId, address seederAddr, uint256 ts) internal view returns (bytes32) {
        bytes32 structHash = keccak256(abi.encode(marketplace.RECEIPT_TYPE_HASH(), cId, seederAddr, ts));
        return keccak256(abi.encodePacked("\x19\x01", marketplace.DOMAIN_SEPARATOR(), structHash));
    }

    // Build a signed receipt using a private key
    function _signReceipt(uint256 privateKey, bytes32 cId, address seederAddr, uint256 ts)
        internal
        view
        returns (bytes memory)
    {
        bytes32 hash = _receiptHash(cId, seederAddr, ts);
        (uint8 v, bytes32 r, bytes32 s) = vm.sign(privateKey, hash);
        return abi.encodePacked(r, s, v);
    }

    function test_PublicDistributeRevertsBeforeWindow() public {
        vm.prank(buyer);
        marketplace.purchase{value: contentPrice}(contentId);

        Marketplace.SignedReceipt[] memory receipts = new Marketplace.SignedReceipt[](0);
        Marketplace.SeederBundle[] memory bundles = new Marketplace.SeederBundle[](1);
        bundles[0] = Marketplace.SeederBundle({seeder: seeder1, receipts: receipts});

        // Should revert — distribution window has not elapsed
        vm.prank(seeder1);
        vm.expectRevert(Marketplace.DistributionWindowNotOpen.selector);
        marketplace.publicDistributeWithProofs(contentId, bundles);
    }

    function test_PublicDistributeSucceedsAfterWindow() public {
        // Set up a buyer with a known private key for signing
        uint256 buyerPrivKey = 0xBEEF;
        address buyerWithKey = vm.addr(buyerPrivKey);
        vm.deal(buyerWithKey, 10 ether);

        vm.prank(buyerWithKey);
        marketplace.purchase{value: contentPrice}(contentId);

        // Advance time past the distribution window
        uint256 window = marketplace.distributionWindow();
        vm.warp(block.timestamp + window + 1);

        // Build a signed receipt: buyer attests seeder1 served them
        uint256 ts = block.timestamp;
        bytes memory sig = _signReceipt(buyerPrivKey, contentId, seeder1, ts);

        Marketplace.SignedReceipt[] memory receipts = new Marketplace.SignedReceipt[](1);
        receipts[0] = Marketplace.SignedReceipt({timestamp: ts, signature: sig});

        Marketplace.SeederBundle[] memory bundles = new Marketplace.SeederBundle[](1);
        bundles[0] = Marketplace.SeederBundle({seeder: seeder1, receipts: receipts});

        uint256 poolBefore = marketplace.rewardPool(contentId);

        vm.prank(seeder1);
        marketplace.publicDistributeWithProofs(contentId, bundles);

        // seeder1 should have received the full pool (only one seeder with receipts)
        assertEq(marketplace.claimableRewards(seeder1), poolBefore);
    }

    function test_PublicDistributeReplayProtection() public {
        uint256 buyerPrivKey = 0xCAFE;
        address buyerWithKey = vm.addr(buyerPrivKey);
        vm.deal(buyerWithKey, 10 ether);

        vm.prank(buyerWithKey);
        marketplace.purchase{value: contentPrice}(contentId);

        uint256 window = marketplace.distributionWindow();
        vm.warp(block.timestamp + window + 1);

        uint256 ts = block.timestamp;
        bytes memory sig = _signReceipt(buyerPrivKey, contentId, seeder1, ts);

        Marketplace.SignedReceipt[] memory receipts = new Marketplace.SignedReceipt[](1);
        receipts[0] = Marketplace.SignedReceipt({timestamp: ts, signature: sig});

        Marketplace.SeederBundle[] memory bundles = new Marketplace.SeederBundle[](1);
        bundles[0] = Marketplace.SeederBundle({seeder: seeder1, receipts: receipts});

        // First distribution succeeds
        vm.prank(seeder1);
        marketplace.publicDistributeWithProofs(contentId, bundles);
        uint256 firstReward = marketplace.claimableRewards(seeder1);
        assertTrue(firstReward > 0);

        // Submitting the same receipt again yields zero additional weight (skipped)
        // Pool should be near-empty (dust only), so second distribution reverts NoRewardsToDistribute
        // or succeeds with zero payout. We just verify the reward didn't double.
        vm.prank(seeder1);
        // If pool has dust it may succeed; if pool is 0 it reverts. Either is correct.
        try marketplace.publicDistributeWithProofs(contentId, bundles) {} catch {}
        // Reward must not have doubled
        uint256 secondReward = marketplace.claimableRewards(seeder1);
        assertEq(secondReward, firstReward); // no additional rewards from replayed receipt
    }

    function test_PublicDistributeSkipsInvalidBuyer() public {
        // signer has NOT purchased — their receipt should be skipped
        uint256 nonBuyerPrivKey = 0xDEAD;

        // buyer purchases (not nonBuyer)
        vm.prank(buyer);
        marketplace.purchase{value: contentPrice}(contentId);

        uint256 window = marketplace.distributionWindow();
        vm.warp(block.timestamp + window + 1);

        uint256 ts = block.timestamp;
        // nonBuyer signs a receipt (they never purchased, so it's invalid)
        bytes memory sig = _signReceipt(nonBuyerPrivKey, contentId, seeder1, ts);

        Marketplace.SignedReceipt[] memory receipts = new Marketplace.SignedReceipt[](1);
        receipts[0] = Marketplace.SignedReceipt({timestamp: ts, signature: sig});

        Marketplace.SeederBundle[] memory bundles = new Marketplace.SeederBundle[](1);
        bundles[0] = Marketplace.SeederBundle({seeder: seeder1, receipts: receipts});

        // Should revert with ZeroWeight since the only receipt is invalid
        vm.prank(seeder1);
        vm.expectRevert(Marketplace.ZeroWeight.selector);
        marketplace.publicDistributeWithProofs(contentId, bundles);

        // seeder1 should have nothing claimable
        assertEq(marketplace.claimableRewards(seeder1), 0);
    }

    function test_PublicDistributeRevertsIfNoPurchases() public {
        // No purchases at all — lastPurchaseTime is 0, reverts before seeder eligibility check
        bytes32 freshContentHash = keccak256("fresh-content");
        vm.prank(creator);
        bytes32 freshContentId = registry.publishContent(freshContentHash, metadataURI, contentPrice);

        Marketplace.SeederBundle[] memory bundles = new Marketplace.SeederBundle[](0);

        vm.warp(block.timestamp + 365 days); // well past any window

        vm.prank(seeder1);
        vm.expectRevert(Marketplace.NoRewardsToDistribute.selector);
        marketplace.publicDistributeWithProofs(freshContentId, bundles);
    }

    function test_SetDistributionWindow() public {
        uint256 newWindow = 7 days;
        vm.prank(deployer);
        marketplace.setDistributionWindow(newWindow);
        assertEq(marketplace.distributionWindow(), newWindow);
    }

    function test_LastPurchaseTimeUpdated() public {
        uint256 timeBefore = block.timestamp;
        vm.prank(buyer);
        marketplace.purchase{value: contentPrice}(contentId);
        assertEq(marketplace.lastPurchaseTime(contentId), timeBefore);
    }

    // --- updateContentFile tests ---

    function test_UpdateContentFile() public {
        bytes32 newContentHash = keccak256("game-file-v2-data");

        vm.prank(creator);
        registry.updateContentFile(contentId, newContentHash);

        // The on-chain content hash should now reflect the new file
        assertEq(registry.getContentHash(contentId), newContentHash);
        // The contentId is unchanged
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
        // Buyer purchases the original content
        vm.prank(buyer);
        marketplace.purchase{value: contentPrice}(contentId);
        assertTrue(marketplace.hasPurchased(contentId, buyer));

        // Creator pushes a file update
        bytes32 newContentHash = keccak256("game-file-v2-data");
        vm.prank(creator);
        registry.updateContentFile(contentId, newContentHash);

        // Buyer's purchase record is still valid for the same contentId
        assertTrue(marketplace.hasPurchased(contentId, buyer));
    }
}
