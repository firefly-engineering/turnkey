# Auto-managed by turnkey. Hash: 4be24c3291bfaffe
# Manual sections marked with turnkey:preserve-start/end are not modified.

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
