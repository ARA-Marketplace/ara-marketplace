// SPDX-License-Identifier: LGPL-3.0
pragma solidity ^0.8.24;

import {ERC1155} from "@openzeppelin/contracts/token/ERC1155/ERC1155.sol";
import {ERC1155Supply} from "@openzeppelin/contracts/token/ERC1155/extensions/ERC1155Supply.sol";
import {ERC2981} from "@openzeppelin/contracts/token/common/ERC2981.sol";
import {Initializable} from "@openzeppelin/contracts/proxy/utils/Initializable.sol";
import {UUPSUpgradeable} from "@openzeppelin/contracts/proxy/utils/UUPSUpgradeable.sol";
import {AraStaking} from "./AraStaking.sol";

/// @title AraContent
/// @notice ERC-1155 content token replacing ContentRegistry. Each content item is a token type.
///         Publishing creates the token type (no minting). Purchasing (via Marketplace) mints.
///         Supports limited editions, unlimited editions, and ERC-2981 royalties.
contract AraContent is ERC1155, ERC1155Supply, ERC2981, Initializable, UUPSUpgradeable {
    AraStaking public staking;

    struct ContentMeta {
        address creator;
        bytes32 contentHash; // BLAKE3 hash (P2P content identifier)
        string metadataURI; // IPFS/Arweave URI for metadata JSON
        uint256 priceWei; // Price in ETH (wei)
        uint256 fileSize; // File size in bytes
        uint256 maxSupply; // 0 = unlimited edition
        uint256 createdAt;
        bool active;
    }

    /// @notice contentId => content metadata
    mapping(bytes32 => ContentMeta) public contents;

    /// @notice contentId => file size (kept separate for Marketplace compat)
    mapping(bytes32 => uint256) public fileSizes;

    /// @notice creator => list of contentIds
    mapping(address => bytes32[]) public creatorContents;

    /// @notice All content IDs for enumeration
    bytes32[] public allContentIds;

    /// @notice Per-creator nonce for unique content IDs
    mapping(address => uint256) public publisherNonce;

    /// @notice Authorized minter (Marketplace contract)
    address public minter;

    /// @notice Contract owner (can upgrade, set minter)
    address public owner;

    event ContentPublished(
        bytes32 indexed contentId,
        address indexed creator,
        bytes32 contentHash,
        string metadataURI,
        uint256 priceWei,
        uint256 fileSize,
        uint256 maxSupply
    );
    event ContentUpdated(bytes32 indexed contentId, uint256 newPriceWei, string newMetadataURI);
    event ContentFileUpdated(bytes32 indexed contentId, bytes32 oldHash, bytes32 newHash, address indexed creator);
    event ContentDelisted(bytes32 indexed contentId);
    event MinterUpdated(address indexed oldMinter, address indexed newMinter);

    error InsufficientStake();
    error ContentAlreadyExists(bytes32 contentId);
    error NotContentCreator();
    error ContentNotActive();
    error ZeroPrice();
    error ZeroFileSize();
    error EditionSoldOut();
    error OnlyMinter();
    error OnlyOwner();

    modifier onlyMinter() {
        if (msg.sender != minter) revert OnlyMinter();
        _;
    }

    modifier onlyOwner() {
        if (msg.sender != owner) revert OnlyOwner();
        _;
    }

    /// @custom:oz-upgrades-unsafe-allow constructor
    constructor() ERC1155("") {
        _disableInitializers();
    }

    /// @notice Initialize the contract (called once via proxy)
    function initialize(address _staking) external initializer {
        staking = AraStaking(_staking);
        owner = msg.sender;
    }

    // --- Publishing ---

    /// @notice Register new content. Creates token type but does NOT mint.
    /// @param contentHash BLAKE3 hash of the content blob
    /// @param metadataURI URI pointing to content metadata JSON
    /// @param priceWei Price in wei
    /// @param fileSize Size of the content file in bytes
    /// @param maxSupply Maximum copies (0 = unlimited)
    /// @param royaltyBps Creator royalty on secondary sales (basis points, e.g. 1000 = 10%)
    /// @return contentId The unique identifier for this content
    function publishContent(
        bytes32 contentHash,
        string calldata metadataURI,
        uint256 priceWei,
        uint256 fileSize,
        uint256 maxSupply,
        uint96 royaltyBps
    ) external returns (bytes32 contentId) {
        if (!staking.isEligiblePublisher(msg.sender)) revert InsufficientStake();
        if (priceWei == 0) revert ZeroPrice();
        if (fileSize == 0) revert ZeroFileSize();

        uint256 nonce = publisherNonce[msg.sender];
        contentId = keccak256(abi.encodePacked(contentHash, msg.sender, nonce));
        publisherNonce[msg.sender] = nonce + 1;
        if (contents[contentId].creator != address(0)) revert ContentAlreadyExists(contentId);

        contents[contentId] = ContentMeta({
            creator: msg.sender,
            contentHash: contentHash,
            metadataURI: metadataURI,
            priceWei: priceWei,
            fileSize: fileSize,
            maxSupply: maxSupply,
            createdAt: block.timestamp,
            active: true
        });

        fileSizes[contentId] = fileSize;

        creatorContents[msg.sender].push(contentId);
        allContentIds.push(contentId);

        // Set ERC-2981 royalty for this token
        if (royaltyBps > 0) {
            _setTokenRoyalty(uint256(contentId), msg.sender, royaltyBps);
        }

        emit ContentPublished(contentId, msg.sender, contentHash, metadataURI, priceWei, fileSize, maxSupply);
    }

    // --- Minting (called by Marketplace on purchase) ---

    /// @notice Mint a content token to a buyer. Only callable by the Marketplace.
    /// @param to Buyer address
    /// @param contentId Content to mint
    function mint(address to, bytes32 contentId) external onlyMinter {
        ContentMeta storage c = contents[contentId];
        uint256 tokenId = uint256(contentId);
        if (c.maxSupply > 0 && totalSupply(tokenId) >= c.maxSupply) revert EditionSoldOut();
        _mint(to, tokenId, 1, "");
    }

    // --- Content management (creator only) ---

    /// @notice Update content metadata and/or price
    function updateContent(bytes32 contentId, uint256 newPriceWei, string calldata newMetadataURI) external {
        ContentMeta storage c = contents[contentId];
        if (c.creator != msg.sender) revert NotContentCreator();
        if (!c.active) revert ContentNotActive();

        c.priceWei = newPriceWei;
        c.metadataURI = newMetadataURI;

        emit ContentUpdated(contentId, newPriceWei, newMetadataURI);
    }

    /// @notice Replace the content file (BLAKE3 hash). ContentId stays the same.
    function updateContentFile(bytes32 contentId, bytes32 newContentHash) external {
        ContentMeta storage c = contents[contentId];
        if (c.creator != msg.sender) revert NotContentCreator();
        if (!c.active) revert ContentNotActive();
        bytes32 oldHash = c.contentHash;
        c.contentHash = newContentHash;
        emit ContentFileUpdated(contentId, oldHash, newContentHash, msg.sender);
    }

    /// @notice Update file size (e.g. after file update)
    function updateFileSize(bytes32 contentId, uint256 newFileSize) external {
        ContentMeta storage c = contents[contentId];
        if (c.creator != msg.sender) revert NotContentCreator();
        if (!c.active) revert ContentNotActive();
        if (newFileSize == 0) revert ZeroFileSize();
        c.fileSize = newFileSize;
        fileSizes[contentId] = newFileSize;
    }

    /// @notice Delist content from the marketplace
    function delistContent(bytes32 contentId) external {
        ContentMeta storage c = contents[contentId];
        if (c.creator != msg.sender) revert NotContentCreator();
        c.active = false;
        emit ContentDelisted(contentId);
    }

    // --- View functions ---

    function getContentCount() external view returns (uint256) {
        return allContentIds.length;
    }

    function getCreatorContentCount(address creator) external view returns (uint256) {
        return creatorContents[creator].length;
    }

    function getContentHash(bytes32 contentId) external view returns (bytes32) {
        return contents[contentId].contentHash;
    }

    function getPrice(bytes32 contentId) external view returns (uint256) {
        return contents[contentId].priceWei;
    }

    function getCreator(bytes32 contentId) external view returns (address) {
        return contents[contentId].creator;
    }

    function isActive(bytes32 contentId) external view returns (bool) {
        return contents[contentId].active;
    }

    function getFileSize(bytes32 contentId) external view returns (uint256) {
        return fileSizes[contentId];
    }

    function getMaxSupply(bytes32 contentId) external view returns (uint256) {
        return contents[contentId].maxSupply;
    }

    /// @notice Get total minted for a content item
    function getTotalMinted(bytes32 contentId) external view returns (uint256) {
        return totalSupply(uint256(contentId));
    }

    // --- ERC-1155 URI override ---

    /// @notice Returns per-token metadata URI
    function uri(uint256 id) public view override returns (string memory) {
        return contents[bytes32(id)].metadataURI;
    }

    // --- Admin ---

    /// @notice Set the authorized minter (Marketplace contract)
    function setMinter(address _minter) external onlyOwner {
        emit MinterUpdated(minter, _minter);
        minter = _minter;
    }

    /// @notice Transfer ownership
    function transferOwnership(address newOwner) external onlyOwner {
        owner = newOwner;
    }

    // --- Required overrides ---

    function _update(address from, address to, uint256[] memory ids, uint256[] memory values)
        internal
        override(ERC1155, ERC1155Supply)
    {
        super._update(from, to, ids, values);
    }

    function supportsInterface(bytes4 interfaceId) public view override(ERC1155, ERC2981) returns (bool) {
        return super.supportsInterface(interfaceId);
    }

    function _authorizeUpgrade(address) internal override onlyOwner {}
}
