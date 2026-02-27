// SPDX-License-Identifier: LGPL-3.0
pragma solidity ^0.8.24;

import {Script, console} from "forge-std/Script.sol";
import {ERC1967Proxy} from "@openzeppelin/contracts/proxy/ERC1967/ERC1967Proxy.sol";
import {AraStaking} from "../src/AraStaking.sol";
import {AraContent} from "../src/AraContent.sol";
import {Marketplace} from "../src/Marketplace.sol";
import {AraCollections} from "../src/AraCollections.sol";
import {AraNameRegistry} from "../src/AraNameRegistry.sol";
import {MockARAToken} from "../src/MockARAToken.sol";

/// @notice Deploy all Ara Marketplace contracts with UUPS proxies.
/// Usage:
///   Sepolia: forge script script/Deploy.s.sol --rpc-url $SEPOLIA_RPC_URL --broadcast --verify
///   Mainnet: forge script script/Deploy.s.sol --rpc-url $ETH_RPC_URL --broadcast --verify
contract DeployScript is Script {
    // Mainnet ARA token (existing, not redeployed)
    address constant ARA_TOKEN_MAINNET = 0xa92E7c82B11d10716aB534051B271D2f6aEf7Df5;

    uint256 constant CREATOR_SHARE_BPS = 8500; // 85% to creator
    uint256 constant RESALE_REWARD_BPS = 400; // 4% of resale price to seeders
    uint256 constant STAKER_REWARD_BPS = 250; // 2.5% of primary purchase to stakers
    uint256 constant RESALE_STAKER_REWARD_BPS = 100; // 1% of resale price to stakers

    function run() external {
        uint256 deployerPrivateKey = vm.envUint("DEPLOYER_PRIVATE_KEY");
        address deployer = vm.addr(deployerPrivateKey);

        vm.startBroadcast(deployerPrivateKey);

        if (block.chainid == 1) {
            // Mainnet: use existing ARA token, production stakes
            _deploy(ARA_TOKEN_MAINNET, 1000 ether, 100 ether, CREATOR_SHARE_BPS, RESALE_REWARD_BPS);
        } else {
            // Testnet: deploy mintable mock token, low stakes for easy testing
            MockARAToken mock = new MockARAToken(deployer);
            console.log("MockARAToken deployed at:", address(mock));

            // Mint 10,000,000 tARA to deployer
            mock.mint(deployer, 10_000_000 ether);
            console.log("Minted 10,000,000 tARA to deployer:", deployer);

            // 10 tARA to publish, 1 tARA to seed
            _deploy(address(mock), 10 ether, 1 ether, CREATOR_SHARE_BPS, RESALE_REWARD_BPS);
        }

        vm.stopBroadcast();
    }

    function _deploy(
        address araToken,
        uint256 publisherMinStake,
        uint256 seederMinStake,
        uint256 creatorShareBps,
        uint256 resaleRewardBps
    ) internal {
        // 1. Deploy core proxies
        address stakingAddr;
        address contentAddr;
        address marketplaceAddr;
        {
            AraStaking stakingImpl = new AraStaking();
            AraContent contentImpl = new AraContent();
            Marketplace marketplaceImpl = new Marketplace();

            ERC1967Proxy stakingProxy = new ERC1967Proxy(
                address(stakingImpl),
                abi.encodeCall(AraStaking.initialize, (araToken, publisherMinStake, seederMinStake))
            );
            stakingAddr = address(stakingProxy);

            ERC1967Proxy contentProxy = new ERC1967Proxy(
                address(contentImpl), abi.encodeCall(AraContent.initialize, (stakingAddr))
            );
            contentAddr = address(contentProxy);

            ERC1967Proxy marketplaceProxy = new ERC1967Proxy(
                address(marketplaceImpl),
                abi.encodeCall(
                    Marketplace.initialize,
                    (contentAddr, stakingAddr, creatorShareBps, resaleRewardBps)
                )
            );
            marketplaceAddr = address(marketplaceProxy);

            AraContent(contentAddr).setMinter(marketplaceAddr);
            AraStaking(stakingAddr).initializeV2(marketplaceAddr);
            Marketplace(payable(marketplaceAddr)).initializeV2(
                STAKER_REWARD_BPS, RESALE_STAKER_REWARD_BPS, RESALE_REWARD_BPS
            );
        }

        // 2. Deploy AraCollections + AraNameRegistry proxies
        address collectionsAddr;
        address nameRegistryAddr;
        {
            AraCollections collectionsImpl = new AraCollections();
            ERC1967Proxy collectionsProxy = new ERC1967Proxy(
                address(collectionsImpl),
                abi.encodeCall(AraCollections.initialize, (contentAddr))
            );
            collectionsAddr = address(collectionsProxy);

            AraNameRegistry nameRegistryImpl = new AraNameRegistry();
            ERC1967Proxy nameRegistryProxy = new ERC1967Proxy(
                address(nameRegistryImpl),
                abi.encodeCall(AraNameRegistry.initialize, ())
            );
            nameRegistryAddr = address(nameRegistryProxy);
        }

        // 3. Log summary
        _logSummary(araToken, stakingAddr, contentAddr, marketplaceAddr,
                     collectionsAddr, nameRegistryAddr, publisherMinStake, seederMinStake);
    }

    function _logSummary(
        address araToken,
        address stakingAddr,
        address contentAddr,
        address marketplaceAddr,
        address collectionsAddr,
        address nameRegistryAddr,
        uint256 publisherMinStake,
        uint256 seederMinStake
    ) internal view {
        console.log("");
        console.log("=== Deployment Summary (UUPS Proxies) ===");
        console.log("Chain ID:                ", block.chainid);
        console.log("ARA Token:               ", araToken);
        console.log("AraStaking (proxy):      ", stakingAddr);
        console.log("AraContent (proxy):      ", contentAddr);
        console.log("Marketplace (proxy):     ", marketplaceAddr);
        console.log("AraCollections (proxy):  ", collectionsAddr);
        console.log("AraNameRegistry (proxy): ", nameRegistryAddr);
        console.log("Creator Share:            85%");
        console.log("Staker Share:             2.5% (primary), 1% (resale)");
        console.log("Seeder Share:             12.5% (primary), 4% (resale)");
        console.log("Publisher Min Stake:     ", publisherMinStake / 1 ether, "ARA");
        console.log("Seeder Min Stake:        ", seederMinStake / 1 ether, "ARA");
        console.log("");
        console.log("=== Paste into AppConfig::default() ===");
        console.log("chain_id:                 11155111");
        console.log("ara_token_address:       ", araToken);
        console.log("staking_address:         ", stakingAddr);
        console.log("registry_address:        ", contentAddr);
        console.log("marketplace_address:     ", marketplaceAddr);
        console.log("collections_address:     ", collectionsAddr);
        console.log("name_registry_address:   ", nameRegistryAddr);
        console.log("");
        console.log("NOTE: Config uses PROXY addresses (permanent). Implementation addresses are irrelevant.");
    }
}
