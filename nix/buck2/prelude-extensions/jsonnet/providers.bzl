# Copyright (c) Firefly Engineering and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

"""Jsonnet providers for Buck2."""

JsonnetToolchainInfo = provider(
    doc = "Information about the Jsonnet toolchain.",
    fields = {
        "jsonnet": provider_field(typing.Any, default = None),  # RunInfo
    },
)

JsonnetLibraryInfo = provider(
    doc = "Information about compiled Jsonnet output.",
    fields = {
        "output": provider_field(typing.Any, default = None),  # Artifact (JSON output file or directory)
        "sources": provider_field(typing.Any, default = None),  # list[Artifact] (source .jsonnet files)
        "import_paths": provider_field(typing.Any, default = None),  # list[str] (paths for -J flag)
    },
)
