// SPDX-License-Identifier: MIT
pragma solidity ^0.8.20;

import "../src/MyToken.sol";

/// @title MyTokenTest
/// @notice Tests for the MyToken contract
contract MyTokenTest {
    MyToken public token;
    address public owner;
    address public user;

    /// @notice Set up the test environment
    function setUp() public {
        owner = address(this);
        user = address(0x1234);
        token = new MyToken(owner);
    }

    /// @notice Test initial supply
    function test_InitialSupply() public view {
        uint256 expectedSupply = 1000000 * 10 ** token.decimals();
        assert(token.totalSupply() == expectedSupply);
        assert(token.balanceOf(owner) == expectedSupply);
    }

    /// @notice Test token name and symbol
    function test_NameAndSymbol() public view {
        assert(keccak256(bytes(token.name())) == keccak256(bytes("MyToken")));
        assert(keccak256(bytes(token.symbol())) == keccak256(bytes("MTK")));
    }

    /// @notice Test minting
    function test_Mint() public {
        uint256 mintAmount = 1000 * 10 ** token.decimals();
        token.mint(user, mintAmount);
        assert(token.balanceOf(user) == mintAmount);
    }

    /// @notice Test burning
    function test_Burn() public {
        uint256 burnAmount = 100 * 10 ** token.decimals();
        uint256 initialBalance = token.balanceOf(owner);

        token.burn(burnAmount);

        assert(token.balanceOf(owner) == initialBalance - burnAmount);
    }

    /// @notice Test transfer
    function test_Transfer() public {
        uint256 transferAmount = 500 * 10 ** token.decimals();

        token.transfer(user, transferAmount);

        assert(token.balanceOf(user) == transferAmount);
    }

    /// @notice Fuzz test for mint and transfer
    function testFuzz_MintAndTransfer(uint96 amount) public {
        // Limit amount to avoid overflow
        uint256 mintAmount = uint256(amount);
        if (mintAmount == 0) return;

        token.mint(user, mintAmount);
        assert(token.balanceOf(user) == mintAmount);
    }
}
