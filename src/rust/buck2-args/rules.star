# buck2-args - buck2's command line as tk reads and rewrites it, and the
# local target overrides it injects
load("@prelude//:rules.bzl", "rust_library", "rust_test")

rust_library(
    name = "buck2-args",
    srcs = glob(["src/**/*.rs"]),
    edition = "2024",
    deps = [
        "rustdeps//vendor/toml:toml",
    ],
    visibility = ["PUBLIC"],
)

rust_test(
    name = "buck2-args-test",
    srcs = glob(["src/**/*.rs"]),
    edition = "2024",
    deps = [
        "rustdeps//vendor/tempfile:tempfile",
        "rustdeps//vendor/toml:toml",
    ],
)
