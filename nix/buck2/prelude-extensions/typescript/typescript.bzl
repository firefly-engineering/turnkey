# Copyright (c) Firefly Engineering and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

"""TypeScript rules for Buck2.

This module provides rules for compiling TypeScript to JavaScript.

Example usage:

    load("@prelude//typescript:typescript.bzl", "typescript_library", "typescript_binary")

    typescript_library(
        name = "mylib",
        srcs = glob(["src/**/*.ts"]),
    )

    typescript_binary(
        name = "myapp",
        main = "src/main.ts",
        srcs = ["src/main.ts"],
        deps = [":mylib"],
    )
"""

load(":providers.bzl", _TypeScriptLibraryInfo = "TypeScriptLibraryInfo", _TypeScriptToolchainInfo = "TypeScriptToolchainInfo")
load(":toolchain.bzl", _system_typescript_toolchain = "system_typescript_toolchain")
load(":ts_binary.bzl", _typescript_binary = "typescript_binary")
load(":ts_library.bzl", _typescript_library = "typescript_library")

# Re-export providers
TypeScriptToolchainInfo = _TypeScriptToolchainInfo
TypeScriptLibraryInfo = _TypeScriptLibraryInfo

# Re-export rules
system_typescript_toolchain = _system_typescript_toolchain
typescript_library = _typescript_library
typescript_binary = _typescript_binary

# Rule implementations for registration with prelude
implemented_rules = {
    "typescript_library": _typescript_library,
    "typescript_binary": _typescript_binary,
    "system_typescript_toolchain": _system_typescript_toolchain,
}

# Extra attributes for rules (if needed for toolchain injection)
extra_attributes = {
    "typescript_library": {},
    "typescript_binary": {},
    "system_typescript_toolchain": {},
}
