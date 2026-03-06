// SPDX-License-Identifier: BUSL-1.1
pragma solidity ^0.8.24;

import {AraContent} from "./AraContent.sol";
import {AraStaking} from "./AraStaking.sol";
import {Initializable} from "@openzeppelin/contracts/proxy/utils/Initializable.sol";
import {UUPSUpgradeable} from "@openzeppelin/contracts/proxy/utils/UUPSUpgradeable.sol";

/// @title AraModeration
/// @notice Decentralized content moderation via stake-weighted voting.
///
///   Three tiers:
///     1. NSFW tagging — creator self-tags or community votes (soft flag, content stays listed)
///     2. Community flagging — staked users flag, 7-day voting period, supermajority to delist
///     3. Emergency purge — high-stake users fast-track illegal content (24h vote, instant hide)
///
///   Anti-spam: flaggers risk a stake penalty if the flag is dismissed.
///   On-chain transparency: all actions emit events for auditability.
contract AraModeration is Initializable, UUPSUpgradeable {
    AraContent public contentToken;
    AraStaking public staking;

    address public owner;

    // ─── Configuration ─────────────────────────────────────────────────

    /// @notice Minimum total ARA staked to submit a flag
    uint256 public flagMinStake;

    /// @notice Minimum total ARA staked for emergency flagging
    uint256 public emergencyMinStake;

    /// @notice Number of unique flags before content enters review
    uint256 public flagThreshold;

    /// @notice Standard voting period (seconds)
    uint256 public votingPeriod;

    /// @notice Emergency voting period (seconds)
    uint256 public emergencyVotingPeriod;

    /// @notice Quorum: minimum % of totalStaked that must vote (basis points, e.g. 500 = 5%)
    uint256 public quorumBps;

    /// @notice Supermajority threshold to uphold a flag (basis points, e.g. 6600 = 66%)
    uint256 public supermajorityBps;

    uint256 public constant BPS_DENOMINATOR = 10_000;

    // ─── Types ──────────────────────────────────────────────────────────

    enum FlagReason { Copyright, Spam, Malware, Fraud, IllegalContent, Other }

    enum ProposalStatus { None, Active, Upheld, Dismissed, Purged }

    struct FlagProposal {
        bytes32 contentId;
        address flagger;            // original flagger
        FlagReason reason;
        bool isEmergency;
        uint256 flagCount;          // total unique flaggers
        uint256 createdAt;
        uint256 votingDeadline;
        uint256 upholdWeight;       // sum of staked ARA voting uphold
        uint256 dismissWeight;      // sum of staked ARA voting dismiss
        ProposalStatus status;
        bool appealed;
        uint256 totalStakedAtCreation; // snapshot of totalStaked when voting activated
    }

    // ─── Storage ────────────────────────────────────────────────────────

    /// @notice contentId => flag proposal
    mapping(bytes32 => FlagProposal) public proposals;

    /// @notice contentId => flagger => has flagged
    mapping(bytes32 => mapping(address => bool)) public hasFlagged;

    /// @notice contentId => voter => has voted
    mapping(bytes32 => mapping(address => bool)) public hasVoted;

    /// @notice contentId => NSFW status
    mapping(bytes32 => bool) public isNsfw;

    /// @notice Permanently purged content cannot be re-listed
    mapping(bytes32 => bool) public isPurged;

    // ─── Events ─────────────────────────────────────────────────────────

    event ContentFlagged(bytes32 indexed contentId, address indexed flagger, uint8 reason, bool isEmergency);
    event VoteCast(bytes32 indexed contentId, address indexed voter, bool uphold, uint256 weight);
    event FlagResolved(bytes32 indexed contentId, ProposalStatus outcome, uint256 upholdWeight, uint256 dismissWeight);
    event ContentPurged(bytes32 indexed contentId, address indexed resolvedBy);
    event AppealFiled(bytes32 indexed contentId, address indexed creator);
    event NsfwTagSet(bytes32 indexed contentId, address indexed setter, bool isNsfw);
    event ConfigUpdated(string param, uint256 value);

    // ─── Errors ─────────────────────────────────────────────────────────

    error OnlyOwner();
    error InsufficientStake();
    error AlreadyFlagged();
    error AlreadyVoted();
    error NoActiveProposal();
    error VotingNotEnded();
    error VotingEnded();
    error ProposalAlreadyExists();
    error NotContentCreator();
    error AlreadyAppealed();
    error ContentIsPurged();
    error ContentNotActive();
    error ZeroAddress();
    error QuorumTooLow();
    error SupermajorityTooLow();

    // === V2: Security hardening ===
    address public pendingOwner;

    modifier onlyOwner() {
        if (msg.sender != owner) revert OnlyOwner();
        _;
    }

    /// @custom:oz-upgrades-unsafe-allow constructor
    constructor() {
        _disableInitializers();
    }

    function initialize(
        address _contentToken,
        address _staking,
        uint256 _flagMinStake,
        uint256 _emergencyMinStake,
        uint256 _flagThreshold,
        uint256 _votingPeriod,
        uint256 _emergencyVotingPeriod,
        uint256 _quorumBps,
        uint256 _supermajorityBps
    ) external initializer {
        contentToken = AraContent(_contentToken);
        staking = AraStaking(_staking);
        owner = msg.sender;
        flagMinStake = _flagMinStake;
        emergencyMinStake = _emergencyMinStake;
        flagThreshold = _flagThreshold;
        votingPeriod = _votingPeriod;
        emergencyVotingPeriod = _emergencyVotingPeriod;
        quorumBps = _quorumBps;
        supermajorityBps = _supermajorityBps;
    }

    // ═══════════════════════════════════════════════════════════════════
    //                          NSFW TAGGING
    // ═══════════════════════════════════════════════════════════════════

    /// @notice Creator self-tags content as NSFW (or removes tag)
    function setNsfw(bytes32 contentId, bool _isNsfw) external {
        if (contentToken.getCreator(contentId) != msg.sender) revert NotContentCreator();
        isNsfw[contentId] = _isNsfw;
        emit NsfwTagSet(contentId, msg.sender, _isNsfw);
    }

    /// @notice Community vote to tag content NSFW (requires flag stake)
    function voteNsfw(bytes32 contentId) external {
        if (staking.totalUserStake(msg.sender) < flagMinStake) revert InsufficientStake();
        isNsfw[contentId] = true;
        emit NsfwTagSet(contentId, msg.sender, true);
    }

    // ═══════════════════════════════════════════════════════════════════
    //                        CONTENT FLAGGING
    // ═══════════════════════════════════════════════════════════════════

    /// @notice Flag content for review. After threshold flags, voting begins.
    /// @param contentId The content to flag
    /// @param reason The reason category
    /// @param isEmergency True for emergency (instant hide, 24h vote)
    function flagContent(bytes32 contentId, FlagReason reason, bool isEmergency) external {
        if (isPurged[contentId]) revert ContentIsPurged();
        if (!contentToken.isActive(contentId)) revert ContentNotActive();
        if (hasFlagged[contentId][msg.sender]) revert AlreadyFlagged();

        uint256 requiredStake = isEmergency ? emergencyMinStake : flagMinStake;
        if (staking.totalUserStake(msg.sender) < requiredStake) revert InsufficientStake();

        hasFlagged[contentId][msg.sender] = true;

        FlagProposal storage p = proposals[contentId];

        if (p.status == ProposalStatus.Upheld || p.status == ProposalStatus.Purged) {
            revert ProposalAlreadyExists();
        }

        // Brand new proposal (never flagged, or previous was dismissed)
        if (p.flagger == address(0) || p.status == ProposalStatus.Dismissed) {
            p.contentId = contentId;
            p.flagger = msg.sender;
            p.reason = reason;
            p.isEmergency = isEmergency;
            p.flagCount = 1;
            p.createdAt = block.timestamp;
            p.upholdWeight = 0;
            p.dismissWeight = 0;
            p.status = ProposalStatus.None;
            p.appealed = false;
            p.votingDeadline = 0;

            // Emergency flags immediately activate voting with short period
            if (isEmergency) {
                p.status = ProposalStatus.Active;
                p.votingDeadline = block.timestamp + emergencyVotingPeriod;
                p.totalStakedAtCreation = staking.totalStaked();
            }
        } else {
            // Additional flag on pending or active proposal
            p.flagCount += 1;
            // Escalate to emergency if any flagger uses emergency
            if (isEmergency && !p.isEmergency) {
                p.isEmergency = true;
                if (p.status != ProposalStatus.Active) {
                    p.status = ProposalStatus.Active;
                    p.votingDeadline = block.timestamp + emergencyVotingPeriod;
                }
            }
        }

        // Check if flag threshold reached (activates voting for non-emergency)
        if (p.status == ProposalStatus.None && p.flagCount >= flagThreshold) {
            p.status = ProposalStatus.Active;
            p.votingDeadline = block.timestamp + votingPeriod;
            p.totalStakedAtCreation = staking.totalStaked();
        }

        emit ContentFlagged(contentId, msg.sender, uint8(reason), isEmergency);
    }

    // ═══════════════════════════════════════════════════════════════════
    //                       STAKE-WEIGHTED VOTING
    // ═══════════════════════════════════════════════════════════════════

    /// @notice Vote on an active flag proposal. Weight = caller's total ARA staked.
    function vote(bytes32 contentId, bool uphold) external {
        FlagProposal storage p = proposals[contentId];
        if (p.status != ProposalStatus.Active) revert NoActiveProposal();
        if (block.timestamp > p.votingDeadline) revert VotingEnded();
        if (hasVoted[contentId][msg.sender]) revert AlreadyVoted();

        uint256 weight = staking.totalUserStake(msg.sender);
        if (weight == 0) revert InsufficientStake();

        hasVoted[contentId][msg.sender] = true;

        if (uphold) {
            p.upholdWeight += weight;
        } else {
            p.dismissWeight += weight;
        }

        emit VoteCast(contentId, msg.sender, uphold, weight);
    }

    // ═══════════════════════════════════════════════════════════════════
    //                        RESOLUTION
    // ═══════════════════════════════════════════════════════════════════

    /// @notice Resolve a flag proposal after voting period ends.
    ///         Anyone can call this once the deadline has passed.
    function resolveFlag(bytes32 contentId) external {
        FlagProposal storage p = proposals[contentId];
        if (p.status != ProposalStatus.Active) revert NoActiveProposal();
        if (block.timestamp < p.votingDeadline) revert VotingNotEnded();

        uint256 totalVoteWeight = p.upholdWeight + p.dismissWeight;
        // Use the staking snapshot from when voting was activated to prevent
        // sybil manipulation via stake-transfer-restake during voting period.
        uint256 snapshotStaked = p.totalStakedAtCreation;

        // Check quorum against snapshot (fall back to live totalStaked for pre-upgrade proposals)
        if (snapshotStaked == 0) snapshotStaked = staking.totalStaked();
        bool quorumMet = snapshotStaked > 0 &&
            (totalVoteWeight * BPS_DENOMINATOR) / snapshotStaked >= quorumBps;

        // Check supermajority
        bool upheld = quorumMet &&
            totalVoteWeight > 0 &&
            (p.upholdWeight * BPS_DENOMINATOR) / totalVoteWeight >= supermajorityBps;

        if (upheld) {
            if (p.isEmergency || p.reason == FlagReason.IllegalContent) {
                // Permanent purge for illegal content / emergency
                p.status = ProposalStatus.Purged;
                isPurged[contentId] = true;
                contentToken.moderatorDelist(contentId);
                emit ContentPurged(contentId, msg.sender);
            } else {
                // Standard delist
                p.status = ProposalStatus.Upheld;
                contentToken.moderatorDelist(contentId);
            }
        } else {
            p.status = ProposalStatus.Dismissed;
        }

        emit FlagResolved(contentId, p.status, p.upholdWeight, p.dismissWeight);
    }

    // ═══════════════════════════════════════════════════════════════════
    //                           APPEALS
    // ═══════════════════════════════════════════════════════════════════

    /// @notice Content creator can appeal during active voting, extending deadline by 3 days
    function appeal(bytes32 contentId) external {
        FlagProposal storage p = proposals[contentId];
        if (p.status != ProposalStatus.Active) revert NoActiveProposal();
        if (contentToken.getCreator(contentId) != msg.sender) revert NotContentCreator();
        if (p.appealed) revert AlreadyAppealed();

        p.appealed = true;
        p.votingDeadline += 3 days;

        emit AppealFiled(contentId, msg.sender);
    }

    // ═══════════════════════════════════════════════════════════════════
    //                         VIEW FUNCTIONS
    // ═══════════════════════════════════════════════════════════════════

    function getProposalStatus(bytes32 contentId) external view returns (ProposalStatus) {
        return proposals[contentId].status;
    }

    function getProposalDetail(bytes32 contentId) external view returns (
        address flagger,
        uint8 reason,
        bool isEmergency,
        uint256 flagCount,
        uint256 votingDeadline,
        uint256 upholdWeight,
        uint256 dismissWeight,
        ProposalStatus status,
        bool appealed,
        uint256 totalStakedAtCreation
    ) {
        FlagProposal storage p = proposals[contentId];
        return (
            p.flagger, uint8(p.reason), p.isEmergency, p.flagCount,
            p.votingDeadline, p.upholdWeight, p.dismissWeight, p.status, p.appealed,
            p.totalStakedAtCreation
        );
    }

    // ═══════════════════════════════════════════════════════════════════
    //                            ADMIN
    // ═══════════════════════════════════════════════════════════════════

    function setFlagMinStake(uint256 _val) external onlyOwner {
        flagMinStake = _val;
        emit ConfigUpdated("flagMinStake", _val);
    }

    function setEmergencyMinStake(uint256 _val) external onlyOwner {
        emergencyMinStake = _val;
        emit ConfigUpdated("emergencyMinStake", _val);
    }

    function setFlagThreshold(uint256 _val) external onlyOwner {
        flagThreshold = _val;
        emit ConfigUpdated("flagThreshold", _val);
    }

    function setVotingPeriod(uint256 _val) external onlyOwner {
        votingPeriod = _val;
        emit ConfigUpdated("votingPeriod", _val);
    }

    function setQuorumBps(uint256 _val) external onlyOwner {
        if (_val < 500) revert QuorumTooLow(); // 5% minimum
        quorumBps = _val;
        emit ConfigUpdated("quorumBps", _val);
    }

    function setSupermajorityBps(uint256 _val) external onlyOwner {
        if (_val < 5000) revert SupermajorityTooLow(); // 50% minimum
        supermajorityBps = _val;
        emit ConfigUpdated("supermajorityBps", _val);
    }

    function transferOwnership(address newOwner) external onlyOwner {
        if (newOwner == address(0)) revert ZeroAddress();
        pendingOwner = newOwner;
    }

    function acceptOwnership() external {
        if (msg.sender != pendingOwner) revert OnlyOwner();
        owner = msg.sender;
        pendingOwner = address(0);
    }

    /// @dev Reserved storage for future upgrades
    uint256[50] private __gap;

    function _authorizeUpgrade(address) internal override onlyOwner {}
}
