// SPDX-License-Identifier: LGPL-3.0
pragma solidity ^0.8.24;

import {DeployHelper} from "./helpers/DeployHelper.sol";
import {AraContent} from "../src/AraContent.sol";

contract AraContentTest is DeployHelper {
    address public creator = makeAddr("creator");
    address public nobody = makeAddr("nobody");
    address public buyer1 = makeAddr("buyer1");
    address public buyer2 = makeAddr("buyer2");

    bytes32 public contentHash = keccak256("game-file-data");
    string public metadataURI = "ipfs://QmTest123";
    uint256 public price = 0.1 ether;
    uint256 public fileSize = 1_000_000;

    function setUp() public {
        _deployStack();

        // Fund and stake for the creator
        token.mint(creator, 10_000 ether);
        vm.startPrank(creator);
        token.approve(address(staking), PUBLISHER_MIN);
        staking.stake(PUBLISHER_MIN);
        vm.stopPrank();
    }

    // --- Publishing ---

    function test_PublishContent() public {
        vm.prank(creator);
        bytes32 contentId = contentToken.publishContent(contentHash, metadataURI, price, fileSize, 0, 1000);

        assertEq(contentToken.getCreator(contentId), creator);
        assertEq(contentToken.getContentHash(contentId), contentHash);
        assertEq(contentToken.getPrice(contentId), price);
        assertTrue(contentToken.isActive(contentId));
        assertEq(contentToken.getContentCount(), 1);
        assertEq(contentToken.getFileSize(contentId), fileSize);
        assertEq(contentToken.getMaxSupply(contentId), 0); // unlimited
    }

    function test_PublishContentWithLimitedEdition() public {
        vm.prank(creator);
        bytes32 contentId = contentToken.publishContent(contentHash, metadataURI, price, fileSize, 100, 1000);

        assertEq(contentToken.getMaxSupply(contentId), 100);
        assertEq(contentToken.getTotalMinted(contentId), 0); // nothing minted yet
    }

    function test_RevertPublishWithoutStake() public {
        vm.prank(nobody);
        vm.expectRevert();
        contentToken.publishContent(contentHash, metadataURI, price, fileSize, 0, 0);
    }

    function test_PublishSameFileTwice() public {
        vm.startPrank(creator);
        bytes32 id1 = contentToken.publishContent(contentHash, metadataURI, price, fileSize, 0, 0);
        bytes32 id2 = contentToken.publishContent(contentHash, "ipfs://QmSecond", 0.2 ether, 2_000_000, 0, 0);
        vm.stopPrank();

        assertTrue(id1 != id2);
        assertTrue(contentToken.isActive(id1));
        assertTrue(contentToken.isActive(id2));
        assertEq(contentToken.getContentCount(), 2);
    }

    function test_RevertPublishZeroPrice() public {
        vm.prank(creator);
        vm.expectRevert();
        contentToken.publishContent(contentHash, metadataURI, 0, fileSize, 0, 0);
    }

    function test_RevertPublishZeroFileSize() public {
        vm.prank(creator);
        vm.expectRevert(AraContent.ZeroFileSize.selector);
        contentToken.publishContent(contentHash, metadataURI, price, 0, 0, 0);
    }

    // --- Minting ---

    function test_MintByMarketplace() public {
        vm.prank(creator);
        bytes32 contentId = contentToken.publishContent(contentHash, metadataURI, price, fileSize, 0, 0);

        // Marketplace is the minter
        vm.prank(address(marketplace));
        contentToken.mint(buyer1, contentId);

        assertEq(contentToken.balanceOf(buyer1, uint256(contentId)), 1);
        assertEq(contentToken.getTotalMinted(contentId), 1);
    }

    function test_RevertMintByNonMinter() public {
        vm.prank(creator);
        bytes32 contentId = contentToken.publishContent(contentHash, metadataURI, price, fileSize, 0, 0);

        vm.prank(nobody);
        vm.expectRevert(AraContent.OnlyMinter.selector);
        contentToken.mint(buyer1, contentId);
    }

    function test_LimitedEditionEnforcesMaxSupply() public {
        vm.prank(creator);
        bytes32 contentId = contentToken.publishContent(contentHash, metadataURI, price, fileSize, 2, 0);

        vm.startPrank(address(marketplace));
        contentToken.mint(buyer1, contentId);
        contentToken.mint(buyer2, contentId);

        // Third mint should revert
        vm.expectRevert(AraContent.EditionSoldOut.selector);
        contentToken.mint(makeAddr("buyer3"), contentId);
        vm.stopPrank();

        assertEq(contentToken.getTotalMinted(contentId), 2);
    }

    function test_UnlimitedEditionNoCapEnforced() public {
        vm.prank(creator);
        bytes32 contentId = contentToken.publishContent(contentHash, metadataURI, price, fileSize, 0, 0);

        vm.startPrank(address(marketplace));
        for (uint256 i = 0; i < 50; i++) {
            contentToken.mint(makeAddr(string(abi.encodePacked("buyer", i))), contentId);
        }
        vm.stopPrank();

        assertEq(contentToken.getTotalMinted(contentId), 50);
    }

    // --- ERC-1155 Token Transfers ---

    function test_TokenTransfer() public {
        vm.prank(creator);
        bytes32 contentId = contentToken.publishContent(contentHash, metadataURI, price, fileSize, 0, 0);

        vm.prank(address(marketplace));
        contentToken.mint(buyer1, contentId);

        uint256 tokenId = uint256(contentId);

        // Buyer1 transfers to buyer2
        vm.prank(buyer1);
        contentToken.safeTransferFrom(buyer1, buyer2, tokenId, 1, "");

        assertEq(contentToken.balanceOf(buyer1, tokenId), 0);
        assertEq(contentToken.balanceOf(buyer2, tokenId), 1);
    }

    // --- ERC-2981 Royalties ---

    function test_RoyaltyInfo() public {
        vm.prank(creator);
        bytes32 contentId = contentToken.publishContent(contentHash, metadataURI, price, fileSize, 0, 1000); // 10% royalty

        uint256 salePrice = 1 ether;
        (address receiver, uint256 royaltyAmount) = contentToken.royaltyInfo(uint256(contentId), salePrice);

        assertEq(receiver, creator);
        assertEq(royaltyAmount, 0.1 ether); // 10% of 1 ETH
    }

    function test_RoyaltyInfoZeroBps() public {
        vm.prank(creator);
        bytes32 contentId = contentToken.publishContent(contentHash, metadataURI, price, fileSize, 0, 0); // 0% royalty

        (address receiver, uint256 royaltyAmount) = contentToken.royaltyInfo(uint256(contentId), 1 ether);

        // No royalty set → returns (address(0), 0)
        assertEq(receiver, address(0));
        assertEq(royaltyAmount, 0);
    }

    // --- URI ---

    function test_UriReturnsMetadata() public {
        vm.prank(creator);
        bytes32 contentId = contentToken.publishContent(contentHash, metadataURI, price, fileSize, 0, 0);

        string memory returnedUri = contentToken.uri(uint256(contentId));
        assertEq(returnedUri, metadataURI);
    }

    // --- Content management ---

    function test_UpdateContent() public {
        vm.startPrank(creator);
        bytes32 contentId = contentToken.publishContent(contentHash, metadataURI, price, fileSize, 0, 0);
        contentToken.updateContent(contentId, 0.2 ether, "ipfs://QmUpdated");
        vm.stopPrank();

        assertEq(contentToken.getPrice(contentId), 0.2 ether);
    }

    function test_RevertUpdateByNonCreator() public {
        vm.prank(creator);
        bytes32 contentId = contentToken.publishContent(contentHash, metadataURI, price, fileSize, 0, 0);

        vm.prank(nobody);
        vm.expectRevert();
        contentToken.updateContent(contentId, 0.2 ether, "ipfs://QmHacked");
    }

    function test_UpdateContentFile() public {
        vm.prank(creator);
        bytes32 contentId = contentToken.publishContent(contentHash, metadataURI, price, fileSize, 0, 0);

        bytes32 newHash = keccak256("game-file-v2-data");
        vm.prank(creator);
        contentToken.updateContentFile(contentId, newHash);

        assertEq(contentToken.getContentHash(contentId), newHash);
    }

    function test_UpdateFileSize() public {
        vm.prank(creator);
        bytes32 contentId = contentToken.publishContent(contentHash, metadataURI, price, fileSize, 0, 0);

        uint256 newSize = 2_000_000;
        vm.prank(creator);
        contentToken.updateFileSize(contentId, newSize);

        assertEq(contentToken.getFileSize(contentId), newSize);
    }

    function test_DelistContent() public {
        vm.startPrank(creator);
        bytes32 contentId = contentToken.publishContent(contentHash, metadataURI, price, fileSize, 0, 0);
        contentToken.delistContent(contentId);
        vm.stopPrank();

        assertFalse(contentToken.isActive(contentId));
    }

    function test_RevertDelistByNonCreator() public {
        vm.prank(creator);
        bytes32 contentId = contentToken.publishContent(contentHash, metadataURI, price, fileSize, 0, 0);

        vm.prank(nobody);
        vm.expectRevert();
        contentToken.delistContent(contentId);
    }

    // --- supportsInterface ---

    function test_SupportsERC1155() public view {
        assertTrue(contentToken.supportsInterface(0xd9b67a26)); // ERC-1155
    }

    function test_SupportsERC2981() public view {
        assertTrue(contentToken.supportsInterface(0x2a55205a)); // ERC-2981
    }
}
