# composition - CompositionBackend trait for FUSE and symlink backends
load("@prelude//:rules.bzl", "rust_library", "rust_test")

rust_library(
    name = "composition",
    srcs = glob(["src/**/*.rs"]),
    edition = "2024",
    deps = [
        "rustdeps//vendor/log:log",
        "rustdeps//vendor/thiserror:thiserror",
    ],
    visibility = ["PUBLIC"],
)

rust_test(
    name = "composition-test",
    srcs = glob(["src/**/*.rs"]),
    edition = "2024",
    deps = [
        "rustdeps//vendor/log:log",
        "rustdeps//vendor/tempfile:tempfile",
        "rustdeps//vendor/thiserror:thiserror",
    ],
)
