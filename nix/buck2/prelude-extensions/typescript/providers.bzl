# Copyright (c) Firefly Engineering and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

"""TypeScript providers for Buck2."""

TypeScriptToolchainInfo = provider(
    doc = "Information about the TypeScript toolchain.",
    fields = {
        "node": provider_field(typing.Any, default = None),  # RunInfo
        "tsc": provider_field(typing.Any, default = None),  # RunInfo
        "tsc_flags": provider_field(typing.Any, default = []),  # list[str]
    },
)

TypeScriptLibraryInfo = provider(
    doc = "Information about a compiled TypeScript library.",
    fields = {
        "output_dir": provider_field(typing.Any, default = None),  # Artifact
        "declaration_dir": provider_field(typing.Any, default = None),  # Artifact | None
        "srcs": provider_field(typing.Any, default = []),  # list[Artifact]
        "transitive_outputs": provider_field(typing.Any, default = None),  # TransitiveSet
    },
)

# Define transitive set for outputs
def _ts_output_artifacts(value: Artifact):
    return value

TypeScriptOutputsTSet = transitive_set(args_projections = {"artifacts": _ts_output_artifacts})

def get_transitive_outputs(
        actions: AnalysisActions,
        value: Artifact | None = None,
        deps: list[TypeScriptLibraryInfo] = []) -> TypeScriptOutputsTSet:
    """Build a transitive set of compiled TypeScript outputs."""
    kwargs = {}
    if value:
        kwargs["value"] = value
    if deps:
        kwargs["children"] = [dep.transitive_outputs for dep in deps if dep.transitive_outputs]

    return actions.tset(TypeScriptOutputsTSet, **kwargs)
