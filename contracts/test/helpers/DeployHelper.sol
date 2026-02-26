// SPDX-License-Identifier: LGPL-3.0
pragma solidity ^0.8.24;

import {Test} from "forge-std/Test.sol";
import {ERC1967Proxy} from "@openzeppelin/contracts/proxy/ERC1967/ERC1967Proxy.sol";
import {AraStaking} from "../../src/AraStaking.sol";
import {AraContent} from "../../src/AraContent.sol";
import {Marketplace} from "../../src/Marketplace.sol";

/// @dev Minimal mock ERC20 for test token
contract MockToken {
    mapping(address => uint256) public balanceOf;
    mapping(address => mapping(address => uint256)) public allowance;

    function mint(address to, uint256 amount) external {
        balanceOf[to] += amount;
    }

    function approve(address spender, uint256 amount) external returns (bool) {
        allowance[msg.sender][spender] = amount;
        return true;
    }

    function transfer(address to, uint256 amount) external returns (bool) {
        balanceOf[msg.sender] -= amount;
        balanceOf[to] += amount;
        return true;
    }

    function transferFrom(address from, address to, uint256 amount) external returns (bool) {
        require(allowance[from][msg.sender] >= amount, "Insufficient allowance");
        allowance[from][msg.sender] -= amount;
        balanceOf[from] -= amount;
        balanceOf[to] += amount;
        return true;
    }
}

/// @dev Base test contract that deploys the full proxy stack
abstract contract DeployHelper is Test {
    MockToken public token;
    AraStaking public staking;
    AraContent public contentToken;
    Marketplace public marketplace;

    uint256 public constant PUBLISHER_MIN = 1000 ether;
    uint256 public constant SEEDER_MIN = 100 ether;
    uint256 public constant CREATOR_SHARE_BPS = 8500;
    uint256 public constant RESALE_REWARD_BPS = 500;

    /// @dev Deploy implementations and proxies. Call from setUp().
    function _deployStack() internal {
        token = new MockToken();

        // Deploy implementations
        AraStaking stakingImpl = new AraStaking();
        AraContent contentImpl = new AraContent();
        Marketplace marketplaceImpl = new Marketplace();

        // Deploy proxies
        ERC1967Proxy stakingProxy = new ERC1967Proxy(
            address(stakingImpl),
            abi.encodeCall(AraStaking.initialize, (address(token), PUBLISHER_MIN, SEEDER_MIN))
        );
        staking = AraStaking(address(stakingProxy));

        ERC1967Proxy contentProxy =
            new ERC1967Proxy(address(contentImpl), abi.encodeCall(AraContent.initialize, (address(stakingProxy))));
        contentToken = AraContent(address(contentProxy));

        ERC1967Proxy marketplaceProxy = new ERC1967Proxy(
            address(marketplaceImpl),
            abi.encodeCall(
                Marketplace.initialize,
                (address(contentProxy), address(stakingProxy), CREATOR_SHARE_BPS, RESALE_REWARD_BPS)
            )
        );
        marketplace = Marketplace(payable(address(marketplaceProxy)));

        // Authorize marketplace to mint
        contentToken.setMinter(address(marketplace));
    }

    /// @dev Compute EIP-712 DeliveryReceipt hash
    function _receiptHash(bytes32 cId, address seederAddr, uint256 bytesServedVal, uint256 ts)
        internal
        view
        returns (bytes32)
    {
        bytes32 structHash =
            keccak256(abi.encode(marketplace.RECEIPT_TYPE_HASH(), cId, seederAddr, bytesServedVal, ts));
        return keccak256(abi.encodePacked("\x19\x01", marketplace.DOMAIN_SEPARATOR(), structHash));
    }

    /// @dev Sign a delivery receipt with a given private key
    function _signReceipt(uint256 privateKey, bytes32 cId, address seederAddr, uint256 bytesServedVal, uint256 ts)
        internal
        view
        returns (bytes memory)
    {
        bytes32 hash = _receiptHash(cId, seederAddr, bytesServedVal, ts);
        (uint8 v, bytes32 r, bytes32 s) = vm.sign(privateKey, hash);
        return abi.encodePacked(r, s, v);
    }
}
