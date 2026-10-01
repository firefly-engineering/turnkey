# gostd - ports of the Go standard library behaviours turnkey's generated
# files depend on
load("@prelude//:rules.bzl", "rust_library", "rust_test")

rust_library(
    name = "gostd",
    srcs = glob(["src/**/*.rs"]),
    edition = "2024",
    deps = [
        "rustdeps//vendor/unicode-properties:unicode-properties",
    ],
    visibility = ["PUBLIC"],
)

rust_test(
    name = "gostd-test",
    srcs = glob(["src/**/*.rs"]),
    edition = "2024",
    deps = [
        "rustdeps//vendor/unicode-properties:unicode-properties",
    ],
)
