// SPDX-License-Identifier: BUSL-1.1
pragma solidity ^0.8.24;

import {AraContent} from "./AraContent.sol";
import {Initializable} from "@openzeppelin/contracts/proxy/utils/Initializable.sol";
import {UUPSUpgradeable} from "@openzeppelin/contracts/proxy/utils/UUPSUpgradeable.sol";

/// @title AraCollections
/// @notice On-chain collection registry. Creators group published content into
///         named collections with a banner image and description, visible to all
///         marketplace users after sync.
contract AraCollections is Initializable, UUPSUpgradeable {
    /// @custom:oz-upgrades-unsafe-allow constructor
    constructor() {
        _disableInitializers();
    }

    AraContent public contentToken;
    address public owner;

    struct Collection {
        address creator;
        string name;
        string description;
        string bannerUri; // iroh blob hash or IPFS URI
        uint256 createdAt;
        bool active;
    }

    uint256 public nextCollectionId; // starts at 1 after initialize

    /// @notice collectionId => collection metadata
    mapping(uint256 => Collection) public collections;

    /// @notice collectionId => ordered list of contentIds
    mapping(uint256 => bytes32[]) public collectionItems;

    /// @notice contentId => collectionId (0 means unassigned)
    mapping(bytes32 => uint256) public contentCollection;

    /// @notice creator address => list of collectionIds they own
    mapping(address => uint256[]) public creatorCollections;

    event CollectionCreated(uint256 indexed collectionId, address indexed creator, string name);
    event CollectionUpdated(uint256 indexed collectionId, string name, string description, string bannerUri);
    event CollectionDeleted(uint256 indexed collectionId);
    event ItemAddedToCollection(uint256 indexed collectionId, bytes32 indexed contentId);
    event ItemRemovedFromCollection(uint256 indexed collectionId, bytes32 indexed contentId);

    // === V2: Security hardening ===
    address public pendingOwner;

    modifier onlyOwner() {
        require(msg.sender == owner, "Not owner");
        _;
    }

    modifier onlyCollectionCreator(uint256 collectionId) {
        require(collections[collectionId].creator == msg.sender, "Not collection creator");
        require(collections[collectionId].active, "Collection deleted");
        _;
    }

    function initialize(address _contentToken) external initializer {
        contentToken = AraContent(_contentToken);
        owner = msg.sender;
        nextCollectionId = 1;
    }

    /// @notice Create a new collection
    /// @param name Collection name (max 100 chars)
    /// @param description Collection description
    /// @param bannerUri URI for banner image (iroh hash, IPFS, etc.)
    /// @return collectionId The ID of the newly created collection
    function createCollection(
        string calldata name,
        string calldata description,
        string calldata bannerUri
    ) external returns (uint256 collectionId) {
        require(bytes(name).length > 0 && bytes(name).length <= 100, "Invalid name length");

        collectionId = nextCollectionId++;
        collections[collectionId] = Collection({
            creator: msg.sender,
            name: name,
            description: description,
            bannerUri: bannerUri,
            createdAt: block.timestamp,
            active: true
        });
        creatorCollections[msg.sender].push(collectionId);

        emit CollectionCreated(collectionId, msg.sender, name);
    }

    /// @notice Update collection metadata (creator only)
    function updateCollection(
        uint256 collectionId,
        string calldata name,
        string calldata description,
        string calldata bannerUri
    ) external onlyCollectionCreator(collectionId) {
        require(bytes(name).length > 0 && bytes(name).length <= 100, "Invalid name length");

        Collection storage c = collections[collectionId];
        c.name = name;
        c.description = description;
        c.bannerUri = bannerUri;

        emit CollectionUpdated(collectionId, name, description, bannerUri);
    }

    /// @notice Soft-delete a collection (creator only). Items are unlinked.
    function deleteCollection(uint256 collectionId) external onlyCollectionCreator(collectionId) {
        // Unlink all items
        bytes32[] storage items = collectionItems[collectionId];
        for (uint256 i = 0; i < items.length; i++) {
            contentCollection[items[i]] = 0;
        }
        delete collectionItems[collectionId];
        collections[collectionId].active = false;

        emit CollectionDeleted(collectionId);
    }

    /// @notice Add a content item to a collection (creator only, must be content creator)
    function addItem(uint256 collectionId, bytes32 contentId)
        external
        onlyCollectionCreator(collectionId)
    {
        // Verify caller is the content creator
        require(contentToken.getCreator(contentId) == msg.sender, "Not content creator");
        require(contentCollection[contentId] == 0, "Already in a collection");

        collectionItems[collectionId].push(contentId);
        contentCollection[contentId] = collectionId;

        emit ItemAddedToCollection(collectionId, contentId);
    }

    /// @notice Remove a content item from a collection (creator only)
    function removeItem(uint256 collectionId, bytes32 contentId)
        external
        onlyCollectionCreator(collectionId)
    {
        require(contentCollection[contentId] == collectionId, "Not in this collection");

        // Remove from array (swap with last)
        bytes32[] storage items = collectionItems[collectionId];
        for (uint256 i = 0; i < items.length; i++) {
            if (items[i] == contentId) {
                items[i] = items[items.length - 1];
                items.pop();
                break;
            }
        }
        contentCollection[contentId] = 0;

        emit ItemRemovedFromCollection(collectionId, contentId);
    }

    // === View functions ===

    /// @notice Get all contentIds in a collection
    function getCollectionItems(uint256 collectionId) external view returns (bytes32[] memory) {
        return collectionItems[collectionId];
    }

    /// @notice Get all collectionIds for a creator
    function getCreatorCollections(address creator) external view returns (uint256[] memory) {
        return creatorCollections[creator];
    }

    /// @notice Get the number of items in a collection
    function getCollectionItemCount(uint256 collectionId) external view returns (uint256) {
        return collectionItems[collectionId].length;
    }

    function transferOwnership(address newOwner) external onlyOwner {
        require(newOwner != address(0), "Zero address");
        pendingOwner = newOwner;
    }

    function acceptOwnership() external {
        require(msg.sender == pendingOwner, "Not pending owner");
        owner = msg.sender;
        pendingOwner = address(0);
    }

    /// @dev Reserved storage for future upgrades
    uint256[50] private __gap;

    function _authorizeUpgrade(address) internal override onlyOwner {}
}
