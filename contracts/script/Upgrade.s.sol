// SPDX-License-Identifier: BUSL-1.1
pragma solidity ^0.8.24;

import {Script, console} from "forge-std/Script.sol";
import {AraStaking} from "../src/AraStaking.sol";
import {AraContent} from "../src/AraContent.sol";
import {Marketplace} from "../src/Marketplace.sol";
import {UUPSUpgradeable} from "@openzeppelin/contracts/proxy/utils/UUPSUpgradeable.sol";

/// @notice Upgrade Ara Marketplace proxy contracts to latest implementations.
///
/// This upgrades AraStaking, AraContent, and Marketplace proxies so that
/// new functions (publishContentWithToken, purchaseWithToken, token staking
/// rewards, etc.) become available through the existing proxy addresses.
///
/// Usage:
///   forge script script/Upgrade.s.sol --rpc-url $SEPOLIA_RPC_URL --broadcast --verify
///
/// Requires:
///   DEPLOYER_PRIVATE_KEY - must be the owner of all proxy contracts
contract UpgradeScript is Script {
    // Sepolia proxy addresses (from 2026-02-27 deployment)
    address constant STAKING_PROXY  = 0xfD41Ae37cD729b6a70e42641ea14187e213b29e6;
    address constant CONTENT_PROXY  = 0xd45ff950bBC1c823F66C4EbdF72De23Eb02e4831;
    address constant MARKETPLACE_PROXY = 0xD7992b6A863FBacE3BB58BFE5D31EAe580adF4E0;

    // Sepolia USDC (Circle official)
    address constant SEPOLIA_USDC = 0x1c7D4B196Cb0C7B01d743Fbc6116a902379C7238;

    function run() external {
        uint256 deployerPrivateKey = vm.envUint("DEPLOYER_PRIVATE_KEY");
        vm.startBroadcast(deployerPrivateKey);

        // 1. Deploy new implementations
        AraStaking newStakingImpl = new AraStaking();
        AraContent newContentImpl = new AraContent();
        Marketplace newMarketplaceImpl = new Marketplace();

        console.log("New AraStaking impl:  ", address(newStakingImpl));
        console.log("New AraContent impl:  ", address(newContentImpl));
        console.log("New Marketplace impl: ", address(newMarketplaceImpl));

        // 2. Upgrade each proxy (UUPS: call upgradeToAndCall on the proxy)
        //    Empty bytes = no re-initialization call needed
        UUPSUpgradeable(STAKING_PROXY).upgradeToAndCall(
            address(newStakingImpl), ""
        );
        console.log("AraStaking proxy upgraded");

        UUPSUpgradeable(CONTENT_PROXY).upgradeToAndCall(
            address(newContentImpl), ""
        );
        console.log("AraContent proxy upgraded");

        UUPSUpgradeable(payable(MARKETPLACE_PROXY)).upgradeToAndCall(
            address(newMarketplaceImpl), ""
        );
        console.log("Marketplace proxy upgraded");

        // 3. Whitelist USDC on the Marketplace for token purchases
        Marketplace(payable(MARKETPLACE_PROXY)).setSupportedToken(SEPOLIA_USDC, true);
        console.log("USDC whitelisted on Marketplace:", SEPOLIA_USDC);

        vm.stopBroadcast();

        // Summary
        console.log("");
        console.log("=== Upgrade Complete ===");
        console.log("AraStaking proxy:    ", STAKING_PROXY, " -> impl:", address(newStakingImpl));
        console.log("AraContent proxy:    ", CONTENT_PROXY, " -> impl:", address(newContentImpl));
        console.log("Marketplace proxy:   ", MARKETPLACE_PROXY, " -> impl:", address(newMarketplaceImpl));
        console.log("Sepolia USDC:        ", SEPOLIA_USDC, " (whitelisted)");
        console.log("");
        console.log("Proxy addresses are UNCHANGED. No app config update needed.");
    }
}
