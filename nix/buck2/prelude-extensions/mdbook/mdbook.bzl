# Copyright (c) Firefly Engineering and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

"""mdbook rules for Buck2.

This module provides rules for building mdbook documentation projects.

Example usage in a rules.star file:

    load("@prelude//mdbook:mdbook.bzl", "mdbook_book")

    mdbook_book(
        name = "user-manual",
        book_toml = "book.toml",
        srcs = glob(["src/**/*.md"]),
    )

To build the book:
    tk build //docs/user-manual:user-manual

To serve the book locally for development:
    tk run //docs/user-manual:user-manual
    # or explicitly:
    tk run //docs/user-manual:user-manual[serve]
"""

load(":mdbook_book.bzl", _mdbook_book = "mdbook_book")
load(":providers.bzl", _MdbookBookInfo = "MdbookBookInfo", _MdbookToolchainInfo = "MdbookToolchainInfo")
load(":toolchain.bzl", _system_mdbook_toolchain = "system_mdbook_toolchain")

# Rules
mdbook_book = _mdbook_book
system_mdbook_toolchain = _system_mdbook_toolchain

# Providers
MdbookToolchainInfo = _MdbookToolchainInfo
MdbookBookInfo = _MdbookBookInfo
