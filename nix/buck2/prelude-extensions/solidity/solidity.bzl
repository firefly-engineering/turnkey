# Copyright (c) Firefly Engineering and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

"""Solidity rules for Buck2.

This module provides rules for compiling Solidity smart contracts.

forge drives both solidity_library and solidity_test, in a forge project
staged from the root foundry.toml and remappings.txt, the sources at their
repository-relative paths and the soldeps bundle at the cell link's path
(forge_project.bzl). Compiler and test settings (optimizer, fuzz runs, ...)
live in the root foundry.toml, as they do for native forge; the compiler is
the toolchain's solc.

Example usage:

    load("@prelude//solidity:solidity.bzl", "solidity_library", "solidity_contract", "solidity_test")

    # Compile Solidity sources
    solidity_library(
        name = "token_lib",
        srcs = ["src/Token.sol"],
        deps = ["soldeps//:openzeppelin_contracts"],
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
        deps = [":token_lib"],
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
solidity_contract = _solidity_contract

def _config(rule_name: str, key: str) -> str:
    """A key of .buckconfig's [solidity], which turnkey generates."""
    value = read_root_config("solidity", key, None)
    if value == None:
        fail(("{}: .buckconfig has no [solidity] {}. turnkey writes that section when " +
              "turnkey.toolchains.buck2.solidity.enable is set, and the root rules.star " +
              "must export foundry.toml, and remappings.txt with Solidity dependencies.").format(rule_name, key))
    return value

def _with_forge_project(rule_name: str, kwargs: dict) -> dict:
    """Default the forge project's inputs (forge_project.bzl) from .buckconfig.

    turnkey's generated .buckconfig carries them in [solidity]: the root
    foundry.toml's target always; with a soldeps cell (`soldeps_dir` set), the
    root remappings.txt's target, the cell's bundle and its link, where the
    bundle is staged. Declaring them makes every setting and dependency source
    an input of the action, instead of a file read in place.
    """
    if "foundry_toml" not in kwargs:
        kwargs["foundry_toml"] = _config(rule_name, "foundry_toml")
    if read_root_config("solidity", "soldeps_dir", None) != None:
        for attr, key in [
            ("soldeps", "soldeps_bundle"),
            ("soldeps_dir", "soldeps_dir"),
            ("remappings_txt", "remappings_txt"),
        ]:
            if attr not in kwargs:
                kwargs[attr] = _config(rule_name, key)
    return kwargs

def solidity_library(**kwargs):
    """solidity_library, with the root forge config and the soldeps bundle as inputs."""
    _solidity_library(**_with_forge_project("solidity_library", kwargs))

def solidity_test(**kwargs):
    """solidity_test, with the root forge config and the soldeps bundle as inputs."""
    _solidity_test(**_with_forge_project("solidity_test", kwargs))

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
