// SPDX-License-Identifier: LGPL-3.0
pragma solidity ^0.8.24;

import {AraStaking} from "./AraStaking.sol";

/// @title ContentRegistry
/// @notice On-chain registry of published content. Each piece of content is identified
///         by a contentId derived from its BLAKE3 hash and creator address.
///         Creators must have sufficient ARA staked to publish.
contract ContentRegistry {
    AraStaking public immutable staking;

    struct Content {
        address creator;
        bytes32 contentHash; // BLAKE3 hash from iroh (P2P content identifier)
        string metadataURI; // IPFS/Arweave URI for metadata JSON
        uint256 priceWei; // Price in ETH (wei)
        uint256 createdAt;
        bool active;
    }

    /// @notice contentId => Content
    mapping(bytes32 => Content) public contents;

    /// @notice contentId => file size in bytes (for proportional reward calculation)
    mapping(bytes32 => uint256) public fileSizes;

    /// @notice creator => list of contentIds
    mapping(address => bytes32[]) public creatorContents;

    /// @notice All content IDs for enumeration
    bytes32[] public allContentIds;

    /// @notice Per-creator nonce so the same file can be published multiple times
    mapping(address => uint256) public publisherNonce;

    event ContentPublished(
        bytes32 indexed contentId,
        address indexed creator,
        bytes32 contentHash,
        string metadataURI,
        uint256 priceWei,
        uint256 fileSize
    );
    event ContentUpdated(bytes32 indexed contentId, uint256 newPriceWei, string newMetadataURI);
    event ContentFileUpdated(bytes32 indexed contentId, bytes32 oldHash, bytes32 newHash, address indexed creator);
    event ContentDelisted(bytes32 indexed contentId);

    error InsufficientStake();
    error ContentAlreadyExists(bytes32 contentId);
    error NotContentCreator();
    error ContentNotActive();
    error ZeroPrice();
    error ZeroFileSize();

    constructor(address _staking) {
        staking = AraStaking(_staking);
    }

    /// @notice Register new content on the marketplace.
    /// @param contentHash BLAKE3 hash of the content blob (from iroh)
    /// @param metadataURI URI pointing to content metadata JSON
    /// @param priceWei Price in wei that buyers must pay
    /// @param fileSize Size of the content file in bytes
    /// @return contentId The unique identifier for this content
    function publishContent(bytes32 contentHash, string calldata metadataURI, uint256 priceWei, uint256 fileSize)
        external
        returns (bytes32 contentId)
    {
        if (!staking.isEligiblePublisher(msg.sender)) revert InsufficientStake();
        if (priceWei == 0) revert ZeroPrice();
        if (fileSize == 0) revert ZeroFileSize();

        uint256 nonce = publisherNonce[msg.sender];
        contentId = keccak256(abi.encodePacked(contentHash, msg.sender, nonce));
        publisherNonce[msg.sender] = nonce + 1;
        if (contents[contentId].creator != address(0)) revert ContentAlreadyExists(contentId);

        contents[contentId] = Content({
            creator: msg.sender,
            contentHash: contentHash,
            metadataURI: metadataURI,
            priceWei: priceWei,
            createdAt: block.timestamp,
            active: true
        });

        fileSizes[contentId] = fileSize;

        creatorContents[msg.sender].push(contentId);
        allContentIds.push(contentId);

        emit ContentPublished(contentId, msg.sender, contentHash, metadataURI, priceWei, fileSize);
    }

    /// @notice Update content metadata and/or price. Only the creator can update.
    function updateContent(bytes32 contentId, uint256 newPriceWei, string calldata newMetadataURI) external {
        Content storage c = contents[contentId];
        if (c.creator != msg.sender) revert NotContentCreator();
        if (!c.active) revert ContentNotActive();

        c.priceWei = newPriceWei;
        c.metadataURI = newMetadataURI;

        emit ContentUpdated(contentId, newPriceWei, newMetadataURI);
    }

    /// @notice Replace the content file for an existing listing. Only the creator can call.
    ///         The contentId and all purchase records remain unchanged; only the BLAKE3 blob hash
    ///         (used for P2P retrieval) is updated. Buyers re-download to get the new file.
    function updateContentFile(bytes32 contentId, bytes32 newContentHash) external {
        Content storage c = contents[contentId];
        if (c.creator != msg.sender) revert NotContentCreator();
        if (!c.active) revert ContentNotActive();
        bytes32 oldHash = c.contentHash;
        c.contentHash = newContentHash;
        emit ContentFileUpdated(contentId, oldHash, newContentHash, msg.sender);
    }

    /// @notice Update file size for existing content (e.g. after file update). Creator only.
    function updateFileSize(bytes32 contentId, uint256 newFileSize) external {
        Content storage c = contents[contentId];
        if (c.creator != msg.sender) revert NotContentCreator();
        if (!c.active) revert ContentNotActive();
        if (newFileSize == 0) revert ZeroFileSize();
        fileSizes[contentId] = newFileSize;
    }

    /// @notice Delist content from the marketplace. Only the creator can delist.
    function delistContent(bytes32 contentId) external {
        Content storage c = contents[contentId];
        if (c.creator != msg.sender) revert NotContentCreator();
        c.active = false;
        emit ContentDelisted(contentId);
    }

    /// @notice Get total number of published content items
    function getContentCount() external view returns (uint256) {
        return allContentIds.length;
    }

    /// @notice Get number of content items by a specific creator
    function getCreatorContentCount(address creator) external view returns (uint256) {
        return creatorContents[creator].length;
    }

    /// @notice Get a content's hash for P2P retrieval
    function getContentHash(bytes32 contentId) external view returns (bytes32) {
        return contents[contentId].contentHash;
    }

    /// @notice Get a content's price in wei
    function getPrice(bytes32 contentId) external view returns (uint256) {
        return contents[contentId].priceWei;
    }

    /// @notice Get a content's creator address
    function getCreator(bytes32 contentId) external view returns (address) {
        return contents[contentId].creator;
    }

    /// @notice Check if content is active (listed)
    function isActive(bytes32 contentId) external view returns (bool) {
        return contents[contentId].active;
    }

    /// @notice Get a content's file size in bytes
    function getFileSize(bytes32 contentId) external view returns (uint256) {
        return fileSizes[contentId];
    }
}
