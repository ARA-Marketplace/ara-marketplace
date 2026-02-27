// SPDX-License-Identifier: LGPL-3.0
pragma solidity ^0.8.24;

import {IAraToken} from "./interfaces/IAraToken.sol";
import {Initializable} from "@openzeppelin/contracts/proxy/utils/Initializable.sol";
import {UUPSUpgradeable} from "@openzeppelin/contracts/proxy/utils/UUPSUpgradeable.sol";

/// @title AraStaking
/// @notice Manages ARA token staking for publishers and seeders.
///         Publishers must stake a minimum amount to list content.
///         Seeders allocate stake to specific content to earn rewards.
///         All stakers passively earn a share of marketplace purchase fees
///         proportional to their staked ARA (Synthetix-style accumulator).
contract AraStaking is Initializable, UUPSUpgradeable {
    IAraToken public araToken;

    /// @notice General staked balance per user (not allocated to any content)
    mapping(address => uint256) public stakedBalance;

    /// @notice Content-specific stake: user => contentId => amount
    mapping(address => mapping(bytes32 => uint256)) public contentStake;

    /// @notice Minimum ARA stake required to publish content
    uint256 public publisherMinStake;

    /// @notice Minimum ARA stake required to seed content and earn rewards
    uint256 public seederMinStake;

    address public owner;

    // === V2: Passive staker rewards (Synthetix accumulator) ===

    /// @notice Authorized marketplace contract that can deposit ETH rewards
    address public authorizedMarketplace;

    /// @notice Total ARA staked across all users (general + content-allocated)
    uint256 public totalStaked;

    /// @notice Per-user total stake (general + content-allocated)
    mapping(address => uint256) public totalUserStake;

    /// @notice Accumulated reward per staked token, scaled by 1e18
    uint256 public rewardPerTokenStored;

    /// @notice Last checkpoint of rewardPerToken for each user
    mapping(address => uint256) public userRewardPerTokenPaid;

    /// @notice Accrued but unclaimed ETH rewards per user
    mapping(address => uint256) public pendingRewards;

    /// @notice Total ETH deposited for staker rewards (lifetime)
    uint256 public totalStakerRewardsDeposited;

    /// @notice Total ETH claimed by stakers (lifetime)
    uint256 public totalStakerRewardsClaimed;

    event Staked(address indexed user, uint256 amount);
    event Unstaked(address indexed user, uint256 amount);
    event ContentStakeAdded(address indexed user, bytes32 indexed contentId, uint256 amount);
    event ContentStakeRemoved(address indexed user, bytes32 indexed contentId, uint256 amount);
    event PublisherMinStakeUpdated(uint256 oldValue, uint256 newValue);
    event SeederMinStakeUpdated(uint256 oldValue, uint256 newValue);
    event StakerRewardDeposited(uint256 amount);
    event StakerRewardClaimed(address indexed user, uint256 amount);
    event AuthorizedMarketplaceUpdated(address indexed oldMarketplace, address indexed newMarketplace);

    error InsufficientStakedBalance(uint256 requested, uint256 available);
    error InsufficientContentStake(uint256 requested, uint256 available);
    error TransferFailed();
    error OnlyOwner();
    error ZeroAmount();
    error OnlyAuthorizedMarketplace();
    error NoStakerRewardsToClaim();

    modifier onlyOwner() {
        if (msg.sender != owner) revert OnlyOwner();
        _;
    }

    /// @dev Checkpoint a user's accrued rewards before changing their stake.
    modifier updateReward(address account) {
        rewardPerTokenStored = rewardPerToken();
        if (account != address(0)) {
            pendingRewards[account] = earned(account);
            userRewardPerTokenPaid[account] = rewardPerTokenStored;
        }
        _;
    }

    /// @custom:oz-upgrades-unsafe-allow constructor
    constructor() {
        _disableInitializers();
    }

    /// @notice Initialize the contract (called once via proxy)
    function initialize(address _araToken, uint256 _publisherMinStake, uint256 _seederMinStake) external initializer {
        araToken = IAraToken(_araToken);
        publisherMinStake = _publisherMinStake;
        seederMinStake = _seederMinStake;
        owner = msg.sender;
    }

    /// @notice Stake ARA tokens into the general pool.
    ///         User must first call araToken.approve(this, amount).
    /// @param amount Amount of ARA to stake (in wei, 18 decimals)
    function stake(uint256 amount) external updateReward(msg.sender) {
        if (amount == 0) revert ZeroAmount();
        if (!araToken.transferFrom(msg.sender, address(this), amount)) revert TransferFailed();
        stakedBalance[msg.sender] += amount;
        totalStaked += amount;
        totalUserStake[msg.sender] += amount;
        emit Staked(msg.sender, amount);
    }

    /// @notice Unstake ARA tokens from the general pool back to the caller.
    ///         Cannot unstake tokens that are allocated to content.
    /// @param amount Amount of ARA to unstake
    function unstake(uint256 amount) external updateReward(msg.sender) {
        if (amount == 0) revert ZeroAmount();
        if (stakedBalance[msg.sender] < amount) {
            revert InsufficientStakedBalance(amount, stakedBalance[msg.sender]);
        }
        stakedBalance[msg.sender] -= amount;
        totalStaked -= amount;
        totalUserStake[msg.sender] -= amount;
        if (!araToken.transfer(msg.sender, amount)) revert TransferFailed();
        emit Unstaked(msg.sender, amount);
    }

    /// @notice Allocate staked ARA from the general pool to a specific content.
    ///         This signals intent to seed that content and earn rewards.
    /// @param contentId The content identifier
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

    /// @notice Transfer ownership
    function transferOwnership(address newOwner) external onlyOwner {
        owner = newOwner;
    }

    // ============================================================
    //                     PASSIVE STAKER REWARDS (V2)
    // ============================================================

    /// @notice V2 initializer for upgrade (can only be called once)
    function initializeV2(address _marketplace) external reinitializer(2) {
        authorizedMarketplace = _marketplace;
    }

    /// @notice Called by Marketplace during purchase to deposit ETH for staker rewards.
    function addReward() external payable {
        if (msg.sender != authorizedMarketplace) revert OnlyAuthorizedMarketplace();
        if (msg.value > 0 && totalStaked > 0) {
            rewardPerTokenStored += (msg.value * 1e18) / totalStaked;
        }
        totalStakerRewardsDeposited += msg.value;
        emit StakerRewardDeposited(msg.value);
    }

    /// @notice View: current reward per token
    function rewardPerToken() public view returns (uint256) {
        return rewardPerTokenStored;
    }

    /// @notice View: how much ETH a user has earned but not yet claimed
    function earned(address account) public view returns (uint256) {
        return (totalUserStake[account] * (rewardPerToken() - userRewardPerTokenPaid[account])) / 1e18
            + pendingRewards[account];
    }

    /// @notice Claim all accrued passive staker ETH rewards
    function claimStakingReward() external updateReward(msg.sender) {
        uint256 reward = pendingRewards[msg.sender];
        if (reward == 0) revert NoStakerRewardsToClaim();
        pendingRewards[msg.sender] = 0;
        totalStakerRewardsClaimed += reward;
        (bool sent,) = payable(msg.sender).call{value: reward}("");
        if (!sent) revert TransferFailed();
        emit StakerRewardClaimed(msg.sender, reward);
    }

    /// @notice Admin: set the authorized marketplace contract
    function setAuthorizedMarketplace(address _marketplace) external onlyOwner {
        emit AuthorizedMarketplaceUpdated(authorizedMarketplace, _marketplace);
        authorizedMarketplace = _marketplace;
    }

    /// @dev Sum all content stakes for a user (expensive, for view only).
    function _totalContentStake(address) internal pure returns (uint256) {
        // NOTE: Cannot iterate mappings in Solidity. For publisher eligibility,
        // we rely on general stakedBalance only. Content stake is separate and
        // used for per-content seeder eligibility.
        return 0;
    }

    function _authorizeUpgrade(address) internal override onlyOwner {}
}
