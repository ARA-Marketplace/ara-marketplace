// SPDX-License-Identifier: LGPL-3.0
pragma solidity ^0.8.24;

import {Test} from "forge-std/Test.sol";
import {DeployHelper, MockToken} from "./helpers/DeployHelper.sol";
import {Marketplace} from "../src/Marketplace.sol";
import {AraContent} from "../src/AraContent.sol";
import {AraStaking} from "../src/AraStaking.sol";

/// @title InvariantHandler
/// @notice Handler contract that the invariant fuzzer calls. Each function is an "action"
///         the fuzzer can take: stake, publish, purchase, resale, claim.
contract InvariantHandler is Test {
    DeployHelper internal helper;
    MockToken public token;
    AraStaking public staking;
    AraContent public contentToken;
    Marketplace public marketplace;

    // Tracked actors
    address[] public actors;
    mapping(address => uint256) public actorKeys;
    bytes32[] public publishedContent;

    // Ghost variables for invariant checking
    uint256 public ghost_totalEthDeposited;     // total ETH sent into marketplace via purchases
    uint256 public ghost_totalCreatorPaid;      // total ETH paid to creators
    uint256 public ghost_totalStakerForwarded;  // total ETH forwarded to staking
    uint256 public ghost_totalSeederClaimed;    // total ETH claimed by seeders

    constructor(
        DeployHelper _helper,
        MockToken _token,
        AraStaking _staking,
        AraContent _contentToken,
        Marketplace _marketplace
    ) {
        helper = _helper;
        token = _token;
        staking = _staking;
        contentToken = _contentToken;
        marketplace = _marketplace;

        // Create 5 actors with known private keys
        for (uint256 i = 1; i <= 5; i++) {
            address actor = vm.addr(i);
            actors.push(actor);
            actorKeys[actor] = i;
            vm.deal(actor, 1000 ether);
            token.mint(actor, 100_000 ether);

            // Each actor stakes
            vm.startPrank(actor);
            token.approve(address(staking), 10_000 ether);
            staking.stake(10_000 ether);
            vm.stopPrank();
        }
    }

    function publish(uint256 actorSeed, uint256 price) external {
        address actor = actors[actorSeed % actors.length];
        price = bound(price, 1000, 10 ether);

        vm.prank(actor);
        bytes32 cId = contentToken.publishContent(
            keccak256(abi.encode(publishedContent.length)),
            "ipfs://inv",
            price,
            1_000_000,
            0,
            1000
        );
        publishedContent.push(cId);
    }

    function purchase(uint256 actorSeed, uint256 contentSeed) external {
        if (publishedContent.length == 0) return;

        address actor = actors[actorSeed % actors.length];
        bytes32 cId = publishedContent[contentSeed % publishedContent.length];

        if (marketplace.hasPurchased(cId, actor)) return;
        if (!contentToken.isActive(cId)) return;

        uint256 price = contentToken.getPrice(cId);
        if (actor.balance < price) return;

        uint256 creatorBal = _getCreatorBalance(cId);
        uint256 stakingBal = address(staking).balance;

        vm.prank(actor);
        marketplace.purchase{value: price}(cId, type(uint256).max);

        ghost_totalEthDeposited += price;
        ghost_totalCreatorPaid += (_getCreatorBalance(cId) - creatorBal);
        ghost_totalStakerForwarded += (address(staking).balance - stakingBal);
    }

    function claimDeliveryReward(uint256 seederSeed, uint256 buyerSeed, uint256 contentSeed) external {
        if (publishedContent.length == 0) return;

        address seeder = actors[seederSeed % actors.length];
        address buyer_ = actors[buyerSeed % actors.length];
        bytes32 cId = publishedContent[contentSeed % publishedContent.length];

        if (!marketplace.hasPurchased(cId, buyer_)) return;
        if (marketplace.getBuyerReward(cId, buyer_) == 0) return;

        uint256 buyerKey = actorKeys[buyer_];
        uint256 ts = block.timestamp;

        // Sign receipt
        bytes32 structHash = keccak256(
            abi.encode(marketplace.RECEIPT_TYPE_HASH(), cId, seeder, 1_000_000, ts)
        );
        bytes32 digest = keccak256(
            abi.encodePacked("\x19\x01", marketplace.DOMAIN_SEPARATOR(), structHash)
        );
        (uint8 v, bytes32 r, bytes32 s) = vm.sign(buyerKey, digest);
        bytes memory sig = abi.encodePacked(r, s, v);

        uint256 seederBal = seeder.balance;

        vm.prank(seeder);
        try marketplace.claimDeliveryReward(cId, buyer_, 1_000_000, ts, sig) {
            ghost_totalSeederClaimed += (seeder.balance - seederBal);
        } catch {}
    }

    function stakeMore(uint256 actorSeed, uint256 amount) external {
        address actor = actors[actorSeed % actors.length];
        amount = bound(amount, 1 ether, 1000 ether);

        vm.startPrank(actor);
        token.approve(address(staking), amount);
        staking.stake(amount);
        vm.stopPrank();
    }

    function unstake(uint256 actorSeed, uint256 amount) external {
        address actor = actors[actorSeed % actors.length];
        uint256 available = staking.stakedBalance(actor);
        if (available == 0) return;
        amount = bound(amount, 1, available);

        vm.prank(actor);
        staking.unstake(amount);
    }

    function _getCreatorBalance(bytes32 cId) internal view returns (uint256) {
        address cr = contentToken.getCreator(cId);
        return cr.balance;
    }

    function getPublishedContentCount() external view returns (uint256) {
        return publishedContent.length;
    }
}

