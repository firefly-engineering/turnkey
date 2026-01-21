# Auto-managed by turnkey. Hash: e3b0c44298fc1c14
# Manual sections marked with turnkey:preserve-start/end are not modified.

load("@prelude//typescript:typescript.bzl", "typescript_binary", "typescript_library")

typescript_library(
    name = "greeter",
    srcs = ["greeter.ts"],
    visibility = ["PUBLIC"],
)

typescript_binary(
    name = "typescript-hello",
    srcs = ["main.ts"],
    visibility = ["PUBLIC"],
)
