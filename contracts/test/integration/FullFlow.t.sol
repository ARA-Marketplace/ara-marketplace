// SPDX-License-Identifier: LGPL-3.0
pragma solidity ^0.8.24;

import {Test, console} from "forge-std/Test.sol";
import {ERC1967Proxy} from "@openzeppelin/contracts/proxy/ERC1967/ERC1967Proxy.sol";
import {AraStaking} from "../../src/AraStaking.sol";
import {AraContent} from "../../src/AraContent.sol";
import {Marketplace} from "../../src/Marketplace.sol";
import {IAraToken} from "../../src/interfaces/IAraToken.sol";

/// @notice Integration test that can run against a mainnet fork with the real ARA token.
/// Usage: forge test --match-contract FullFlowForkTest --fork-url $ETH_RPC_URL
contract FullFlowForkTest is Test {
    // Mainnet ARA token
    address constant ARA_TOKEN = 0xa92E7c82B11d10716aB534051B271D2f6aEf7Df5;

    AraStaking public staking;
    AraContent public contentToken;
    Marketplace public marketplace;

    address public deployer;
    address public creator;
    address public seeder;

    uint256 public buyerPrivKey = 0xBEEF;
    address public buyer;

    uint256 public constant PUBLISHER_MIN = 1000 ether;
    uint256 public constant SEEDER_MIN = 100 ether;
    uint256 public constant CREATOR_SHARE_BPS = 8500;
    uint256 public constant RESALE_REWARD_BPS = 400;
    uint256 public constant STAKER_REWARD_BPS = 250;
    uint256 public constant RESALE_STAKER_REWARD_BPS = 100;
    uint256 public constant FILE_SIZE = 1_000_000;

    function setUp() public {
        deployer = makeAddr("deployer");
        creator = makeAddr("creator");
        buyer = vm.addr(buyerPrivKey);
        seeder = makeAddr("seeder");

        vm.startPrank(deployer);

        AraStaking stakingImpl = new AraStaking();
        AraContent contentImpl = new AraContent();
        Marketplace marketplaceImpl = new Marketplace();

        ERC1967Proxy stakingProxy = new ERC1967Proxy(
            address(stakingImpl), abi.encodeCall(AraStaking.initialize, (ARA_TOKEN, PUBLISHER_MIN, SEEDER_MIN))
        );
        staking = AraStaking(address(stakingProxy));

        ERC1967Proxy contentProxy =
            new ERC1967Proxy(address(contentImpl), abi.encodeCall(AraContent.initialize, (address(stakingProxy))));
        contentToken = AraContent(address(contentProxy));

        ERC1967Proxy marketplaceProxy = new ERC1967Proxy(
            address(marketplaceImpl),
            abi.encodeCall(
                Marketplace.initialize,
                (address(contentProxy), address(stakingProxy), CREATOR_SHARE_BPS, RESALE_REWARD_BPS)
            )
        );
        marketplace = Marketplace(payable(address(marketplaceProxy)));

        contentToken.setMinter(address(marketplace));

        // V2 initialization: passive staker rewards
        staking.initializeV2(address(marketplace));
        marketplace.initializeV2(STAKER_REWARD_BPS, RESALE_STAKER_REWARD_BPS, RESALE_REWARD_BPS);

        vm.stopPrank();

        vm.deal(buyer, 10 ether);
        deal(ARA_TOKEN, creator, 10_000 ether);
        deal(ARA_TOKEN, seeder, 5_000 ether);
    }

    function _receiptHash(bytes32 cId, address seederAddr, uint256 bytesServedVal, uint256 ts)
        internal
        view
        returns (bytes32)
    {
        bytes32 structHash =
            keccak256(abi.encode(marketplace.RECEIPT_TYPE_HASH(), cId, seederAddr, bytesServedVal, ts));
        return keccak256(abi.encodePacked("\x19\x01", marketplace.DOMAIN_SEPARATOR(), structHash));
    }

    function _signReceipt(uint256 privateKey, bytes32 cId, address seederAddr, uint256 bytesServedVal, uint256 ts)
        internal
        view
        returns (bytes memory)
    {
        bytes32 hash = _receiptHash(cId, seederAddr, bytesServedVal, ts);
        (uint8 v, bytes32 r, bytes32 s) = vm.sign(privateKey, hash);
        return abi.encodePacked(r, s, v);
    }

    function test_FullFlowOnFork() public {
        vm.startPrank(creator);
        IAraToken(ARA_TOKEN).approve(address(staking), PUBLISHER_MIN);
        staking.stake(PUBLISHER_MIN);
        assertTrue(staking.isEligiblePublisher(creator));

        bytes32 contentHash = keccak256("my-awesome-game");
        bytes32 contentId =
            contentToken.publishContent(contentHash, "ipfs://QmMetadata", 0.1 ether, FILE_SIZE, 0, 1000);
        vm.stopPrank();

        vm.startPrank(seeder);
        IAraToken(ARA_TOKEN).approve(address(staking), SEEDER_MIN);
        staking.stake(SEEDER_MIN);
        staking.stakeForContent(contentId, SEEDER_MIN);
        assertTrue(staking.isEligibleSeeder(seeder, contentId));
        vm.stopPrank();

        uint256 creatorBalBefore = creator.balance;
        vm.prank(buyer);
        marketplace.purchase{value: 0.1 ether}(contentId);
        assertTrue(marketplace.hasPurchased(contentId, buyer));
        assertEq(contentToken.balanceOf(buyer, uint256(contentId)), 1);

        uint256 creatorPayment = (0.1 ether * 8500) / 10_000;
        uint256 stakerReward = (0.1 ether * STAKER_REWARD_BPS) / 10_000;
        assertEq(creator.balance - creatorBalBefore, creatorPayment);
        assertEq(marketplace.getBuyerReward(contentId, buyer), 0.1 ether - creatorPayment - stakerReward);

        uint256 ts = block.timestamp;
        bytes memory sig = _signReceipt(buyerPrivKey, contentId, seeder, FILE_SIZE, ts);

        uint256 seederBalBefore = seeder.balance;
        uint256 expectedReward = marketplace.getBuyerReward(contentId, buyer);

        vm.prank(seeder);
        marketplace.claimDeliveryReward(contentId, buyer, FILE_SIZE, ts, sig);

        assertEq(seeder.balance - seederBalBefore, expectedReward);
        assertEq(marketplace.getBuyerReward(contentId, buyer), 0);

        console.log("Full flow completed successfully!");
        console.log("Creator received:", creatorPayment, "wei");
        console.log("Staker reward:", stakerReward, "wei");
        console.log("Seeder earned:", expectedReward, "wei");
    }
}
