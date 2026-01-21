# Auto-managed by turnkey. Hash: 4be24c3291bfaffe
# Manual sections marked with turnkey:preserve-start/end are not modified.

load("@prelude//typescript:typescript.bzl", "typescript_binary")

typescript_binary(
    name = "typescript-hello-deps",
    srcs = ["main.ts"],
    deps = [
        # turnkey:auto-start
        "jsdeps//:lodash",
        # turnkey:auto-end
    ],
    visibility = ["PUBLIC"],
)
