// SPDX-License-Identifier: LGPL-3.0
pragma solidity ^0.8.24;

import {ERC20} from "openzeppelin-contracts/contracts/token/ERC20/ERC20.sol";

/// @title MockARAToken
/// @notice Testnet-only mintable ERC20 compatible with IAraToken.
///         Implements the legacy deposit/withdraw/amountDeposited as no-ops
///         so it satisfies AraStaking's interface without mainnet token behaviour.
contract MockARAToken is ERC20 {
    address public owner;
    mapping(address => uint256) private _deposits;

    modifier onlyOwner() {
        require(msg.sender == owner, "MockARAToken: not owner");
        _;
    }

    constructor(address initialOwner) ERC20("Ara Token (Test)", "tARA") {
        owner = initialOwner;
    }

    /// @notice Mint tokens to any address. Only callable by owner (deployer).
    function mint(address to, uint256 amount) external onlyOwner {
        _mint(to, amount);
    }

    // --- IAraToken extras not in standard ERC20 ---

    function increaseApproval(address spender, uint256 addedValue) external returns (bool) {
        _approve(msg.sender, spender, allowance(msg.sender, spender) + addedValue);
        return true;
    }

    function decreaseApproval(address spender, uint256 subtractedValue) external returns (bool) {
        uint256 current = allowance(msg.sender, spender);
        _approve(msg.sender, spender, current >= subtractedValue ? current - subtractedValue : 0);
        return true;
    }

    // Legacy deposit/withdraw/amountDeposited — no-ops on testnet
    function deposit(uint256 value) external returns (bool) {
        _deposits[msg.sender] += value;
        return true;
    }

    function withdraw(uint256 value) external returns (bool) {
        if (_deposits[msg.sender] >= value) _deposits[msg.sender] -= value;
        return true;
    }

    function amountDeposited(address account) external view returns (uint256) {
        return _deposits[account];
    }
}
