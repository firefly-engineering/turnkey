// SPDX-License-Identifier: MIT
pragma solidity ^0.8.20;

/// @title Counter
/// @notice A simple counter contract for demonstration
contract Counter {
    uint256 private _count;

    /// @notice Emitted when the count changes
    event CountChanged(uint256 newCount);

    /// @notice Get the current count
    /// @return The current count value
    function count() public view returns (uint256) {
        return _count;
    }

    /// @notice Increment the counter by 1
    function increment() public {
        _count += 1;
        emit CountChanged(_count);
    }

    /// @notice Decrement the counter by 1
    /// @dev Reverts if count is already 0
    function decrement() public {
        require(_count > 0, "Counter: cannot decrement below zero");
        _count -= 1;
        emit CountChanged(_count);
    }

    /// @notice Set the counter to a specific value
    /// @param newCount The new count value
    function setCount(uint256 newCount) public {
        _count = newCount;
        emit CountChanged(_count);
    }
}
