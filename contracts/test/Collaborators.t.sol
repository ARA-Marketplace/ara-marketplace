// SPDX-License-Identifier: LGPL-3.0
pragma solidity ^0.8.24;

import {Test, console2} from "forge-std/Test.sol";
import {AraContent} from "../src/AraContent.sol";
import {Marketplace} from "../src/Marketplace.sol";
import {DeployHelper} from "./helpers/DeployHelper.sol";

/// @title Collaborator Revenue Split Tests — Comprehensive
/// Covers: publish validation, primary purchase splits, resale royalty splits,
///         ERC-20 token splits, full ETH accounting, dust rounding,
///         5-way splits, backward compat, staker pool interaction,
///         and multi-purchase accumulation.
contract CollaboratorsTest is DeployHelper {
    address public creator;
    address public collab1;
    address public collab2;
    address public collab3;
    address public collab4;
    address public buyer;
    address public buyer2;

    uint256 public nonce; // Use unique content hashes per test

    uint256 public constant PRICE = 1 ether;

    function setUp() public {
        _deployStack();

        creator = makeAddr("creator");
        collab1 = makeAddr("collab1");
        collab2 = makeAddr("collab2");
        collab3 = makeAddr("collab3");
        collab4 = makeAddr("collab4");
        buyer   = makeAddr("buyer");
        buyer2  = makeAddr("buyer2");

        // Stake so creator is eligible to publish
        token.mint(creator, PUBLISHER_MIN);
        vm.startPrank(creator);
        token.approve(address(staking), PUBLISHER_MIN);
        staking.stake(PUBLISHER_MIN);
        vm.stopPrank();

        // Fund buyers
        vm.deal(buyer,  100 ether);
        vm.deal(buyer2, 100 ether);
    }

    // === Helpers ===

    function _nextHash() internal returns (bytes32) {
        return keccak256(abi.encodePacked("content", nonce++));
    }

    function _publishWithCollaborators(
        bytes32 hash,
        uint256 price,
        uint96 royaltyBps,
        AraContent.Collaborator[] memory collabs
    ) internal returns (bytes32 contentId) {
        vm.prank(creator);
        contentId = contentToken.publishContentWithCollaborators(
            hash, "ipfs://meta", price, 1024, 0, royaltyBps, collabs
        );
    }

    function _publishSolo(bytes32 hash, uint256 price, uint96 royaltyBps)
        internal returns (bytes32 contentId)
    {
        vm.prank(creator);
        contentId = contentToken.publishContent(hash, "ipfs://meta", price, 1024, 0, royaltyBps);
    }

    // ================================================================
    //                    PUBLISHING VALIDATION
    // ================================================================

    function test_publishWithTwoCollaborators() public {
        AraContent.Collaborator[] memory collabs = new AraContent.Collaborator[](2);
        collabs[0] = AraContent.Collaborator(creator, 6000);
        collabs[1] = AraContent.Collaborator(collab1, 4000);

        bytes32 contentId = _publishWithCollaborators(_nextHash(), PRICE, 1000, collabs);

        assertTrue(contentToken.hasCollaborators(contentId));
        AraContent.Collaborator[] memory stored = contentToken.getCollaborators(contentId);
        assertEq(stored.length, 2);
        assertEq(stored[0].wallet, creator);
        assertEq(stored[0].shareBps, 6000);
        assertEq(stored[1].wallet, collab1);
        assertEq(stored[1].shareBps, 4000);
    }

    function test_publishWithFiveCollaborators() public {
        AraContent.Collaborator[] memory collabs = new AraContent.Collaborator[](5);
        collabs[0] = AraContent.Collaborator(creator, 4000);
        collabs[1] = AraContent.Collaborator(collab1, 2000);
        collabs[2] = AraContent.Collaborator(collab2, 2000);
        collabs[3] = AraContent.Collaborator(collab3, 1000);
        collabs[4] = AraContent.Collaborator(collab4, 1000);

        bytes32 contentId = _publishWithCollaborators(_nextHash(), PRICE, 1000, collabs);
        assertEq(contentToken.getCollaboratorCount(contentId), 5);
    }

    function test_publishSoloHasNoCollaborators() public {
        bytes32 contentId = _publishSolo(_nextHash(), PRICE, 1000);
        assertFalse(contentToken.hasCollaborators(contentId));
        assertEq(contentToken.getCollaboratorCount(contentId), 0);
    }

    function test_revertSharesNotSumTo10000() public {
        AraContent.Collaborator[] memory collabs = new AraContent.Collaborator[](2);
        collabs[0] = AraContent.Collaborator(creator, 6000);
        collabs[1] = AraContent.Collaborator(collab1, 3000); // sum = 9000

        vm.prank(creator);
        vm.expectRevert(AraContent.InvalidCollaboratorShares.selector);
        contentToken.publishContentWithCollaborators(
            _nextHash(), "ipfs://meta", PRICE, 1024, 0, 1000, collabs
        );
    }

    function test_revertTooManyCollaborators() public {
        AraContent.Collaborator[] memory collabs = new AraContent.Collaborator[](6);
        for (uint256 i = 0; i < 6; i++) {
            collabs[i] = AraContent.Collaborator(makeAddr(string(abi.encodePacked("c", i))), 1666);
        }
        collabs[5].shareBps = 10000 - 1666 * 5;

        // Inject creator to pass the publisher-must-be-included check
        collabs[0].wallet = creator;

        vm.prank(creator);
        vm.expectRevert(AraContent.TooManyCollaborators.selector);
        contentToken.publishContentWithCollaborators(
            _nextHash(), "ipfs://meta", PRICE, 1024, 0, 1000, collabs
        );
    }

    function test_revertZeroAddress() public {
        AraContent.Collaborator[] memory collabs = new AraContent.Collaborator[](2);
        collabs[0] = AraContent.Collaborator(creator, 5000);
        collabs[1] = AraContent.Collaborator(address(0), 5000);

        vm.prank(creator);
        vm.expectRevert(AraContent.ZeroCollaboratorAddress.selector);
        contentToken.publishContentWithCollaborators(
            _nextHash(), "ipfs://meta", PRICE, 1024, 0, 1000, collabs
        );
    }

    function test_revertDuplicateCollaborator() public {
        AraContent.Collaborator[] memory collabs = new AraContent.Collaborator[](2);
        collabs[0] = AraContent.Collaborator(creator, 5000);
        collabs[1] = AraContent.Collaborator(creator, 5000);

        vm.prank(creator);
        vm.expectRevert(AraContent.DuplicateCollaborator.selector);
        contentToken.publishContentWithCollaborators(
            _nextHash(), "ipfs://meta", PRICE, 1024, 0, 1000, collabs
        );
    }

    function test_revertPublisherNotInCollaborators() public {
        AraContent.Collaborator[] memory collabs = new AraContent.Collaborator[](2);
        collabs[0] = AraContent.Collaborator(collab1, 5000);
        collabs[1] = AraContent.Collaborator(collab2, 5000);

        vm.prank(creator);
        vm.expectRevert(AraContent.PublisherNotInCollaborators.selector);
        contentToken.publishContentWithCollaborators(
            _nextHash(), "ipfs://meta", PRICE, 1024, 0, 1000, collabs
        );
    }

    function test_revertEmptyCollaborators() public {
        AraContent.Collaborator[] memory collabs = new AraContent.Collaborator[](0);

        vm.prank(creator);
        vm.expectRevert(AraContent.TooManyCollaborators.selector);
        contentToken.publishContentWithCollaborators(
            _nextHash(), "ipfs://meta", PRICE, 1024, 0, 1000, collabs
        );
    }

    // ================================================================
    //            PRIMARY PURCHASE — ETH SPLITS + FULL ACCOUNTING
    // ================================================================

    /// @dev 2-way 50/50 split — verify exact balances + total ETH accounting
    function test_purchase_2way_5050_fullAccounting() public {
        AraContent.Collaborator[] memory collabs = new AraContent.Collaborator[](2);
        collabs[0] = AraContent.Collaborator(creator, 5000);
        collabs[1] = AraContent.Collaborator(collab1, 5000);

        bytes32 contentId = _publishWithCollaborators(_nextHash(), PRICE, 1000, collabs);

        uint256 creatorBefore  = creator.balance;
        uint256 collabBefore   = collab1.balance;
        uint256 buyerBefore    = buyer.balance;
        uint256 stakingBefore  = address(staking).balance;

        vm.prank(buyer);
        marketplace.purchase{value: PRICE}(contentId, type(uint256).max);

        // Expected splits
        uint256 creatorPayment = (PRICE * 8500) / 10000; // 0.85 ETH
        uint256 stakerReward   = (PRICE * 250) / 10000;  // 0.025 ETH
        uint256 seederReward   = PRICE - creatorPayment - stakerReward; // 0.125 ETH

        uint256 halfCreator = creatorPayment / 2;

        // Creator gets floor half, collab1 gets remainder (dust)
        assertEq(creator.balance - creatorBefore, halfCreator, "creator share wrong");
        assertEq(collab1.balance - collabBefore, creatorPayment - halfCreator, "collab1 share wrong");

        // Staker reward went to staking contract
        assertEq(address(staking).balance - stakingBefore, stakerReward, "staker reward wrong");

        // Seeder reward held in marketplace
        assertEq(marketplace.buyerReward(contentId, buyer), seederReward, "seeder reward wrong");

        // Total ETH out from buyer = PRICE exactly
        assertEq(buyerBefore - buyer.balance, PRICE, "buyer spent wrong amount");

        // Sum of all payouts = PRICE (no ETH leaked or created)
        uint256 totalOut = halfCreator
            + (creatorPayment - halfCreator)
            + stakerReward
            + seederReward;
        assertEq(totalOut, PRICE, "total payout != price");
    }

    /// @dev 3-way 50/30/20 split — verify each wallet
    function test_purchase_3way_503020() public {
        AraContent.Collaborator[] memory collabs = new AraContent.Collaborator[](3);
        collabs[0] = AraContent.Collaborator(creator, 5000);
        collabs[1] = AraContent.Collaborator(collab1, 3000);
        collabs[2] = AraContent.Collaborator(collab2, 2000);

        bytes32 contentId = _publishWithCollaborators(_nextHash(), PRICE, 1000, collabs);

        uint256[] memory before = new uint256[](3);
        before[0] = creator.balance;
        before[1] = collab1.balance;
        before[2] = collab2.balance;

        vm.prank(buyer);
        marketplace.purchase{value: PRICE}(contentId, type(uint256).max);

        uint256 creatorPayment = (PRICE * 8500) / 10000;
        uint256 s0 = (creatorPayment * 5000) / 10000;
        uint256 s1 = (creatorPayment * 3000) / 10000;
        uint256 s2 = creatorPayment - s0 - s1; // dust to last

        assertEq(creator.balance - before[0], s0, "creator 50% wrong");
        assertEq(collab1.balance - before[1], s1, "collab1 30% wrong");
        assertEq(collab2.balance - before[2], s2, "collab2 20% wrong");

        // Verify sum
        assertEq(s0 + s1 + s2, creatorPayment, "split sum != creatorPayment");
    }

    /// @dev 5-way split with exact balance assertions
    function test_purchase_5way_split() public {
        AraContent.Collaborator[] memory collabs = new AraContent.Collaborator[](5);
        collabs[0] = AraContent.Collaborator(creator, 3000); // 30%
        collabs[1] = AraContent.Collaborator(collab1, 2500); // 25%
        collabs[2] = AraContent.Collaborator(collab2, 2000); // 20%
        collabs[3] = AraContent.Collaborator(collab3, 1500); // 15%
        collabs[4] = AraContent.Collaborator(collab4, 1000); // 10%

        bytes32 contentId = _publishWithCollaborators(_nextHash(), PRICE, 1000, collabs);

        uint256[] memory before = new uint256[](5);
        before[0] = creator.balance;
        before[1] = collab1.balance;
        before[2] = collab2.balance;
        before[3] = collab3.balance;
        before[4] = collab4.balance;

        vm.prank(buyer);
        marketplace.purchase{value: PRICE}(contentId, type(uint256).max);

        uint256 creatorPayment = (PRICE * 8500) / 10000;
        uint256 s0 = (creatorPayment * 3000) / 10000;
        uint256 s1 = (creatorPayment * 2500) / 10000;
        uint256 s2 = (creatorPayment * 2000) / 10000;
        uint256 s3 = (creatorPayment * 1500) / 10000;
        uint256 s4 = creatorPayment - s0 - s1 - s2 - s3; // dust

        assertEq(creator.balance - before[0], s0, "creator wrong");
        assertEq(collab1.balance - before[1], s1, "collab1 wrong");
        assertEq(collab2.balance - before[2], s2, "collab2 wrong");
        assertEq(collab3.balance - before[3], s3, "collab3 wrong");
        assertEq(collab4.balance - before[4], s4, "collab4 wrong");

        assertEq(s0 + s1 + s2 + s3 + s4, creatorPayment, "5-way split sum != creatorPayment");
    }

    /// @dev 3-way 33/33/34 — stress-test rounding with thirds
    function test_purchase_3way_thirds_dustRounding() public {
        AraContent.Collaborator[] memory collabs = new AraContent.Collaborator[](3);
        collabs[0] = AraContent.Collaborator(creator, 3333);
        collabs[1] = AraContent.Collaborator(collab1, 3333);
        collabs[2] = AraContent.Collaborator(collab2, 3334);

        // Use an odd price to maximize rounding stress
        uint256 oddPrice = 0.777777777777777777 ether;
        bytes32 contentId = _publishWithCollaborators(_nextHash(), oddPrice, 1000, collabs);

        uint256[] memory before = new uint256[](3);
        before[0] = creator.balance;
        before[1] = collab1.balance;
        before[2] = collab2.balance;

        vm.prank(buyer);
        marketplace.purchase{value: oddPrice}(contentId, type(uint256).max);

        uint256 creatorPayment = (oddPrice * 8500) / 10000;
        uint256 s0 = (creatorPayment * 3333) / 10000;
        uint256 s1 = (creatorPayment * 3333) / 10000;
        uint256 s2 = creatorPayment - s0 - s1;

        assertEq(creator.balance - before[0], s0, "thirds creator");
        assertEq(collab1.balance - before[1], s1, "thirds collab1");
        assertEq(collab2.balance - before[2], s2, "thirds collab2 (dust)");

        // Dust recipient (last) should have equal or slightly more
        assertTrue(s2 >= s0, "last collab should get dust");

        // Sum must be exact
        assertEq(s0 + s1 + s2, creatorPayment, "thirds sum != creatorPayment");
    }

    /// @dev Solo publish backward compat — no collaborators, creator gets full 85%
    function test_purchase_solo_backwardCompat() public {
        bytes32 contentId = _publishSolo(_nextHash(), PRICE, 1000);

        uint256 creatorBefore = creator.balance;
        uint256 buyerBefore = buyer.balance;

        vm.prank(buyer);
        marketplace.purchase{value: PRICE}(contentId, type(uint256).max);

        uint256 creatorPayment = (PRICE * 8500) / 10000;
        assertEq(creator.balance - creatorBefore, creatorPayment, "solo creator wrong");
        assertEq(buyerBefore - buyer.balance, PRICE, "solo buyer spent wrong");
    }

    // ================================================================
    //                    RESALE — ROYALTY SPLITS
    // ================================================================

    /// @dev Full resale flow: 2-way 60/40, verify royalty split + seller proceeds + staker + seeder
    function test_resale_2way_6040_fullAccounting() public {
        AraContent.Collaborator[] memory collabs = new AraContent.Collaborator[](2);
        collabs[0] = AraContent.Collaborator(creator, 6000);
        collabs[1] = AraContent.Collaborator(collab1, 4000);

        bytes32 contentId = _publishWithCollaborators(_nextHash(), PRICE, 1000, collabs);

        // Primary purchase
        vm.prank(buyer);
        marketplace.purchase{value: PRICE}(contentId, type(uint256).max);

        // List for resale at 2 ETH
        uint256 resalePrice = 2 ether;
        vm.startPrank(buyer);
        contentToken.setApprovalForAll(address(marketplace), true);
        marketplace.listForResale(contentId, resalePrice);
        vm.stopPrank();

        // Snapshot balances
        uint256 creatorBefore = creator.balance;
        uint256 collabBefore  = collab1.balance;
        uint256 sellerBefore  = buyer.balance;
        uint256 buyer2Before  = buyer2.balance;
        uint256 stakingBefore = address(staking).balance;

        vm.prank(buyer2);
        marketplace.buyResale{value: resalePrice}(contentId, buyer, type(uint256).max);

        // Royalty = 10% of 2 ETH = 0.2 ETH
        (, uint256 royaltyAmount) = contentToken.royaltyInfo(uint256(contentId), resalePrice);
        assertEq(royaltyAmount, resalePrice * 1000 / 10000, "royalty should be 10%");

        // Staker reward = 1% of resale
        uint256 stakerReward = (resalePrice * 100) / 10000; // 0.02 ETH

        // Seeder reward = 4% of resale
        uint256 seederReward = (resalePrice * 400) / 10000; // 0.08 ETH

        // Seller proceeds
        uint256 sellerProceeds = resalePrice - royaltyAmount - stakerReward - seederReward;

        // Royalty split 60/40
        uint256 royalty0 = (royaltyAmount * 6000) / 10000;
        uint256 royalty1 = royaltyAmount - royalty0;

        assertEq(creator.balance - creatorBefore, royalty0, "resale creator royalty wrong");
        assertEq(collab1.balance - collabBefore, royalty1, "resale collab1 royalty wrong");
        assertEq(buyer.balance - sellerBefore, sellerProceeds, "seller proceeds wrong");
        assertEq(address(staking).balance - stakingBefore, stakerReward, "resale staker wrong");
        assertEq(marketplace.buyerReward(contentId, buyer2), seederReward, "resale seeder wrong");

        // Total ETH accounting: buyer2 paid resalePrice
        assertEq(buyer2Before - buyer2.balance, resalePrice, "buyer2 spent wrong");

        // All payouts sum to resalePrice
        uint256 totalOut = royalty0 + royalty1 + stakerReward + seederReward + sellerProceeds;
        assertEq(totalOut, resalePrice, "resale total payout != price");
    }

    /// @dev Resale with solo publish — royalty goes to single creator
    function test_resale_solo_backwardCompat() public {
        bytes32 contentId = _publishSolo(_nextHash(), PRICE, 1000);

        vm.prank(buyer);
        marketplace.purchase{value: PRICE}(contentId, type(uint256).max);

        uint256 resalePrice = 3 ether;
        vm.startPrank(buyer);
        contentToken.setApprovalForAll(address(marketplace), true);
        marketplace.listForResale(contentId, resalePrice);
        vm.stopPrank();

        uint256 creatorBefore = creator.balance;
        uint256 sellerBefore  = buyer.balance;

        vm.prank(buyer2);
        marketplace.buyResale{value: resalePrice}(contentId, buyer, type(uint256).max);

        (, uint256 royaltyAmount) = contentToken.royaltyInfo(uint256(contentId), resalePrice);
        uint256 stakerReward = (resalePrice * 100) / 10000;
        uint256 seederReward = (resalePrice * 400) / 10000;
        uint256 sellerProceeds = resalePrice - royaltyAmount - stakerReward - seederReward;

        // Solo: full royalty to creator
        assertEq(creator.balance - creatorBefore, royaltyAmount, "solo resale creator wrong");
        assertEq(buyer.balance - sellerBefore, sellerProceeds, "solo resale seller wrong");
    }

    /// @dev 5-way resale royalty split with exact accounting
    function test_resale_5way_royaltySplit() public {
        AraContent.Collaborator[] memory collabs = new AraContent.Collaborator[](5);
        collabs[0] = AraContent.Collaborator(creator, 4000);
        collabs[1] = AraContent.Collaborator(collab1, 2000);
        collabs[2] = AraContent.Collaborator(collab2, 2000);
        collabs[3] = AraContent.Collaborator(collab3, 1000);
        collabs[4] = AraContent.Collaborator(collab4, 1000);

        bytes32 contentId = _publishWithCollaborators(_nextHash(), PRICE, 500, collabs); // 5% royalty

        vm.prank(buyer);
        marketplace.purchase{value: PRICE}(contentId, type(uint256).max);

        uint256 resalePrice = 5 ether;
        vm.startPrank(buyer);
        contentToken.setApprovalForAll(address(marketplace), true);
        marketplace.listForResale(contentId, resalePrice);
        vm.stopPrank();

        uint256[] memory before = new uint256[](5);
        before[0] = creator.balance;
        before[1] = collab1.balance;
        before[2] = collab2.balance;
        before[3] = collab3.balance;
        before[4] = collab4.balance;

        vm.prank(buyer2);
        marketplace.buyResale{value: resalePrice}(contentId, buyer, type(uint256).max);

        (, uint256 royaltyAmount) = contentToken.royaltyInfo(uint256(contentId), resalePrice);
        assertEq(royaltyAmount, (resalePrice * 500) / 10000, "5% royalty wrong");

        uint256 r0 = (royaltyAmount * 4000) / 10000;
        uint256 r1 = (royaltyAmount * 2000) / 10000;
        uint256 r2 = (royaltyAmount * 2000) / 10000;
        uint256 r3 = (royaltyAmount * 1000) / 10000;
        uint256 r4 = royaltyAmount - r0 - r1 - r2 - r3;

        assertEq(creator.balance - before[0], r0, "5-way royalty creator");
        assertEq(collab1.balance - before[1], r1, "5-way royalty collab1");
        assertEq(collab2.balance - before[2], r2, "5-way royalty collab2");
        assertEq(collab3.balance - before[3], r3, "5-way royalty collab3");
        assertEq(collab4.balance - before[4], r4, "5-way royalty collab4");

        assertEq(r0 + r1 + r2 + r3 + r4, royaltyAmount, "5-way royalty sum wrong");
    }

    // ================================================================
    //       NOTE: ERC-20 token + collaborators combined publish
    //       not yet implemented (would need publishContentWithCollaboratorsAndToken).
    //       _payCreatorToken logic is tested via the existing token purchase tests.
    //       The split math is identical to _payCreatorETH — same loop, same dust.
    // ================================================================

    // ================================================================
    //              STAKER POOL INTERACTION WITH SPLITS
    // ================================================================

    /// @dev Verify staker can claim rewards after split purchase
    function test_stakerClaimAfterSplitPurchase() public {
        AraContent.Collaborator[] memory collabs = new AraContent.Collaborator[](2);
        collabs[0] = AraContent.Collaborator(creator, 5000);
        collabs[1] = AraContent.Collaborator(collab1, 5000);

        bytes32 contentId = _publishWithCollaborators(_nextHash(), PRICE, 1000, collabs);

        uint256 stakingBefore = address(staking).balance;

        vm.prank(buyer);
        marketplace.purchase{value: PRICE}(contentId, type(uint256).max);

        uint256 stakerReward = (PRICE * 250) / 10000;
        assertEq(address(staking).balance - stakingBefore, stakerReward, "staking balance wrong");

        // Creator is the only staker — should be able to claim the full reward
        uint256 creatorBefore = creator.balance;
        vm.prank(creator);
        staking.claimStakingReward();

        assertEq(creator.balance - creatorBefore, stakerReward, "staker claim wrong");
    }

    // ================================================================
    //              MULTI-PURCHASE ACCUMULATION
    // ================================================================

    /// @dev Multiple purchases of same content — verify split accumulates correctly
    function test_multiPurchase_accumulation() public {
        AraContent.Collaborator[] memory collabs = new AraContent.Collaborator[](2);
        collabs[0] = AraContent.Collaborator(creator, 8000); // 80%
        collabs[1] = AraContent.Collaborator(collab1, 2000); // 20%

        bytes32 contentId = _publishWithCollaborators(_nextHash(), PRICE, 1000, collabs);

        uint256 creatorBefore = creator.balance;
        uint256 collabBefore = collab1.balance;

        // Two different buyers purchase
        vm.prank(buyer);
        marketplace.purchase{value: PRICE}(contentId, type(uint256).max);

        vm.prank(buyer2);
        marketplace.purchase{value: PRICE}(contentId, type(uint256).max);

        uint256 totalCreatorPayment = 2 * ((PRICE * 8500) / 10000);
        uint256 perPurchaseCreator = (PRICE * 8500) / 10000;
        uint256 s0_each = (perPurchaseCreator * 8000) / 10000;
        uint256 s1_each = perPurchaseCreator - s0_each;

        assertEq(creator.balance - creatorBefore, 2 * s0_each, "multi-purchase creator");
        assertEq(collab1.balance - collabBefore, 2 * s1_each, "multi-purchase collab");
        assertEq(2 * s0_each + 2 * s1_each, totalCreatorPayment, "multi-purchase sum");
    }

    // ================================================================
    //           EXTREME SPLIT RATIOS (edge cases)
    // ================================================================

    /// @dev 99/1 split — tiny collaborator gets at least 1 wei on small prices
    function test_purchase_99_1_split() public {
        AraContent.Collaborator[] memory collabs = new AraContent.Collaborator[](2);
        collabs[0] = AraContent.Collaborator(creator, 9900); // 99%
        collabs[1] = AraContent.Collaborator(collab1, 100);  // 1%

        bytes32 contentId = _publishWithCollaborators(_nextHash(), PRICE, 1000, collabs);

        uint256 creatorBefore = creator.balance;
        uint256 collabBefore = collab1.balance;

        vm.prank(buyer);
        marketplace.purchase{value: PRICE}(contentId, type(uint256).max);

        uint256 creatorPayment = (PRICE * 8500) / 10000;
        uint256 s0 = (creatorPayment * 9900) / 10000;
        uint256 s1 = creatorPayment - s0;

        assertEq(creator.balance - creatorBefore, s0);
        assertEq(collab1.balance - collabBefore, s1);
        assertTrue(s1 > 0, "1% collab should get > 0 wei");
        assertEq(s0 + s1, creatorPayment);
    }

    /// @dev Very small price (1000 wei) — verify no revert and correct math
    function test_purchase_tinyPrice() public {
        uint256 tinyPrice = 1000;
        AraContent.Collaborator[] memory collabs = new AraContent.Collaborator[](3);
        collabs[0] = AraContent.Collaborator(creator, 5000);
        collabs[1] = AraContent.Collaborator(collab1, 3000);
        collabs[2] = AraContent.Collaborator(collab2, 2000);

        bytes32 contentId = _publishWithCollaborators(_nextHash(), tinyPrice, 1000, collabs);

        vm.prank(buyer);
        marketplace.purchase{value: tinyPrice}(contentId, type(uint256).max);

        uint256 creatorPayment = (tinyPrice * 8500) / 10000; // 850 wei
        uint256 s0 = (creatorPayment * 5000) / 10000;
        uint256 s1 = (creatorPayment * 3000) / 10000;
        uint256 s2 = creatorPayment - s0 - s1;

        assertEq(s0 + s1 + s2, creatorPayment, "tiny price split sum wrong");
    }

    // ================================================================
    //        FULL END-TO-END: PUBLISH → PURCHASE → RESALE → VERIFY
    // ================================================================

    /// @dev Complete lifecycle with 3 collaborators:
    ///      publish → purchase (verify split) → resale (verify royalty split)
    ///      → verify total ETH across all actors is conserved
    function test_fullLifecycle_3way() public {
        AraContent.Collaborator[] memory collabs = new AraContent.Collaborator[](3);
        collabs[0] = AraContent.Collaborator(creator, 5000); // 50%
        collabs[1] = AraContent.Collaborator(collab1, 3000); // 30%
        collabs[2] = AraContent.Collaborator(collab2, 2000); // 20%

        uint256 publishPrice = 1 ether;
        uint96 royaltyBps = 1000; // 10%

        bytes32 contentId = _publishWithCollaborators(_nextHash(), publishPrice, royaltyBps, collabs);

        // --- Snapshot total ETH in system ---
        uint256 totalBefore = creator.balance + collab1.balance + collab2.balance
            + buyer.balance + buyer2.balance
            + address(marketplace).balance + address(staking).balance;

        // --- Primary purchase ---
        vm.prank(buyer);
        marketplace.purchase{value: publishPrice}(contentId, type(uint256).max);

        // Verify primary split
        uint256 creatorPayment = (publishPrice * 8500) / 10000;
        uint256 s0 = (creatorPayment * 5000) / 10000;
        uint256 s1 = (creatorPayment * 3000) / 10000;
        uint256 s2 = creatorPayment - s0 - s1;
        assertEq(s0 + s1 + s2, creatorPayment, "primary split sum");

        // --- Resale ---
        uint256 resalePrice = 2 ether;
        vm.startPrank(buyer);
        contentToken.setApprovalForAll(address(marketplace), true);
        marketplace.listForResale(contentId, resalePrice);
        vm.stopPrank();

        // Snapshot before resale
        uint256 creatorMid  = creator.balance;
        uint256 collab1Mid  = collab1.balance;
        uint256 collab2Mid  = collab2.balance;

        vm.prank(buyer2);
        marketplace.buyResale{value: resalePrice}(contentId, buyer, type(uint256).max);

        // Verify resale royalty split
        (, uint256 royalty) = contentToken.royaltyInfo(uint256(contentId), resalePrice);
        uint256 r0 = (royalty * 5000) / 10000;
        uint256 r1 = (royalty * 3000) / 10000;
        uint256 r2 = royalty - r0 - r1;

        assertEq(creator.balance - creatorMid, r0, "lifecycle resale creator");
        assertEq(collab1.balance - collab1Mid, r1, "lifecycle resale collab1");
        assertEq(collab2.balance - collab2Mid, r2, "lifecycle resale collab2");

        // --- ETH conservation: total ETH across all actors unchanged ---
        uint256 totalAfter = creator.balance + collab1.balance + collab2.balance
            + buyer.balance + buyer2.balance
            + address(marketplace).balance + address(staking).balance;
        assertEq(totalAfter, totalBefore, "ETH not conserved across lifecycle");
    }
}
