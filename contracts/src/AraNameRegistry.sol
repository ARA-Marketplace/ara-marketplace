// SPDX-License-Identifier: LGPL-3.0
pragma solidity ^0.8.24;

import {Initializable} from "@openzeppelin/contracts/proxy/utils/Initializable.sol";
import {UUPSUpgradeable} from "@openzeppelin/contracts/proxy/utils/UUPSUpgradeable.sol";

/// @title AraNameRegistry
/// @notice On-chain display name registry. Each address can register one unique
///         display name (max 32 chars, alphanumeric + hyphens + underscores).
///         Names are globally visible after sync, replacing raw wallet addresses.
contract AraNameRegistry is Initializable, UUPSUpgradeable {
    address public owner;

    /// @notice address => display name
    mapping(address => string) public addressToName;

    /// @notice keccak256(lowercased name) => address (uniqueness enforcement)
    mapping(bytes32 => address) public nameHashToAddress;

    event NameRegistered(address indexed user, string name);
    event NameRemoved(address indexed user, string oldName);

    modifier onlyOwner() {
        require(msg.sender == owner, "Not owner");
        _;
    }

    function initialize() external initializer {
        owner = msg.sender;
    }

    /// @notice Register or update your display name
    /// @param name Display name (1-32 chars, alphanumeric + hyphens + underscores)
    function registerName(string calldata name) external {
        bytes memory nameBytes = bytes(name);
        require(nameBytes.length >= 1 && nameBytes.length <= 32, "Name must be 1-32 chars");
        require(_isValidName(nameBytes), "Invalid chars (use a-z, 0-9, -, _)");

        bytes32 newHash = _nameHash(nameBytes);
        address existing = nameHashToAddress[newHash];
        require(existing == address(0) || existing == msg.sender, "Name already taken");

        // Clear old name if exists
        bytes memory oldName = bytes(addressToName[msg.sender]);
        if (oldName.length > 0) {
            bytes32 oldHash = _nameHash(oldName);
            delete nameHashToAddress[oldHash];
        }

        addressToName[msg.sender] = name;
        nameHashToAddress[newHash] = msg.sender;

        emit NameRegistered(msg.sender, name);
    }

    /// @notice Remove your display name
    function removeName() external {
        bytes memory oldName = bytes(addressToName[msg.sender]);
        require(oldName.length > 0, "No name registered");

        bytes32 oldHash = _nameHash(oldName);
        delete nameHashToAddress[oldHash];
        delete addressToName[msg.sender];

        emit NameRemoved(msg.sender, string(oldName));
    }

    /// @notice Lookup a display name for an address
    function getName(address user) external view returns (string memory) {
        return addressToName[user];
    }

    /// @notice Batch lookup display names
    function getNames(address[] calldata users) external view returns (string[] memory names) {
        names = new string[](users.length);
        for (uint256 i = 0; i < users.length; i++) {
            names[i] = addressToName[users[i]];
        }
    }

    /// @notice Reverse lookup: find the address that owns a name
    function getAddress(string calldata name) external view returns (address) {
        bytes32 h = _nameHash(bytes(name));
        return nameHashToAddress[h];
    }

    /// @dev Lowercase and hash the name for uniqueness
    function _nameHash(bytes memory name) internal pure returns (bytes32) {
        bytes memory lower = new bytes(name.length);
        for (uint256 i = 0; i < name.length; i++) {
            bytes1 c = name[i];
            if (c >= 0x41 && c <= 0x5A) {
                lower[i] = bytes1(uint8(c) + 32); // A-Z => a-z
            } else {
                lower[i] = c;
            }
        }
        return keccak256(lower);
    }

    /// @dev Validate name: only a-z, A-Z, 0-9, hyphen, underscore
    function _isValidName(bytes memory name) internal pure returns (bool) {
        for (uint256 i = 0; i < name.length; i++) {
            bytes1 c = name[i];
            bool ok = (c >= 0x30 && c <= 0x39) // 0-9
                || (c >= 0x41 && c <= 0x5A)     // A-Z
                || (c >= 0x61 && c <= 0x7A)     // a-z
                || c == 0x2D                     // -
                || c == 0x5F;                    // _
            if (!ok) return false;
        }
        return true;
    }

    function _authorizeUpgrade(address) internal override onlyOwner {}
}
