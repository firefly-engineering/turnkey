# tk-rs - tk, turnkey's buck2 wrapper, in Rust (#216): the parts ported so
# far, until its main lands
load("@prelude//:rules.bzl", "rust_library", "rust_test")

rust_library(
    name = "tk-rs",
    srcs = glob(["src/**/*.rs"]),
    crate_root = "src/lib.rs",
    edition = "2024",
    deps = [
        "//src/rust/deps-cells:deps-cells",
        "//src/rust/gostd:gostd",
    ],
    visibility = ["PUBLIC"],
)

rust_test(
    name = "tk-rs-test",
    srcs = glob(["src/**/*.rs"]),
    crate_root = "src/lib.rs",
    edition = "2024",
    deps = [
        "//src/rust/deps-cells:deps-cells",
        "//src/rust/gostd:gostd",
        "rustdeps//vendor/tempfile:tempfile",
    ],
)
