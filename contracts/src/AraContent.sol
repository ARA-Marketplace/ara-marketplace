// SPDX-License-Identifier: BUSL-1.1
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

    /// @notice Authorized moderator (AraModeration contract)
    address public moderator;

    /// @notice Contract owner (can upgrade, set minter)
    address public owner;

    // === V3: Multi-token payment support ===

    /// @notice contentId => payment token address (address(0) = ETH)
    mapping(bytes32 => address) public paymentToken;

    // === V4: Collaborator revenue splits ===

    struct Collaborator {
        address wallet;
        uint256 shareBps; // basis points out of 10000
    }

    uint256 public constant MAX_COLLABORATORS = 5;

    /// @notice contentId => collaborator list (immutable after publish)
    mapping(bytes32 => Collaborator[]) internal _contentCollaborators;

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
    error OnlyModerator();
    error OnlyOwner();
    error TooManyCollaborators();
    error InvalidCollaboratorShares();
    error ZeroCollaboratorAddress();
    error DuplicateCollaborator();
    error PublisherNotInCollaborators();
    error ZeroAddress();
    error RoyaltyTooHigh();

    // === V5: Security hardening ===
    address public pendingOwner;

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
        if (royaltyBps > 5000) revert RoyaltyTooHigh(); // 50% max

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

        if (royaltyBps > 0) {
            _setTokenRoyalty(uint256(contentId), msg.sender, royaltyBps);
        }

        // paymentToken defaults to address(0) = ETH
        emit ContentPublished(contentId, msg.sender, contentHash, metadataURI, priceWei, fileSize, maxSupply);
    }

    /// @notice Register new content priced in an ERC-20 token.
    /// @param _paymentToken The ERC-20 token address for pricing (address(0) = ETH)
    function publishContentWithToken(
        bytes32 contentHash,
        string calldata metadataURI,
        uint256 price,
        uint256 fileSize,
        uint256 maxSupply,
        uint96 royaltyBps,
        address _paymentToken
    ) external returns (bytes32 contentId) {
        if (!staking.isEligiblePublisher(msg.sender)) revert InsufficientStake();
        if (price == 0) revert ZeroPrice();
        if (fileSize == 0) revert ZeroFileSize();
        if (royaltyBps > 5000) revert RoyaltyTooHigh();

        uint256 nonce = publisherNonce[msg.sender];
        contentId = keccak256(abi.encodePacked(contentHash, msg.sender, nonce));
        publisherNonce[msg.sender] = nonce + 1;
        if (contents[contentId].creator != address(0)) revert ContentAlreadyExists(contentId);

        contents[contentId] = ContentMeta({
            creator: msg.sender,
            contentHash: contentHash,
            metadataURI: metadataURI,
            priceWei: price,
            fileSize: fileSize,
            maxSupply: maxSupply,
            createdAt: block.timestamp,
            active: true
        });

        fileSizes[contentId] = fileSize;
        paymentToken[contentId] = _paymentToken;

        creatorContents[msg.sender].push(contentId);
        allContentIds.push(contentId);

        if (royaltyBps > 0) {
            _setTokenRoyalty(uint256(contentId), msg.sender, royaltyBps);
        }

        emit ContentPublished(contentId, msg.sender, contentHash, metadataURI, price, fileSize, maxSupply);
    }

    /// @notice Publish content with collaborator revenue splits.
    /// @param collaborators Array of collaborators and their share in basis points (must sum to 10000).
    ///        The publisher (msg.sender) must be included. Max 5 collaborators.
    function publishContentWithCollaborators(
        bytes32 contentHash,
        string calldata metadataURI,
        uint256 priceWei,
        uint256 fileSize,
        uint256 maxSupply,
        uint96 royaltyBps,
        Collaborator[] calldata collaborators
    ) external returns (bytes32 contentId) {
        if (!staking.isEligiblePublisher(msg.sender)) revert InsufficientStake();
        if (priceWei == 0) revert ZeroPrice();
        if (fileSize == 0) revert ZeroFileSize();
        if (royaltyBps > 5000) revert RoyaltyTooHigh();
        if (collaborators.length == 0 || collaborators.length > MAX_COLLABORATORS) revert TooManyCollaborators();

        // Validate collaborators
        uint256 totalShares;
        bool publisherIncluded;
        for (uint256 i = 0; i < collaborators.length; i++) {
            if (collaborators[i].wallet == address(0)) revert ZeroCollaboratorAddress();
            if (collaborators[i].shareBps == 0) revert InvalidCollaboratorShares();
            totalShares += collaborators[i].shareBps;
            if (collaborators[i].wallet == msg.sender) publisherIncluded = true;
            // Check for duplicates
            for (uint256 j = 0; j < i; j++) {
                if (collaborators[j].wallet == collaborators[i].wallet) revert DuplicateCollaborator();
            }
        }
        if (totalShares != 10000) revert InvalidCollaboratorShares();
        if (!publisherIncluded) revert PublisherNotInCollaborators();

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

        // Store collaborators (immutable)
        for (uint256 i = 0; i < collaborators.length; i++) {
            _contentCollaborators[contentId].push(collaborators[i]);
        }

        creatorContents[msg.sender].push(contentId);
        allContentIds.push(contentId);

        // Set ERC-2981 royalty receiver to the first collaborator.
        // The Marketplace contract handles actual distribution among all collaborators.
        if (royaltyBps > 0) {
            _setTokenRoyalty(uint256(contentId), collaborators[0].wallet, royaltyBps);
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
        if (newPriceWei == 0) revert ZeroPrice();

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

    /// @notice Delist content via moderation contract (governance decision)
    function moderatorDelist(bytes32 contentId) external {
        if (msg.sender != moderator) revert OnlyModerator();
        contents[contentId].active = false;
        emit ContentDelisted(contentId);
    }

    /// @notice Set the authorized moderator contract
    function setModerator(address _moderator) external onlyOwner {
        moderator = _moderator;
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

    /// @notice Get the payment token for content (address(0) = ETH)
    function getPaymentToken(bytes32 contentId) external view returns (address) {
        return paymentToken[contentId];
    }

    /// @notice Check if content has collaborator splits
    function hasCollaborators(bytes32 contentId) external view returns (bool) {
        return _contentCollaborators[contentId].length > 0;
    }

    /// @notice Get collaborator list for content
    function getCollaborators(bytes32 contentId) external view returns (Collaborator[] memory) {
        return _contentCollaborators[contentId];
    }

    /// @notice Get collaborator count for content
    function getCollaboratorCount(bytes32 contentId) external view returns (uint256) {
        return _contentCollaborators[contentId].length;
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

    /// @notice Propose a new owner (two-step transfer)
    function transferOwnership(address newOwner) external onlyOwner {
        if (newOwner == address(0)) revert ZeroAddress();
        pendingOwner = newOwner;
    }

    /// @notice Accept ownership (must be called by the pending owner)
    function acceptOwnership() external {
        if (msg.sender != pendingOwner) revert OnlyOwner();
        owner = msg.sender;
        pendingOwner = address(0);
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

    /// @dev Reserved storage for future upgrades
    uint256[50] private __gap;

    function _authorizeUpgrade(address) internal override onlyOwner {}
}
