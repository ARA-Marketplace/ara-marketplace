// SPDX-License-Identifier: LGPL-3.0
pragma solidity ^0.8.24;

import {Test, console} from "forge-std/Test.sol";
import {AraStaking} from "../src/AraStaking.sol";
import {ContentRegistry} from "../src/ContentRegistry.sol";

// Reuse mock from staking tests
contract MockAraToken2 {
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
        require(allowance[from][msg.sender] >= amount, "Insufficient allowance");
        allowance[from][msg.sender] -= amount;
        balanceOf[from] -= amount;
        balanceOf[to] += amount;
        return true;
    }
}

contract ContentRegistryTest is Test {
    AraStaking public staking;
    ContentRegistry public registry;
    MockAraToken2 public token;

    address public creator = makeAddr("creator");
    address public nobody = makeAddr("nobody");

    uint256 public constant PUBLISHER_MIN = 1000 ether;
    uint256 public constant SEEDER_MIN = 100 ether;

    bytes32 public contentHash = keccak256("game-file-data");
    string public metadataURI = "ipfs://QmTest123";
    uint256 public price = 0.1 ether;

    function setUp() public {
        token = new MockAraToken2();
        staking = new AraStaking(address(token), PUBLISHER_MIN, SEEDER_MIN);
        registry = new ContentRegistry(address(staking));

        // Fund and stake for the creator
        token.mint(creator, 10_000 ether);
        vm.startPrank(creator);
        token.approve(address(staking), PUBLISHER_MIN);
        staking.stake(PUBLISHER_MIN);
        vm.stopPrank();
    }

    function test_PublishContent() public {
        vm.prank(creator);
        bytes32 contentId = registry.publishContent(contentHash, metadataURI, price, 1_000_000);

        assertEq(registry.getCreator(contentId), creator);
        assertEq(registry.getContentHash(contentId), contentHash);
        assertEq(registry.getPrice(contentId), price);
        assertTrue(registry.isActive(contentId));
        assertEq(registry.getContentCount(), 1);
        assertEq(registry.getFileSize(contentId), 1_000_000);
    }

    function test_RevertPublishWithoutStake() public {
        vm.prank(nobody);
        vm.expectRevert();
        registry.publishContent(contentHash, metadataURI, price, 1_000_000);
    }

    function test_PublishSameFileTwice() public {
        vm.startPrank(creator);
        bytes32 id1 = registry.publishContent(contentHash, metadataURI, price, 1_000_000);
        bytes32 id2 = registry.publishContent(contentHash, "ipfs://QmSecond", 0.2 ether, 2_000_000);
        vm.stopPrank();

        // Same file, same creator → two different contentIds (nonce makes them unique)
        assertTrue(id1 != id2);
        assertTrue(registry.isActive(id1));
        assertTrue(registry.isActive(id2));
        assertEq(registry.getContentCount(), 2);
        assertEq(registry.getPrice(id1), price);
        assertEq(registry.getPrice(id2), 0.2 ether);
        assertEq(registry.getCreator(id1), creator);
        assertEq(registry.getCreator(id2), creator);
    }

    function test_RevertPublishZeroPrice() public {
        vm.prank(creator);
        vm.expectRevert();
        registry.publishContent(contentHash, metadataURI, 0, 1_000_000);
    }

    function test_UpdateContent() public {
        vm.startPrank(creator);
        bytes32 contentId = registry.publishContent(contentHash, metadataURI, price, 1_000_000);
        registry.updateContent(contentId, 0.2 ether, "ipfs://QmUpdated");
        vm.stopPrank();

        assertEq(registry.getPrice(contentId), 0.2 ether);
    }

    function test_RevertUpdateByNonCreator() public {
        vm.prank(creator);
        bytes32 contentId = registry.publishContent(contentHash, metadataURI, price, 1_000_000);

        vm.prank(nobody);
        vm.expectRevert();
        registry.updateContent(contentId, 0.2 ether, "ipfs://QmHacked");
    }

    function test_DelistContent() public {
        vm.startPrank(creator);
        bytes32 contentId = registry.publishContent(contentHash, metadataURI, price, 1_000_000);
        registry.delistContent(contentId);
        vm.stopPrank();

        assertFalse(registry.isActive(contentId));
    }

    function test_RevertDelistByNonCreator() public {
        vm.prank(creator);
        bytes32 contentId = registry.publishContent(contentHash, metadataURI, price, 1_000_000);

        vm.prank(nobody);
        vm.expectRevert();
        registry.delistContent(contentId);
    }
}