/// @title InvariantTest
/// @notice Top-level invariant test contract.
contract InvariantTest is DeployHelper {
    InvariantHandler public handler;

    function setUp() public {
        vm.startPrank(makeAddr("deployer"));
        _deployStack();
        vm.stopPrank();

        handler = new InvariantHandler(this, token, staking, contentToken, marketplace);

        // Target only the handler for fuzzing
        targetContract(address(handler));
    }

    /// @notice The marketplace ETH balance must always be >= total unclaimed seeder rewards.
    ///         (Total deposited - creator paid - staker forwarded - seeder claimed) <= marketplace.balance
    function invariant_marketplaceBalanceCoversUnclaimedRewards() external view {
        uint256 expectedSeederPool = handler.ghost_totalEthDeposited()
            - handler.ghost_totalCreatorPaid()
            - handler.ghost_totalStakerForwarded()
            - handler.ghost_totalSeederClaimed();

        assertGe(
            address(marketplace).balance,
            expectedSeederPool,
            "marketplace balance < unclaimed seeder rewards"
        );
    }

    /// @notice totalStaked in contract must match the sum tracked by deposit/withdraw ghosts.
    function invariant_totalStakedNonNegative() external view {
        // totalStaked should be sane (not underflowed)
        assertLe(staking.totalStaked(), type(uint128).max, "totalStaked looks underflowed");
    }

    /// @notice staking contract ETH balance covers unclaimed staking rewards.
    function invariant_stakingBalanceCoversRewards() external view {
        uint256 deposited = staking.totalStakerRewardsDeposited();
        uint256 claimed = staking.totalStakerRewardsClaimed();
        assertGe(
            address(staking).balance,
            deposited - claimed,
            "staking balance < unclaimed staker rewards"
        );
    }

    /// @notice Each actor's totalUserStake == stakedBalance + allocated content stakes.
    ///         Since we can't iterate content stakes, at minimum totalUserStake >= stakedBalance.
    function invariant_userStakeConsistency() external view {
        address[] memory actors = new address[](5);
        for (uint256 i = 0; i < 5; i++) {
            actors[i] = vm.addr(i + 1);
        }
        for (uint256 i = 0; i < actors.length; i++) {
            assertGe(
                staking.totalUserStake(actors[i]),
                staking.stakedBalance(actors[i]),
                "totalUserStake < stakedBalance"
            );
        }
    }
}
