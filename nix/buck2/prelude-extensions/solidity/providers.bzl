# Copyright (c) Firefly Engineering and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

"""Solidity providers for Buck2."""

SolidityToolchainInfo = provider(
    doc = "Information about the Solidity toolchain.",
    fields = {
        "solc": provider_field(typing.Any, default = None),  # RunInfo for solc, which forge compiles with (--use)
        "forge": provider_field(typing.Any, default = None),  # RunInfo for forge (compilation and testing)
        "cast": provider_field(typing.Any, default = None),  # RunInfo for cast (interactions)
        "anvil": provider_field(typing.Any, default = None),  # RunInfo for anvil (local node)
        "jq": provider_field(typing.Any, default = None),  # RunInfo for jq (artifact extraction)
    },
)

SolidityLibraryInfo = provider(
    doc = "Information about compiled Solidity sources.",
    fields = {
        "output_dir": provider_field(typing.Any, default = None),  # Artifact - forge's artifacts directory (out/)
        "srcs": provider_field(typing.Any, default = {}),  # dict[str, Artifact] - source .sol files by repository-relative path
        "transitive_srcs": provider_field(typing.Any, default = {}),  # dict[str, Artifact] - srcs plus every solidity_library dep's
    },
)

SolidityContractInfo = provider(
    doc = "Information about a deployable Solidity contract.",
    fields = {
        "contract_name": provider_field(typing.Any, default = None),  # str - contract name
        "abi": provider_field(typing.Any, default = None),  # Artifact - ABI JSON file
        "bytecode": provider_field(typing.Any, default = None),  # Artifact - deployment bytecode
        "deployed_bytecode": provider_field(typing.Any, default = None),  # Artifact - runtime bytecode
        "metadata": provider_field(typing.Any, default = None),  # Artifact - contract metadata JSON
        "source_map": provider_field(typing.Any, default = None),  # Artifact - source map for debugging
    },
)
