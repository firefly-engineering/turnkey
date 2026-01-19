// SPDX-License-Identifier: MIT
pragma solidity ^0.8.20;

import "@openzeppelin/contracts/token/ERC20/ERC20.sol";
import "@openzeppelin/contracts/token/ERC20/extensions/ERC20Burnable.sol";
import "@openzeppelin/contracts/access/Ownable.sol";

/// @title MyToken
/// @notice A simple ERC20 token with minting and burning capabilities
/// @dev Demonstrates using OpenZeppelin contracts as dependencies
contract MyToken is ERC20, ERC20Burnable, Ownable {
    /// @notice Create a new MyToken
    /// @param initialOwner The address that will own the contract and can mint tokens
    constructor(address initialOwner)
        ERC20("MyToken", "MTK")
        Ownable(initialOwner)
    {
        // Mint initial supply to owner
        _mint(initialOwner, 1000000 * 10 ** decimals());
    }

    /// @notice Mint new tokens (only owner)
    /// @param to The address to mint tokens to
    /// @param amount The amount of tokens to mint
    function mint(address to, uint256 amount) public onlyOwner {
        _mint(to, amount);
    }
}
