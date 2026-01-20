// SPDX-License-Identifier: MIT
pragma solidity ^0.8.20;

import "../src/Counter.sol";

/// @title CounterTest
/// @notice Tests for the Counter contract
contract CounterTest {
    Counter public counter;

    /// @notice Set up the test environment
    function setUp() public {
        counter = new Counter();
    }

    /// @notice Test initial count is zero
    function test_InitialCountIsZero() public view {
        assert(counter.count() == 0);
    }

    /// @notice Test increment
    function test_Increment() public {
        counter.increment();
        assert(counter.count() == 1);

        counter.increment();
        assert(counter.count() == 2);
    }

    /// @notice Test decrement
    function test_Decrement() public {
        counter.setCount(5);
        counter.decrement();
        assert(counter.count() == 4);
    }

    /// @notice Test setCount
    function test_SetCount() public {
        counter.setCount(42);
        assert(counter.count() == 42);
    }

    /// @notice Fuzz test for increment/decrement symmetry
    function testFuzz_IncrementDecrement(uint8 times) public {
        for (uint8 i = 0; i < times; i++) {
            counter.increment();
        }
        assert(counter.count() == times);

        for (uint8 i = 0; i < times; i++) {
            counter.decrement();
        }
        assert(counter.count() == 0);
    }
}
