# gomod - go.mod and go.work files, module paths and versions, as
# golang.org/x/mod reads them
load("@prelude//:rules.bzl", "rust_library", "rust_test")

rust_library(
    name = "gomod",
    srcs = glob(["src/**/*.rs"]),
    edition = "2024",
    deps = [
        "//src/rust/gostd:gostd",
    ],
    visibility = ["PUBLIC"],
)

rust_test(
    name = "gomod-test",
    srcs = glob(["src/**/*.rs"]),
    edition = "2024",
    deps = [
        "//src/rust/gostd:gostd",
    ],
)
