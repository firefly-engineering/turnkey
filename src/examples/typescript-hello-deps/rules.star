# TypeScript example with npm dependencies
#
# This example demonstrates using npm packages with typescript_binary.
# The lodash package is fetched from npm and linked via the jsdeps cell.

load("@prelude//typescript:typescript.bzl", "typescript_binary")

typescript_binary(
    name = "typescript-hello-deps",
    main = "main.ts",
    srcs = ["main.ts"],
    npm_deps = [
        "jsdeps//:lodash",
        "jsdeps//:types_lodash",
    ],
    visibility = ["PUBLIC"],
)
