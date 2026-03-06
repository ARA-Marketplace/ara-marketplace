// SPDX-License-Identifier: LGPL-3.0
pragma solidity ^0.8.24;

import {DeployHelper} from "./helpers/DeployHelper.sol";
import {AraModeration} from "../src/AraModeration.sol";
import {AraContent} from "../src/AraContent.sol";

contract AraModerationTest is DeployHelper {
    address public creator = makeAddr("creator");
    address public flagger1;
    uint256 public flagger1Key;
    address public flagger2;
    uint256 public flagger2Key;
    address public flagger3;
    uint256 public flagger3Key;
    address public voter1 = makeAddr("voter1");
    address public voter2 = makeAddr("voter2");
    address public nobody = makeAddr("nobody");

    bytes32 public contentHash = keccak256("test-content");
    string public metadataURI = "ipfs://QmTest123";
    uint256 public price = 0.1 ether;
    uint256 public fileSize = 1_000_000;

    bytes32 public contentId;

    function setUp() public {
        _deployStack();

        // Create flaggers with deterministic keys
        (flagger1, flagger1Key) = makeAddrAndKey("flagger1");
        (flagger2, flagger2Key) = makeAddrAndKey("flagger2");
        (flagger3, flagger3Key) = makeAddrAndKey("flagger3");

        // Fund and stake creator
        token.mint(creator, 10_000 ether);
        vm.startPrank(creator);
        token.approve(address(staking), PUBLISHER_MIN);
        staking.stake(PUBLISHER_MIN);
        vm.stopPrank();

        // Publish content
        vm.prank(creator);
        contentId = contentToken.publishContent(contentHash, metadataURI, price, fileSize, 0, 1000);

        // Fund and stake flaggers (need flagMinStake = 1000 ARA)
        _stakeUser(flagger1, 1000 ether);
        _stakeUser(flagger2, 1000 ether);
        _stakeUser(flagger3, 1000 ether);

        // Fund and stake voters
        _stakeUser(voter1, 5000 ether);
        _stakeUser(voter2, 3000 ether);
    }

    function _stakeUser(address user, uint256 amount) internal {
        token.mint(user, amount);
        vm.startPrank(user);
        token.approve(address(staking), amount);
        staking.stake(amount);
        vm.stopPrank();
    }

    // ─── NSFW Tests ─────────────────────────────────────────────────────

    function test_CreatorSelfTagNsfw() public {
        vm.prank(creator);
        moderation.setNsfw(contentId, true);
        assertTrue(moderation.isNsfw(contentId));
    }

    function test_CreatorRemoveNsfwTag() public {
        vm.prank(creator);
        moderation.setNsfw(contentId, true);
        assertTrue(moderation.isNsfw(contentId));

        vm.prank(creator);
        moderation.setNsfw(contentId, false);
        assertFalse(moderation.isNsfw(contentId));
    }

    function test_RevertNonCreatorSetNsfw() public {
        vm.prank(flagger1);
        vm.expectRevert(AraModeration.NotContentCreator.selector);
        moderation.setNsfw(contentId, true);
    }

    function test_CommunityVoteNsfw() public {
        vm.prank(flagger1);
        moderation.voteNsfw(contentId);
        assertTrue(moderation.isNsfw(contentId));
    }

    function test_RevertVoteNsfwNoStake() public {
        vm.prank(nobody);
        vm.expectRevert(AraModeration.InsufficientStake.selector);
        moderation.voteNsfw(contentId);
    }

    // ─── Flagging Tests ─────────────────────────────────────────────────

    function test_FlagContent() public {
        vm.prank(flagger1);
        moderation.flagContent(contentId, AraModeration.FlagReason.Spam, false);

        // First flag — not yet active (threshold=3)
        assertEq(uint8(moderation.getProposalStatus(contentId)), uint8(AraModeration.ProposalStatus.None));
    }

    function test_ThreeFlagsActivateVoting() public {
        vm.prank(flagger1);
        moderation.flagContent(contentId, AraModeration.FlagReason.Spam, false);
        vm.prank(flagger2);
        moderation.flagContent(contentId, AraModeration.FlagReason.Spam, false);
        vm.prank(flagger3);
        moderation.flagContent(contentId, AraModeration.FlagReason.Spam, false);

        assertEq(uint8(moderation.getProposalStatus(contentId)), uint8(AraModeration.ProposalStatus.Active));
    }

    function test_RevertDoubleFlagging() public {
        vm.prank(flagger1);
        moderation.flagContent(contentId, AraModeration.FlagReason.Spam, false);

        vm.prank(flagger1);
        vm.expectRevert(AraModeration.AlreadyFlagged.selector);
        moderation.flagContent(contentId, AraModeration.FlagReason.Spam, false);
    }

    function test_RevertFlagWithoutStake() public {
        vm.prank(nobody);
        vm.expectRevert(AraModeration.InsufficientStake.selector);
        moderation.flagContent(contentId, AraModeration.FlagReason.Spam, false);
    }

    // ─── Emergency Flag Tests ───────────────────────────────────────────

    function test_EmergencyFlagActivatesImmediately() public {
        // Emergency requires 10000 ARA
        _stakeUser(flagger1, 9000 ether); // already has 1000, now 10000

        vm.prank(flagger1);
        moderation.flagContent(contentId, AraModeration.FlagReason.IllegalContent, true);

        // Immediately active (no threshold wait)
        assertEq(uint8(moderation.getProposalStatus(contentId)), uint8(AraModeration.ProposalStatus.Active));
    }

    function test_RevertEmergencyWithLowStake() public {
        vm.prank(flagger1); // has only 1000 ARA, needs 10000
        vm.expectRevert(AraModeration.InsufficientStake.selector);
        moderation.flagContent(contentId, AraModeration.FlagReason.IllegalContent, true);
    }

    // ─── Voting Tests ───────────────────────────────────────────────────

    function test_VoteOnProposal() public {
        _activateProposal();

        vm.prank(voter1);
        moderation.vote(contentId, true);

        // Check vote recorded
        assertTrue(moderation.hasVoted(contentId, voter1));
    }

    function test_RevertVoteTwice() public {
        _activateProposal();

        vm.prank(voter1);
        moderation.vote(contentId, true);

        vm.prank(voter1);
        vm.expectRevert(AraModeration.AlreadyVoted.selector);
        moderation.vote(contentId, true);
    }

    function test_RevertVoteAfterDeadline() public {
        _activateProposal();

        vm.warp(block.timestamp + 8 days); // past 7-day voting period

        vm.prank(voter1);
        vm.expectRevert(AraModeration.VotingEnded.selector);
        moderation.vote(contentId, true);
    }

    // ─── Resolution Tests ───────────────────────────────────────────────

    function test_ResolveUpheld() public {
        _activateProposal();

        // voter1 (5000 ARA) and voter2 (3000 ARA) both uphold
        vm.prank(voter1);
        moderation.vote(contentId, true);
        vm.prank(voter2);
        moderation.vote(contentId, true);

        vm.warp(block.timestamp + 8 days);
        moderation.resolveFlag(contentId);

        assertEq(uint8(moderation.getProposalStatus(contentId)), uint8(AraModeration.ProposalStatus.Upheld));
        // Content should be delisted
        assertFalse(contentToken.isActive(contentId));
    }

    function test_ResolveDismissed() public {
        _activateProposal();

        // voter1 (5000 ARA) and voter2 (3000 ARA) both dismiss
        vm.prank(voter1);
        moderation.vote(contentId, false);
        vm.prank(voter2);
        moderation.vote(contentId, false);

        vm.warp(block.timestamp + 8 days);
        moderation.resolveFlag(contentId);

        assertEq(uint8(moderation.getProposalStatus(contentId)), uint8(AraModeration.ProposalStatus.Dismissed));
        // Content should still be active
        assertTrue(contentToken.isActive(contentId));
    }

    function test_RevertResolveBeforeDeadline() public {
        _activateProposal();

        vm.expectRevert(AraModeration.VotingNotEnded.selector);
        moderation.resolveFlag(contentId);
    }

    function test_EmergencyPurge() public {
        // Emergency flag with high stake
        _stakeUser(flagger1, 9000 ether);
        vm.prank(flagger1);
        moderation.flagContent(contentId, AraModeration.FlagReason.IllegalContent, true);

        // Votes to uphold
        vm.prank(voter1);
        moderation.vote(contentId, true);
        vm.prank(voter2);
        moderation.vote(contentId, true);

        vm.warp(block.timestamp + 2 days); // past 1-day emergency period
        moderation.resolveFlag(contentId);

        assertEq(uint8(moderation.getProposalStatus(contentId)), uint8(AraModeration.ProposalStatus.Purged));
        assertTrue(moderation.isPurged(contentId));
        assertFalse(contentToken.isActive(contentId));
    }

    function test_PurgedContentCannotBeReflagged() public {
        // First, purge the content
        _stakeUser(flagger1, 9000 ether);
        vm.prank(flagger1);
        moderation.flagContent(contentId, AraModeration.FlagReason.IllegalContent, true);

        vm.prank(voter1);
        moderation.vote(contentId, true);
        vm.prank(voter2);
        moderation.vote(contentId, true);

        vm.warp(block.timestamp + 2 days);
        moderation.resolveFlag(contentId);

        // Try to flag again
        _stakeUser(flagger2, 9000 ether);
        vm.prank(flagger2);
        vm.expectRevert(AraModeration.ContentIsPurged.selector);
        moderation.flagContent(contentId, AraModeration.FlagReason.Spam, false);
    }

    // ─── Appeal Tests ───────────────────────────────────────────────────

    function test_CreatorAppeal() public {
        _activateProposal();

        (, , , , uint256 deadlineBefore, , , , , ) = moderation.getProposalDetail(contentId);

        vm.prank(creator);
        moderation.appeal(contentId);

        (, , , , uint256 deadlineAfter, , , , bool appealed, ) = moderation.getProposalDetail(contentId);

        assertTrue(appealed);
        assertEq(deadlineAfter, deadlineBefore + 3 days);
    }

    function test_RevertDoubleAppeal() public {
        _activateProposal();

        vm.prank(creator);
        moderation.appeal(contentId);

        vm.prank(creator);
        vm.expectRevert(AraModeration.AlreadyAppealed.selector);
        moderation.appeal(contentId);
    }

    function test_RevertNonCreatorAppeal() public {
        _activateProposal();

        vm.prank(flagger1);
        vm.expectRevert(AraModeration.NotContentCreator.selector);
        moderation.appeal(contentId);
    }

    // ─── Moderator Delist Tests ─────────────────────────────────────────

    function test_RevertUnauthorizedModeratorDelist() public {
        vm.prank(nobody);
        vm.expectRevert(AraContent.OnlyModerator.selector);
        contentToken.moderatorDelist(contentId);
    }

    // ─── Helpers ────────────────────────────────────────────────────────

    function _activateProposal() internal {
        vm.prank(flagger1);
        moderation.flagContent(contentId, AraModeration.FlagReason.Spam, false);
        vm.prank(flagger2);
        moderation.flagContent(contentId, AraModeration.FlagReason.Spam, false);
        vm.prank(flagger3);
        moderation.flagContent(contentId, AraModeration.FlagReason.Spam, false);
    }
}
