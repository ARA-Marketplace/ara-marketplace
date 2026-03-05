// SPDX-License-Identifier: LGPL-3.0
pragma solidity ^0.8.24;

import {DeployHelper, MockToken} from "./helpers/DeployHelper.sol";
import {Marketplace} from "../src/Marketplace.sol";
import {AraContent} from "../src/AraContent.sol";

/// @title FuzzEconomics
/// @notice Fuzz tests for purchase/resale economic invariants.
contract FuzzEconomics is DeployHelper {
    address public deployer = makeAddr("deployer");
    address public creator = makeAddr("creator");
    address public seeder1 = makeAddr("seeder1");

    uint256 public buyerPrivKey = 0xBEEF;
    address public buyer;

    bytes32 public contentHash = keccak256("fuzz-content");
    uint256 public fileSize = 1_000_000;

    function setUp() public {
        buyer = vm.addr(buyerPrivKey);

        vm.startPrank(deployer);
        _deployStack();
        vm.stopPrank();

        token.mint(creator, 10_000 ether);
        token.mint(seeder1, 5_000 ether);

        vm.startPrank(creator);
        token.approve(address(staking), PUBLISHER_MIN);
        staking.stake(PUBLISHER_MIN);
        vm.stopPrank();

        vm.startPrank(seeder1);
        token.approve(address(staking), SEEDER_MIN);
        staking.stake(SEEDER_MIN);
        vm.stopPrank();
    }

    // ═══════════════════════════════════════════════════════════════════
    //                    PRIMARY PURCHASE SPLITS
    // ═══════════════════════════════════════════════════════════════════

    /// @notice creator + staker + seeder must sum exactly to price
    function testFuzz_purchaseSplitSumsToPrice(uint256 price) public {
        // Bound to realistic range: 1000 wei to 100 ETH
        price = bound(price, 1000, 100 ether);

        vm.prank(creator);
        bytes32 cId = contentToken.publishContent(contentHash, "ipfs://fuzz", price, fileSize, 0, 1000);

        vm.deal(buyer, price);

        uint256 creatorBal = creator.balance;
        uint256 stakingBal = address(staking).balance;
        uint256 marketBal = address(marketplace).balance;

        vm.prank(buyer);
        marketplace.purchase{value: price}(cId, type(uint256).max);

        uint256 creatorGot = creator.balance - creatorBal;
        uint256 stakerGot = address(staking).balance - stakingBal;
        uint256 seederHeld = address(marketplace).balance - marketBal;

        assertEq(creatorGot + stakerGot + seederHeld, price, "split does not sum to price");
    }

    /// @notice Individual BPS-calculated shares should be within 1 wei of expected
    function testFuzz_purchaseBpsAccuracy(uint256 price) public {
        price = bound(price, 10_000, 100 ether); // need enough for clean BPS math

        vm.prank(creator);
        bytes32 cId = contentToken.publishContent(
            keccak256(abi.encode("acc", price)), "ipfs://acc", price, fileSize, 0, 500
        );

        vm.deal(buyer, price);

        uint256 creatorBal = creator.balance;

        vm.prank(buyer);
        marketplace.purchase{value: price}(cId, type(uint256).max);

        uint256 creatorGot = creator.balance - creatorBal;
        uint256 expectedCreator = (price * CREATOR_SHARE_BPS) / 10_000;

        assertEq(creatorGot, expectedCreator, "creator payment off");
    }

    // ═══════════════════════════════════════════════════════════════════
    //                    COLLABORATOR SPLITS
    // ═══════════════════════════════════════════════════════════════════

    /// @notice With 2 collaborators, total paid to collabs must equal creator share
    function testFuzz_collaboratorSplitSumsToCreatorPayment(uint256 price, uint256 splitBps) public {
        price = bound(price, 10_000, 100 ether);
        splitBps = bound(splitBps, 1, 9999); // collab1 gets splitBps, collab2 gets rest

        address collab2 = makeAddr("collab2");

        AraContent.Collaborator[] memory collabs = new AraContent.Collaborator[](2);
        collabs[0] = AraContent.Collaborator({wallet: creator, shareBps: splitBps});
        collabs[1] = AraContent.Collaborator({wallet: collab2, shareBps: 10_000 - splitBps});

        vm.prank(creator);
        bytes32 cId = contentToken.publishContentWithCollaborators(
            keccak256(abi.encode("collab", price, splitBps)), "ipfs://collab", price, fileSize, 0, 1000, collabs
        );

        vm.deal(buyer, price);

        uint256 creatorBal = creator.balance;
        uint256 collab2Bal = collab2.balance;

        vm.prank(buyer);
        marketplace.purchase{value: price}(cId, type(uint256).max);

        uint256 creatorGot = creator.balance - creatorBal;
        uint256 collab2Got = collab2.balance - collab2Bal;
        uint256 expectedCreatorTotal = (price * CREATOR_SHARE_BPS) / 10_000;

        assertEq(creatorGot + collab2Got, expectedCreatorTotal, "collab split != creator share");
    }

    // ═══════════════════════════════════════════════════════════════════
    //                    RESALE ACCOUNTING
    // ═══════════════════════════════════════════════════════════════════

    /// @notice Resale: royalty + staker + seeder + seller must sum to price
    function testFuzz_resaleAccountingConserved(uint256 resalePrice, uint96 royaltyBps) public {
        resalePrice = bound(resalePrice, 10_000, 100 ether);
        royaltyBps = uint96(bound(royaltyBps, 0, 5000));

        // Ensure total deductions don't exceed price
        vm.assume(
            (resalePrice * royaltyBps) / 10_000 +
            (resalePrice * RESALE_STAKER_REWARD_BPS) / 10_000 +
            (resalePrice * RESALE_REWARD_BPS) / 10_000 <= resalePrice
        );

        bytes32 cId = _setupResale(resalePrice, royaltyBps);

        address buyer2 = makeAddr("buyer2");
        vm.deal(buyer2, resalePrice);

        // Snapshot balances before resale
        uint256[4] memory before = [
            creator.balance,
            buyer.balance,
            address(staking).balance,
            address(marketplace).balance
        ];

        vm.prank(buyer2);
        marketplace.buyResale{value: resalePrice}(cId, buyer, type(uint256).max);

        uint256 totalOut = (creator.balance - before[0])
            + (buyer.balance - before[1])
            + (address(staking).balance - before[2])
            + (address(marketplace).balance - before[3]);

        assertEq(totalOut, resalePrice, "resale split does not sum to price");
    }

    function _setupResale(uint256 resalePrice, uint96 royaltyBps) internal returns (bytes32 cId) {
        vm.prank(creator);
        cId = contentToken.publishContent(
            keccak256(abi.encode("resale", resalePrice, royaltyBps)),
            "ipfs://resale",
            0.01 ether,
            fileSize,
            0,
            royaltyBps
        );

        vm.deal(buyer, 0.01 ether);
        vm.prank(buyer);
        marketplace.purchase{value: 0.01 ether}(cId, type(uint256).max);

        vm.startPrank(buyer);
        contentToken.setApprovalForAll(address(marketplace), true);
        marketplace.listForResale(cId, resalePrice);
        vm.stopPrank();
    }

    // ═══════════════════════════════════════════════════════════════════
    //                    DELIVERY REWARD CLAIMING
    // ═══════════════════════════════════════════════════════════════════

    /// @notice Claimed reward never exceeds the buyer's reward pool
    function testFuzz_claimDeliveryRewardBounded(uint256 bytesServed) public {
        bytesServed = bound(bytesServed, 1, fileSize * 2); // up to 2x fileSize

        uint256 price = 0.1 ether;
        vm.prank(creator);
        bytes32 cId = contentToken.publishContent(
            keccak256("claim-bounded"), "ipfs://claim", price, fileSize, 0, 1000
        );

        vm.deal(buyer, price);
        vm.prank(buyer);
        marketplace.purchase{value: price}(cId, type(uint256).max);

        uint256 maxReward = marketplace.getBuyerReward(cId, buyer);

        uint256 ts = block.timestamp;
        bytes memory sig = _signReceipt(buyerPrivKey, cId, seeder1, bytesServed, ts);

        uint256 seederBal = seeder1.balance;

        // Allocate content stake for seeder
        vm.prank(seeder1);
        staking.stakeForContent(cId, SEEDER_MIN);

        vm.prank(seeder1);
        marketplace.claimDeliveryReward(cId, buyer, bytesServed, ts, sig);

        uint256 payout = seeder1.balance - seederBal;
        assertLe(payout, maxReward, "payout exceeds reward pool");
    }

    /// @notice Staker reward via accumulator is proportional to stake
    function testFuzz_stakerRewardProportional(uint256 stakeAmount) public {
        stakeAmount = bound(stakeAmount, 1 ether, 100_000 ether);

        address staker2 = makeAddr("staker2");
        token.mint(staker2, stakeAmount);

        vm.startPrank(staker2);
        token.approve(address(staking), stakeAmount);
        staking.stake(stakeAmount);
        vm.stopPrank();

        uint256 price = 1 ether;
        vm.prank(creator);
        bytes32 cId = contentToken.publishContent(
            keccak256(abi.encode("staker-prop", stakeAmount)), "ipfs://sp", price, fileSize, 0, 500
        );

        vm.deal(buyer, price);
        vm.prank(buyer);
        marketplace.purchase{value: price}(cId, type(uint256).max);

        uint256 stakerReward = (price * STAKER_REWARD_BPS) / 10_000;
        uint256 totalStaked = staking.totalStaked();
        uint256 expected = (stakerReward * stakeAmount) / totalStaked;
        uint256 actual = staking.earned(staker2);

        // Allow 0.01% rounding error (accumulator truncation at large stakes)
        assertApproxEqRel(actual, expected, 1e14, "staker reward not proportional");
    }

    // ═══════════════════════════════════════════════════════════════════
    //                    MAX PRICE SLIPPAGE PROTECTION
    // ═══════════════════════════════════════════════════════════════════

    /// @notice purchase reverts if price > maxPrice
    function testFuzz_purchaseMaxPriceReverts(uint256 price, uint256 maxPrice) public {
        price = bound(price, 1000, 100 ether);
        maxPrice = bound(maxPrice, 0, price - 1);

        vm.prank(creator);
        bytes32 cId = contentToken.publishContent(
            keccak256(abi.encode("maxprice", price)), "ipfs://mp", price, fileSize, 0, 500
        );

        vm.deal(buyer, price);
        vm.prank(buyer);
        vm.expectRevert();
        marketplace.purchase{value: price}(cId, maxPrice);
    }
}
