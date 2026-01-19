# Copyright (c) Firefly Engineering and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

"""mdbook providers for Buck2."""

MdbookToolchainInfo = provider(
    doc = "Information about the mdbook toolchain.",
    fields = {
        "mdbook": provider_field(typing.Any, default = None),  # RunInfo
    },
)

MdbookBookInfo = provider(
    doc = "Information about a built mdbook book.",
    fields = {
        "output_dir": provider_field(typing.Any, default = None),  # Artifact (the book/ directory)
        "book_toml": provider_field(typing.Any, default = None),  # Artifact (book.toml source)
        "src_dir": provider_field(typing.Any, default = None),  # str (path to src directory)
    },
)
