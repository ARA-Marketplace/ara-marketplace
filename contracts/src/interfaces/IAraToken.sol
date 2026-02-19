// SPDX-License-Identifier: LGPL-3.0
pragma solidity ^0.8.24;

/// @title IAraToken
/// @notice Interface for the existing ARA ERC20 token on Ethereum mainnet.
/// @dev Mainnet proxy: 0xa92e7c82b11d10716ab534051b271d2f6aef7df5
///      The token has a legacy deposit/withdraw mechanism that constrains approve():
///      approve() requires balanceOf(sender) - value >= deposits[sender].
///      Users with legacy deposits must withdraw() before approving new spenders.
interface IAraToken {
    function name() external view returns (string memory);
    function symbol() external view returns (string memory);
    function decimals() external view returns (uint256);
    function totalSupply() external view returns (uint256);

    function balanceOf(address account) external view returns (uint256);
    function transfer(address to, uint256 amount) external returns (bool);
    function approve(address spender, uint256 amount) external returns (bool);
    function transferFrom(address from, address to, uint256 amount) external returns (bool);
    function allowance(address owner, address spender) external view returns (uint256);
    function increaseApproval(address spender, uint256 addedValue) external returns (bool);
    function decreaseApproval(address spender, uint256 subtractedValue) external returns (bool);

    // Legacy deposit mechanism (informational — new contracts use approve/transferFrom)
    function deposit(uint256 value) external returns (bool);
    function withdraw(uint256 value) external returns (bool);
    function amountDeposited(address owner) external view returns (uint256);
}
