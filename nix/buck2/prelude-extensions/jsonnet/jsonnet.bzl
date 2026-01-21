# Copyright (c) Firefly Engineering and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

"""Jsonnet rules for Buck2.

This module provides rules for compiling and testing Jsonnet files.

Example usage:
    load("@prelude//jsonnet:jsonnet.bzl", "jsonnet_library", "jsonnet_test")

    jsonnet_library(
        name = "config",
        srcs = ["config.jsonnet"],
        ext_strs = {"env": "production"},
    )

    # Test with assertions (std.assertEqual)
    jsonnet_test(
        name = "lib_test",
        src = "lib_test.jsonnet",
        deps = [":mylib"],
    )

    # Test with golden file comparison
    jsonnet_test(
        name = "config_test",
        src = "config.jsonnet",
        golden = "config.expected.json",
    )
"""

load(":jsonnet_library.bzl", _jsonnet_library = "jsonnet_library")
load(":jsonnet_test.bzl", _jsonnet_test = "jsonnet_test")
load(":providers.bzl", _JsonnetLibraryInfo = "JsonnetLibraryInfo", _JsonnetToolchainInfo = "JsonnetToolchainInfo")
load(":toolchain.bzl", _system_jsonnet_toolchain = "system_jsonnet_toolchain")

# Re-export rules
jsonnet_library = _jsonnet_library
jsonnet_test = _jsonnet_test
system_jsonnet_toolchain = _system_jsonnet_toolchain

# Re-export providers
JsonnetLibraryInfo = _JsonnetLibraryInfo
JsonnetToolchainInfo = _JsonnetToolchainInfo
