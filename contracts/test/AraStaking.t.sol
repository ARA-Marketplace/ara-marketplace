// SPDX-License-Identifier: LGPL-3.0
pragma solidity ^0.8.24;

import {DeployHelper} from "./helpers/DeployHelper.sol";
import {AraStaking} from "../src/AraStaking.sol";

contract AraStakingTest is DeployHelper {
    address public alice = makeAddr("alice");
    address public bob = makeAddr("bob");

    function setUp() public {
        _deployStack();

        // Give users tokens
        token.mint(alice, 10_000 ether);
        token.mint(bob, 5_000 ether);
    }

    function test_Stake() public {
        vm.startPrank(alice);
        token.approve(address(staking), 1000 ether);
        staking.stake(1000 ether);
        vm.stopPrank();

        assertEq(staking.stakedBalance(alice), 1000 ether);
        assertEq(token.balanceOf(address(staking)), 1000 ether);
    }

    function test_Unstake() public {
        vm.startPrank(alice);
        token.approve(address(staking), 1000 ether);
        staking.stake(1000 ether);
        staking.unstake(500 ether);
        vm.stopPrank();

        assertEq(staking.stakedBalance(alice), 500 ether);
        assertEq(token.balanceOf(alice), 9500 ether);
    }

    function test_RevertUnstakeInsufficientBalance() public {
        vm.startPrank(alice);
        token.approve(address(staking), 100 ether);
        staking.stake(100 ether);

        vm.expectRevert();
        staking.unstake(200 ether);
        vm.stopPrank();
    }

    function test_StakeForContent() public {
        bytes32 contentId = keccak256("test-content");

        vm.startPrank(alice);
        token.approve(address(staking), 500 ether);
        staking.stake(500 ether);
        staking.stakeForContent(contentId, 200 ether);
        vm.stopPrank();

        assertEq(staking.stakedBalance(alice), 300 ether);
        assertEq(staking.contentStake(alice, contentId), 200 ether);
    }

    function test_UnstakeFromContent() public {
        bytes32 contentId = keccak256("test-content");

        vm.startPrank(alice);
        token.approve(address(staking), 500 ether);
        staking.stake(500 ether);
        staking.stakeForContent(contentId, 200 ether);
        staking.unstakeFromContent(contentId, 100 ether);
        vm.stopPrank();

        assertEq(staking.stakedBalance(alice), 400 ether);
        assertEq(staking.contentStake(alice, contentId), 100 ether);
    }

    function test_IsEligiblePublisher() public {
        vm.startPrank(alice);
        token.approve(address(staking), PUBLISHER_MIN);
        staking.stake(PUBLISHER_MIN);
        vm.stopPrank();

        assertTrue(staking.isEligiblePublisher(alice));
        assertFalse(staking.isEligiblePublisher(bob));
    }

    function test_IsEligibleSeeder() public {
        bytes32 contentId = keccak256("test-content");

        vm.startPrank(alice);
        token.approve(address(staking), SEEDER_MIN);
        staking.stake(SEEDER_MIN);
        staking.stakeForContent(contentId, SEEDER_MIN);
        vm.stopPrank();

        assertTrue(staking.isEligibleSeeder(alice, contentId));
        assertFalse(staking.isEligibleSeeder(bob, contentId));
    }

    function test_RevertStakeZero() public {
        vm.prank(alice);
        vm.expectRevert();
        staking.stake(0);
    }

    function test_InitializeRevertsOnImpl() public {
        // Direct call to implementation should revert (initializers disabled)
        AraStaking impl = new AraStaking();
        vm.expectRevert();
        impl.initialize(address(token), PUBLISHER_MIN, SEEDER_MIN);
    }

    function testFuzz_StakeAndUnstake(uint256 stakeAmount, uint256 unstakeAmount) public {
        stakeAmount = bound(stakeAmount, 1, 10_000 ether);
        unstakeAmount = bound(unstakeAmount, 1, stakeAmount);

        vm.startPrank(alice);
        token.approve(address(staking), stakeAmount);
        staking.stake(stakeAmount);
        staking.unstake(unstakeAmount);
        vm.stopPrank();

        assertEq(staking.stakedBalance(alice), stakeAmount - unstakeAmount);
    }

    // ======== V2: Passive staker reward tests ========

    function test_StakeUpdatesTotalStaked() public {
        vm.startPrank(alice);
        token.approve(address(staking), 1000 ether);
        staking.stake(1000 ether);
        vm.stopPrank();

        assertEq(staking.totalStaked(), 1000 ether);
        assertEq(staking.totalUserStake(alice), 1000 ether);

        vm.startPrank(bob);
        token.approve(address(staking), 500 ether);
        staking.stake(500 ether);
        vm.stopPrank();

        assertEq(staking.totalStaked(), 1500 ether);
        assertEq(staking.totalUserStake(bob), 500 ether);
    }

    function test_UnstakeUpdatesTotalStaked() public {
        vm.startPrank(alice);
        token.approve(address(staking), 1000 ether);
        staking.stake(1000 ether);
        staking.unstake(400 ether);
        vm.stopPrank();

        assertEq(staking.totalStaked(), 600 ether);
        assertEq(staking.totalUserStake(alice), 600 ether);
    }

    function test_StakeForContentDoesNotChangeTotalUserStake() public {
        bytes32 contentId = keccak256("test-content");

        vm.startPrank(alice);
        token.approve(address(staking), 500 ether);
        staking.stake(500 ether);
        staking.stakeForContent(contentId, 200 ether);
        vm.stopPrank();

        // totalUserStake should still be 500 (content allocation is internal)
        assertEq(staking.totalUserStake(alice), 500 ether);
        assertEq(staking.totalStaked(), 500 ether);
        // But stakedBalance is reduced
        assertEq(staking.stakedBalance(alice), 300 ether);
    }

    function test_AddRewardOnlyMarketplace() public {
        vm.deal(alice, 1 ether);
        vm.prank(alice);
        vm.expectRevert(AraStaking.OnlyAuthorizedMarketplace.selector);
        staking.addReward{value: 0.1 ether}();
    }

    function test_AddRewardAccumulatesRewardPerToken() public {
        // Alice stakes 1000 ARA
        vm.startPrank(alice);
        token.approve(address(staking), 1000 ether);
        staking.stake(1000 ether);
        vm.stopPrank();

        // Marketplace deposits 1 ETH reward
        vm.deal(address(marketplace), 10 ether);
        vm.prank(address(marketplace));
        staking.addReward{value: 1 ether}();

        // rewardPerToken = (1e18 * 1e18) / (1000e18) = 1e15
        assertEq(staking.rewardPerToken(), 1e15);
    }

    function test_EarnedAccruesCorrectly() public {
        // Alice stakes 1000 ARA
        vm.startPrank(alice);
        token.approve(address(staking), 1000 ether);
        staking.stake(1000 ether);
        vm.stopPrank();

        // Marketplace deposits 1 ETH reward
        vm.deal(address(marketplace), 10 ether);
        vm.prank(address(marketplace));
        staking.addReward{value: 1 ether}();

        // Alice should have earned all 1 ETH (sole staker)
        assertEq(staking.earned(alice), 1 ether);
        assertEq(staking.earned(bob), 0);
    }

    function test_ClaimStakingReward() public {
        // Alice stakes 1000 ARA
        vm.startPrank(alice);
        token.approve(address(staking), 1000 ether);
        staking.stake(1000 ether);
        vm.stopPrank();

        // Marketplace deposits 1 ETH reward
        vm.deal(address(marketplace), 10 ether);
        vm.prank(address(marketplace));
        staking.addReward{value: 1 ether}();

        // Alice claims
        uint256 aliceBalBefore = alice.balance;
        vm.prank(alice);
        staking.claimStakingReward();

        assertEq(alice.balance - aliceBalBefore, 1 ether);
        assertEq(staking.earned(alice), 0);
        assertEq(staking.totalStakerRewardsClaimed(), 1 ether);
    }

    function test_TwoStakersProportionalReward() public {
        // Alice stakes 750 ARA, Bob stakes 250 ARA
        vm.startPrank(alice);
        token.approve(address(staking), 750 ether);
        staking.stake(750 ether);
        vm.stopPrank();

        vm.startPrank(bob);
        token.approve(address(staking), 250 ether);
        staking.stake(250 ether);
        vm.stopPrank();

        // Marketplace deposits 1 ETH
        vm.deal(address(marketplace), 10 ether);
        vm.prank(address(marketplace));
        staking.addReward{value: 1 ether}();

        // Alice should earn 75%, Bob 25%
        assertEq(staking.earned(alice), 0.75 ether);
        assertEq(staking.earned(bob), 0.25 ether);

        // Both claim
        uint256 aliceBal = alice.balance;
        vm.prank(alice);
        staking.claimStakingReward();
        assertEq(alice.balance - aliceBal, 0.75 ether);

        uint256 bobBal = bob.balance;
        vm.prank(bob);
        staking.claimStakingReward();
        assertEq(bob.balance - bobBal, 0.25 ether);
    }

    function test_LateStakerDoesNotEarnPastRewards() public {
        // Alice stakes first
        vm.startPrank(alice);
        token.approve(address(staking), 1000 ether);
        staking.stake(1000 ether);
        vm.stopPrank();

        // Reward deposited
        vm.deal(address(marketplace), 10 ether);
        vm.prank(address(marketplace));
        staking.addReward{value: 1 ether}();

        // Bob stakes after the reward
        vm.startPrank(bob);
        token.approve(address(staking), 1000 ether);
        staking.stake(1000 ether);
        vm.stopPrank();

        // Alice earned all of the first reward, Bob earned nothing
        assertEq(staking.earned(alice), 1 ether);
        assertEq(staking.earned(bob), 0);

        // Second reward: split equally (1000 ARA each)
        vm.prank(address(marketplace));
        staking.addReward{value: 1 ether}();

        assertEq(staking.earned(alice), 1.5 ether);
        assertEq(staking.earned(bob), 0.5 ether);
    }

    function test_ClaimZeroReverts() public {
        vm.prank(alice);
        vm.expectRevert(AraStaking.NoStakerRewardsToClaim.selector);
        staking.claimStakingReward();
    }

    function testFuzz_MultipleRewardsAndClaims(uint256 reward1, uint256 reward2) public {
        // Minimum reward must exceed accumulator precision floor:
        // (reward * 1e18) / totalStaked > 0  →  reward > totalStaked / 1e18
        // With 1000 ARA staked: reward > 1000
        reward1 = bound(reward1, 0.001 ether, 10 ether);
        reward2 = bound(reward2, 0.001 ether, 10 ether);

        // Alice stakes
        vm.startPrank(alice);
        token.approve(address(staking), 1000 ether);
        staking.stake(1000 ether);
        vm.stopPrank();

        vm.deal(address(marketplace), reward1 + reward2);

        // First reward (sole staker — allow accumulator rounding up to 1000 wei)
        vm.prank(address(marketplace));
        staking.addReward{value: reward1}();
        assertApproxEqAbs(staking.earned(alice), reward1, 1000);

        // Claim first
        vm.prank(alice);
        staking.claimStakingReward();
        assertEq(staking.earned(alice), 0);

        // Second reward
        vm.prank(address(marketplace));
        staking.addReward{value: reward2}();
        assertApproxEqAbs(staking.earned(alice), reward2, 1000);

        // Claim second
        uint256 balBefore = alice.balance;
        vm.prank(alice);
        staking.claimStakingReward();
        assertApproxEqAbs(alice.balance - balBefore, reward2, 1000);
    }
}
