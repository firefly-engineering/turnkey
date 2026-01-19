# Copyright (c) Firefly Engineering and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

"""Solidity providers for Buck2."""

SolidityToolchainInfo = provider(
    doc = "Information about the Solidity toolchain.",
    fields = {
        "solc": provider_field(typing.Any, default = None),  # RunInfo for default solc
        "solc_versions": provider_field(typing.Any, default = {}),  # dict[str, RunInfo] for multi-version support
        "default_version": provider_field(typing.Any, default = None),  # str - default solc version
        "forge": provider_field(typing.Any, default = None),  # RunInfo for forge (testing)
        "cast": provider_field(typing.Any, default = None),  # RunInfo for cast (interactions)
        "anvil": provider_field(typing.Any, default = None),  # RunInfo for anvil (local node)
        "soldeps_path": provider_field(typing.Any, default = None),  # str - path to soldeps cell for auto-remapping
    },
)

SolidityLibraryInfo = provider(
    doc = "Information about compiled Solidity sources.",
    fields = {
        "output_dir": provider_field(typing.Any, default = None),  # Artifact - compiled artifacts directory
        "srcs": provider_field(typing.Any, default = []),  # list[Artifact] - source .sol files
        "remappings": provider_field(typing.Any, default = {}),  # dict[str, str] - import remappings
        "solc_version": provider_field(typing.Any, default = None),  # str - solc version used
        "transitive_srcs": provider_field(typing.Any, default = None),  # TransitiveSet
        "transitive_remappings": provider_field(typing.Any, default = {}),  # dict[str, str] - all remappings from deps
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

# Transitive set for Solidity source files
def _sol_src_artifacts(value: Artifact):
    return value

SoliditySrcsTSet = transitive_set(args_projections = {"artifacts": _sol_src_artifacts})

def get_transitive_srcs(
        actions: AnalysisActions,
        value: Artifact | None = None,
        deps: list[SolidityLibraryInfo] = []) -> SoliditySrcsTSet:
    """Build a transitive set of Solidity source files."""
    kwargs = {}
    if value:
        kwargs["value"] = value
    if deps:
        kwargs["children"] = [dep.transitive_srcs for dep in deps if dep.transitive_srcs]

    return actions.tset(SoliditySrcsTSet, **kwargs)

def merge_remappings(
        base: dict[str, str],
        deps: list[SolidityLibraryInfo] = []) -> dict[str, str]:
    """Merge import remappings from dependencies, with base taking precedence."""
    result = {}
    # First add all remappings from dependencies
    for dep in deps:
        if dep.transitive_remappings:
            result.update(dep.transitive_remappings)
        if dep.remappings:
            result.update(dep.remappings)
    # Base remappings take precedence
    result.update(base)
    return result
