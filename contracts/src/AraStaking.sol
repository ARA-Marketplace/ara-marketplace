// SPDX-License-Identifier: LGPL-3.0
pragma solidity ^0.8.24;

import {IAraToken} from "./interfaces/IAraToken.sol";

/// @title AraStaking
/// @notice Manages ARA token staking for publishers and seeders.
///         Publishers must stake a minimum amount to list content.
///         Seeders allocate stake to specific content to earn rewards.
contract AraStaking {
    IAraToken public immutable araToken;

    /// @notice General staked balance per user (not allocated to any content)
    mapping(address => uint256) public stakedBalance;

    /// @notice Content-specific stake: user => contentId => amount
    mapping(address => mapping(bytes32 => uint256)) public contentStake;

    /// @notice Minimum ARA stake required to publish content
    uint256 public publisherMinStake;

    /// @notice Minimum ARA stake required to seed content and earn rewards
    uint256 public seederMinStake;

    address public owner;

    event Staked(address indexed user, uint256 amount);
    event Unstaked(address indexed user, uint256 amount);
    event ContentStakeAdded(address indexed user, bytes32 indexed contentId, uint256 amount);
    event ContentStakeRemoved(address indexed user, bytes32 indexed contentId, uint256 amount);
    event PublisherMinStakeUpdated(uint256 oldValue, uint256 newValue);
    event SeederMinStakeUpdated(uint256 oldValue, uint256 newValue);

    error InsufficientStakedBalance(uint256 requested, uint256 available);
    error InsufficientContentStake(uint256 requested, uint256 available);
    error TransferFailed();
    error OnlyOwner();
    error ZeroAmount();

    modifier onlyOwner() {
        if (msg.sender != owner) revert OnlyOwner();
        _;
    }

    constructor(address _araToken, uint256 _publisherMinStake, uint256 _seederMinStake) {
        araToken = IAraToken(_araToken);
        publisherMinStake = _publisherMinStake;
        seederMinStake = _seederMinStake;
        owner = msg.sender;
    }

    /// @notice Stake ARA tokens into the general pool.
    ///         User must first call araToken.approve(this, amount).
    /// @param amount Amount of ARA to stake (in wei, 18 decimals)
    function stake(uint256 amount) external {
        if (amount == 0) revert ZeroAmount();
        if (!araToken.transferFrom(msg.sender, address(this), amount)) revert TransferFailed();
        stakedBalance[msg.sender] += amount;
        emit Staked(msg.sender, amount);
    }

    /// @notice Unstake ARA tokens from the general pool back to the caller.
    ///         Cannot unstake tokens that are allocated to content.
    /// @param amount Amount of ARA to unstake
    function unstake(uint256 amount) external {
        if (amount == 0) revert ZeroAmount();
        if (stakedBalance[msg.sender] < amount) {
            revert InsufficientStakedBalance(amount, stakedBalance[msg.sender]);
        }
        stakedBalance[msg.sender] -= amount;
        if (!araToken.transfer(msg.sender, amount)) revert TransferFailed();
        emit Unstaked(msg.sender, amount);
    }

    /// @notice Allocate staked ARA from the general pool to a specific content.
    ///         This signals intent to seed that content and earn rewards.
    /// @param contentId The content identifier (from ContentRegistry)
    /// @param amount Amount to allocate from general stake to this content
    function stakeForContent(bytes32 contentId, uint256 amount) external {
        if (amount == 0) revert ZeroAmount();
        if (stakedBalance[msg.sender] < amount) {
            revert InsufficientStakedBalance(amount, stakedBalance[msg.sender]);
        }
        stakedBalance[msg.sender] -= amount;
        contentStake[msg.sender][contentId] += amount;
        emit ContentStakeAdded(msg.sender, contentId, amount);
    }

    /// @notice Remove content-specific stake back to the general pool.
    /// @param contentId The content identifier
    /// @param amount Amount to move back to general stake
    function unstakeFromContent(bytes32 contentId, uint256 amount) external {
        if (amount == 0) revert ZeroAmount();
        if (contentStake[msg.sender][contentId] < amount) {
            revert InsufficientContentStake(amount, contentStake[msg.sender][contentId]);
        }
        contentStake[msg.sender][contentId] -= amount;
        stakedBalance[msg.sender] += amount;
        emit ContentStakeRemoved(msg.sender, contentId, amount);
    }

    /// @notice Check if a user meets the minimum stake to publish content
    function isEligiblePublisher(address user) external view returns (bool) {
        return (stakedBalance[user] + _totalContentStake(user)) >= publisherMinStake;
    }

    /// @notice Check if a user meets the minimum stake for a specific content
    function isEligibleSeeder(address user, bytes32 contentId) external view returns (bool) {
        return contentStake[user][contentId] >= seederMinStake;
    }

    /// @notice Get a user's content-specific stake
    function getContentStake(address user, bytes32 contentId) external view returns (uint256) {
        return contentStake[user][contentId];
    }

    /// @notice Update the minimum stake for publishers
    function setPublisherMinStake(uint256 newMinStake) external onlyOwner {
        emit PublisherMinStakeUpdated(publisherMinStake, newMinStake);
        publisherMinStake = newMinStake;
    }

    /// @notice Update the minimum stake for seeders
    function setSeederMinStake(uint256 newMinStake) external onlyOwner {
        emit SeederMinStakeUpdated(seederMinStake, newMinStake);
        seederMinStake = newMinStake;
    }

    /// @dev Sum all content stakes for a user (expensive, for view only).
    ///      In practice, isEligiblePublisher checks general + content stake total.
    ///      This is a simplified version — a real implementation would need to
    ///      track total content stake separately for gas efficiency.
    function _totalContentStake(address) internal pure returns (uint256) {
        // NOTE: Cannot iterate mappings in Solidity. For publisher eligibility,
        // we rely on general stakedBalance only. Content stake is separate and
        // used for per-content seeder eligibility.
        return 0;
    }
}
