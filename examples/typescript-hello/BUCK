# TypeScript example

load("@prelude//typescript:typescript.bzl", "typescript_binary", "typescript_library")

typescript_library(
    name = "greeter",
    srcs = ["greeter.ts"],
    visibility = ["PUBLIC"],
)

typescript_binary(
    name = "typescript-hello",
    main = "main.ts",
    srcs = ["main.ts"],
    deps = [":greeter"],
    visibility = ["PUBLIC"],
)
