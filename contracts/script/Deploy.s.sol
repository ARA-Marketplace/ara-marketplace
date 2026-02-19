// SPDX-License-Identifier: LGPL-3.0
pragma solidity ^0.8.24;

import {Script, console} from "forge-std/Script.sol";
import {AraStaking} from "../src/AraStaking.sol";
import {ContentRegistry} from "../src/ContentRegistry.sol";
import {Marketplace} from "../src/Marketplace.sol";
import {MockARAToken} from "../src/MockARAToken.sol";

/// @notice Deploy all Ara Marketplace contracts to any network.
/// Usage:
///   Sepolia: forge script script/Deploy.s.sol --rpc-url $SEPOLIA_RPC_URL --broadcast --verify
///   Mainnet: forge script script/Deploy.s.sol --rpc-url $ETH_RPC_URL --broadcast --verify
contract DeployScript is Script {
    // Mainnet ARA token (existing, not redeployed)
    address constant ARA_TOKEN_MAINNET = 0xa92E7c82B11d10716aB534051B271D2f6aEf7Df5;

    uint256 constant CREATOR_SHARE_BPS = 8500; // 85% to creator

    function run() external {
        uint256 deployerPrivateKey = vm.envUint("DEPLOYER_PRIVATE_KEY");
        address deployer = vm.addr(deployerPrivateKey);

        vm.startBroadcast(deployerPrivateKey);

        if (block.chainid == 1) {
            // Mainnet: use existing ARA token, production stakes
            _deploy(ARA_TOKEN_MAINNET, 1000 ether, 100 ether, CREATOR_SHARE_BPS, deployer);
        } else {
            // Testnet: deploy mintable mock token, low stakes for easy testing
            MockARAToken mock = new MockARAToken(deployer);
            console.log("MockARAToken deployed at:", address(mock));

            // Mint 10,000,000 tARA to deployer
            mock.mint(deployer, 10_000_000 ether);
            console.log("Minted 10,000,000 tARA to deployer:", deployer);

            // 10 tARA to publish, 1 tARA to seed
            _deploy(address(mock), 10 ether, 1 ether, CREATOR_SHARE_BPS, deployer);
        }

        vm.stopBroadcast();
    }

    function _deploy(
        address araToken,
        uint256 publisherMinStake,
        uint256 seederMinStake,
        uint256 creatorShareBps,
        address /*deployer*/
    ) internal {
        AraStaking staking = new AraStaking(araToken, publisherMinStake, seederMinStake);
        ContentRegistry registry = new ContentRegistry(address(staking));
        Marketplace marketplace = new Marketplace(address(registry), address(staking), creatorShareBps);

        console.log("");
        console.log("=== Deployment Summary ===");
        console.log("Chain ID:            ", block.chainid);
        console.log("ARA Token:           ", araToken);
        console.log("AraStaking:          ", address(staking));
        console.log("ContentRegistry:     ", address(registry));
        console.log("Marketplace:         ", address(marketplace));
        console.log("Creator Share:        85%");
        console.log("Publisher Min Stake: ", publisherMinStake / 1 ether, "ARA");
        console.log("Seeder Min Stake:    ", seederMinStake / 1 ether, "ARA");
        console.log("");
        console.log("=== Paste into AppConfig::default() ===");
        console.log("chain_id:             11155111");
        console.log("ara_token_address:   ", araToken);
        console.log("staking_address:     ", address(staking));
        console.log("registry_address:    ", address(registry));
        console.log("marketplace_address: ", address(marketplace));
    }
}
