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
}
