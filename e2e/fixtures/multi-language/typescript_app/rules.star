# TypeScript app for multi-language demo
load("@prelude//typescript:typescript.bzl", "typescript_binary", "typescript_library")

typescript_library(
    name = "greeter",
    srcs = ["greeter.ts"],
    visibility = ["PUBLIC"],
)

typescript_binary(
    name = "hello-typescript",
    main = "main.ts",
    srcs = ["main.ts"],
    deps = [":greeter"],
    visibility = ["PUBLIC"],
)
