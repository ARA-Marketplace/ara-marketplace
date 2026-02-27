// SPDX-License-Identifier: LGPL-3.0
pragma solidity ^0.8.24;

import {DeployHelper} from "./helpers/DeployHelper.sol";
import {AraCollections} from "../src/AraCollections.sol";

contract AraCollectionsTest is DeployHelper {
    address public alice = makeAddr("alice");
    address public bob = makeAddr("bob");

    bytes32 public contentHash = keccak256("test-content");
    string public metadataURI = "ipfs://QmTest";
    uint256 public contentPrice = 0.1 ether;
    uint256 public fileSize = 1_000_000;

    bytes32 public contentId;

    function setUp() public {
        _deployStack();

        // Give alice tokens and stake so she can publish
        token.mint(alice, 10_000 ether);
        vm.startPrank(alice);
        token.approve(address(staking), 2000 ether);
        staking.stake(2000 ether);
        contentId = contentToken.publishContent(contentHash, metadataURI, contentPrice, fileSize, 0, 1000);
        vm.stopPrank();
    }

    function test_CreateCollection() public {
        vm.prank(alice);
        uint256 collId = collections.createCollection("My Collection", "A test collection", "ipfs://banner");

        assertEq(collId, 1);
        (address creator, string memory name,,, uint256 createdAt, bool active) = collections.collections(collId);
        assertEq(creator, alice);
        assertEq(name, "My Collection");
        assertTrue(active);
        assertGt(createdAt, 0);
    }

    function test_CreateMultipleCollections() public {
        vm.startPrank(alice);
        uint256 id1 = collections.createCollection("First", "", "");
        uint256 id2 = collections.createCollection("Second", "", "");
        vm.stopPrank();

        assertEq(id1, 1);
        assertEq(id2, 2);

        uint256[] memory aliceColls = collections.getCreatorCollections(alice);
        assertEq(aliceColls.length, 2);
        assertEq(aliceColls[0], 1);
        assertEq(aliceColls[1], 2);
    }

    function test_RevertCreateEmptyName() public {
        vm.prank(alice);
        vm.expectRevert("Invalid name length");
        collections.createCollection("", "desc", "");
    }

    function test_UpdateCollection() public {
        vm.startPrank(alice);
        uint256 collId = collections.createCollection("Old Name", "Old desc", "old-banner");
        collections.updateCollection(collId, "New Name", "New desc", "new-banner");
        vm.stopPrank();

        (, string memory name, string memory desc, string memory banner,,) = collections.collections(collId);
        assertEq(name, "New Name");
        assertEq(desc, "New desc");
        assertEq(banner, "new-banner");
    }

    function test_RevertUpdateNotCreator() public {
        vm.prank(alice);
        uint256 collId = collections.createCollection("Alice's", "", "");

        vm.prank(bob);
        vm.expectRevert("Not collection creator");
        collections.updateCollection(collId, "Hacked", "", "");
    }

    function test_AddItem() public {
        vm.startPrank(alice);
        uint256 collId = collections.createCollection("Games", "", "");
        collections.addItem(collId, contentId);
        vm.stopPrank();

        assertEq(collections.contentCollection(contentId), collId);
        assertEq(collections.getCollectionItemCount(collId), 1);

        bytes32[] memory items = collections.getCollectionItems(collId);
        assertEq(items.length, 1);
        assertEq(items[0], contentId);
    }

    function test_RevertAddItemNotContentCreator() public {
        // Bob creates a collection but tries to add Alice's content
        vm.prank(bob);
        uint256 collId = collections.createCollection("Bob's", "", "");

        vm.prank(bob);
        vm.expectRevert("Not content creator");
        collections.addItem(collId, contentId);
    }

    function test_RevertAddItemAlreadyInCollection() public {
        vm.startPrank(alice);
        uint256 collId = collections.createCollection("Games", "", "");
        collections.addItem(collId, contentId);

        vm.expectRevert("Already in a collection");
        collections.addItem(collId, contentId);
        vm.stopPrank();
    }

    function test_RemoveItem() public {
        vm.startPrank(alice);
        uint256 collId = collections.createCollection("Games", "", "");
        collections.addItem(collId, contentId);
        collections.removeItem(collId, contentId);
        vm.stopPrank();

        assertEq(collections.contentCollection(contentId), 0);
        assertEq(collections.getCollectionItemCount(collId), 0);
    }

    function test_RevertRemoveItemNotInCollection() public {
        vm.startPrank(alice);
        uint256 collId = collections.createCollection("Games", "", "");

        vm.expectRevert("Not in this collection");
        collections.removeItem(collId, contentId);
        vm.stopPrank();
    }

    function test_DeleteCollection() public {
        vm.startPrank(alice);
        uint256 collId = collections.createCollection("Games", "", "");
        collections.addItem(collId, contentId);
        collections.deleteCollection(collId);
        vm.stopPrank();

        (,,,,, bool active) = collections.collections(collId);
        assertFalse(active);
        // Item should be unlinked
        assertEq(collections.contentCollection(contentId), 0);
        assertEq(collections.getCollectionItemCount(collId), 0);
    }

    function test_RevertDeleteNotCreator() public {
        vm.prank(alice);
        uint256 collId = collections.createCollection("Alice's", "", "");

        vm.prank(bob);
        vm.expectRevert("Not collection creator");
        collections.deleteCollection(collId);
    }

    function test_RevertOperateOnDeletedCollection() public {
        vm.startPrank(alice);
        uint256 collId = collections.createCollection("Games", "", "");
        collections.deleteCollection(collId);

        vm.expectRevert("Collection deleted");
        collections.updateCollection(collId, "new", "", "");
        vm.stopPrank();
    }

    function test_MultipleItemsInCollection() public {
        // Publish a second content item
        vm.startPrank(alice);
        bytes32 contentId2 = contentToken.publishContent(
            keccak256("test-content-2"), "ipfs://QmTest2", 0.2 ether, 2_000_000, 0, 500
        );

        uint256 collId = collections.createCollection("Games", "My games", "");
        collections.addItem(collId, contentId);
        collections.addItem(collId, contentId2);
        vm.stopPrank();

        assertEq(collections.getCollectionItemCount(collId), 2);
        bytes32[] memory items = collections.getCollectionItems(collId);
        assertEq(items[0], contentId);
        assertEq(items[1], contentId2);
    }

    function test_RemoveItemSwapsWithLast() public {
        // Publish two more content items
        vm.startPrank(alice);
        bytes32 contentId2 = contentToken.publishContent(
            keccak256("test-content-2"), "ipfs://QmTest2", 0.2 ether, 2_000_000, 0, 0
        );
        bytes32 contentId3 = contentToken.publishContent(
            keccak256("test-content-3"), "ipfs://QmTest3", 0.3 ether, 3_000_000, 0, 0
        );

        uint256 collId = collections.createCollection("Games", "", "");
        collections.addItem(collId, contentId);
        collections.addItem(collId, contentId2);
        collections.addItem(collId, contentId3);

        // Remove the first item — last item (contentId3) should swap in
        collections.removeItem(collId, contentId);
        vm.stopPrank();

        assertEq(collections.getCollectionItemCount(collId), 2);
        bytes32[] memory items = collections.getCollectionItems(collId);
        assertEq(items[0], contentId3); // swapped from end
        assertEq(items[1], contentId2);
    }

    function test_EmitCollectionCreated() public {
        vm.prank(alice);
        vm.expectEmit(false, true, false, true);
        emit AraCollections.CollectionCreated(1, alice, "My Games");
        collections.createCollection("My Games", "", "");
    }

    function test_EmitItemAdded() public {
        vm.startPrank(alice);
        uint256 collId = collections.createCollection("Games", "", "");

        vm.expectEmit(true, true, false, false);
        emit AraCollections.ItemAddedToCollection(collId, contentId);
        collections.addItem(collId, contentId);
        vm.stopPrank();
    }

    function test_EmitItemRemoved() public {
        vm.startPrank(alice);
        uint256 collId = collections.createCollection("Games", "", "");
        collections.addItem(collId, contentId);

        vm.expectEmit(true, true, false, false);
        emit AraCollections.ItemRemovedFromCollection(collId, contentId);
        collections.removeItem(collId, contentId);
        vm.stopPrank();
    }
}
