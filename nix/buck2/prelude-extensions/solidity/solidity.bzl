# Copyright (c) Firefly Engineering and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

"""Solidity rules for Buck2.

This module provides rules for compiling Solidity smart contracts.

Example usage:

    load("@prelude//solidity:solidity.bzl", "solidity_library", "solidity_contract", "solidity_test")

    # Compile Solidity sources
    solidity_library(
        name = "token_lib",
        srcs = ["src/Token.sol"],
        deps = ["//soldeps:openzeppelin_contracts"],
        remappings = {
            "@openzeppelin/": "//soldeps:openzeppelin_contracts/",
        },
        optimizer = True,
        optimizer_runs = 200,
    )

    # Extract specific contract artifacts
    solidity_contract(
        name = "token",
        contract = "Token",
        lib = ":token_lib",
    )

    # Run Foundry tests
    solidity_test(
        name = "token_test",
        srcs = ["test/Token.t.sol"],
        deps = [":token_lib", "//soldeps:forge_std"],
        fuzz_runs = 256,
    )
"""

load(
    ":providers.bzl",
    _SolidityContractInfo = "SolidityContractInfo",
    _SolidityLibraryInfo = "SolidityLibraryInfo",
    _SolidityToolchainInfo = "SolidityToolchainInfo",
)
load(":solidity_contract.bzl", _solidity_contract = "solidity_contract")
load(":solidity_library.bzl", _solidity_library = "solidity_library")
load(":solidity_test.bzl", _solidity_test = "solidity_test")
load(":toolchain.bzl", _system_solidity_toolchain = "system_solidity_toolchain")

# Re-export providers
SolidityToolchainInfo = _SolidityToolchainInfo
SolidityLibraryInfo = _SolidityLibraryInfo
SolidityContractInfo = _SolidityContractInfo

# Re-export rules
system_solidity_toolchain = _system_solidity_toolchain
solidity_library = _solidity_library
solidity_contract = _solidity_contract
solidity_test = _solidity_test

# Rule implementations for registration with prelude
implemented_rules = {
    "solidity_library": _solidity_library,
    "solidity_contract": _solidity_contract,
    "solidity_test": _solidity_test,
    "system_solidity_toolchain": _system_solidity_toolchain,
}

# Extra attributes for rules (if needed for toolchain injection)
extra_attributes = {
    "solidity_library": {},
    "solidity_contract": {},
    "solidity_test": {},
    "system_solidity_toolchain": {},
}
