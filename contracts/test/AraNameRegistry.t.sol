// SPDX-License-Identifier: LGPL-3.0
pragma solidity ^0.8.24;

import {DeployHelper} from "./helpers/DeployHelper.sol";
import {AraNameRegistry} from "../src/AraNameRegistry.sol";

contract AraNameRegistryTest is DeployHelper {
    address public alice = makeAddr("alice");
    address public bob = makeAddr("bob");
    address public carol = makeAddr("carol");

    function setUp() public {
        _deployStack();
    }

    function test_RegisterName() public {
        vm.prank(alice);
        nameRegistry.registerName("alice_rocks");

        assertEq(nameRegistry.getName(alice), "alice_rocks");
        assertEq(nameRegistry.getAddress("alice_rocks"), alice);
    }

    function test_RegisterNameCaseInsensitiveUniqueness() public {
        vm.prank(alice);
        nameRegistry.registerName("CoolName");

        // Bob tries the same name in different case
        vm.prank(bob);
        vm.expectRevert("Name already taken");
        nameRegistry.registerName("coolname");
    }

    function test_UpdateName() public {
        vm.startPrank(alice);
        nameRegistry.registerName("old-name");
        nameRegistry.registerName("new-name");
        vm.stopPrank();

        assertEq(nameRegistry.getName(alice), "new-name");
        assertEq(nameRegistry.getAddress("new-name"), alice);
        // Old name should be freed
        assertEq(nameRegistry.getAddress("old-name"), address(0));
    }

    function test_RemoveName() public {
        vm.startPrank(alice);
        nameRegistry.registerName("alice");
        nameRegistry.removeName();
        vm.stopPrank();

        assertEq(bytes(nameRegistry.getName(alice)).length, 0);
        assertEq(nameRegistry.getAddress("alice"), address(0));
    }

    function test_RevertRemoveNoName() public {
        vm.prank(alice);
        vm.expectRevert("No name registered");
        nameRegistry.removeName();
    }

    function test_RevertNameTooLong() public {
        vm.prank(alice);
        vm.expectRevert("Name must be 1-32 chars");
        nameRegistry.registerName("this-name-is-way-too-long-for-the-registry");
    }

    function test_RevertEmptyName() public {
        vm.prank(alice);
        vm.expectRevert("Name must be 1-32 chars");
        nameRegistry.registerName("");
    }

    function test_RevertInvalidChars() public {
        vm.prank(alice);
        vm.expectRevert("Invalid chars (use a-z, 0-9, -, _)");
        nameRegistry.registerName("alice@home");
    }

    function test_RevertSpaceInName() public {
        vm.prank(alice);
        vm.expectRevert("Invalid chars (use a-z, 0-9, -, _)");
        nameRegistry.registerName("alice rocks");
    }

    function test_ValidNameChars() public {
        vm.prank(alice);
        nameRegistry.registerName("alice-123_ABC");
        assertEq(nameRegistry.getName(alice), "alice-123_ABC");
    }

    function test_RevertNameAlreadyTaken() public {
        vm.prank(alice);
        nameRegistry.registerName("uniquename");

        vm.prank(bob);
        vm.expectRevert("Name already taken");
        nameRegistry.registerName("uniquename");
    }

    function test_FreedNameCanBeReused() public {
        vm.prank(alice);
        nameRegistry.registerName("coolname");

        vm.prank(alice);
        nameRegistry.removeName();

        // Bob can now take it
        vm.prank(bob);
        nameRegistry.registerName("coolname");
        assertEq(nameRegistry.getAddress("coolname"), bob);
    }

    function test_BatchGetNames() public {
        vm.prank(alice);
        nameRegistry.registerName("alice");
        vm.prank(bob);
        nameRegistry.registerName("bob");

        address[] memory addrs = new address[](3);
        addrs[0] = alice;
        addrs[1] = bob;
        addrs[2] = carol; // no name registered

        string[] memory names = nameRegistry.getNames(addrs);
        assertEq(names[0], "alice");
        assertEq(names[1], "bob");
        assertEq(bytes(names[2]).length, 0);
    }

    function test_EmitNameRegistered() public {
        vm.prank(alice);
        vm.expectEmit(true, false, false, true);
        emit AraNameRegistry.NameRegistered(alice, "alice");
        nameRegistry.registerName("alice");
    }

    function test_EmitNameRemoved() public {
        vm.prank(alice);
        nameRegistry.registerName("alice");

        vm.prank(alice);
        vm.expectEmit(true, false, false, true);
        emit AraNameRegistry.NameRemoved(alice, "alice");
        nameRegistry.removeName();
    }

    function test_SameUserCanReclaimOwnName() public {
        vm.startPrank(alice);
        nameRegistry.registerName("myname");
        // Update to something else then back
        nameRegistry.registerName("other");
        nameRegistry.registerName("myname");
        vm.stopPrank();

        assertEq(nameRegistry.getName(alice), "myname");
    }
}
