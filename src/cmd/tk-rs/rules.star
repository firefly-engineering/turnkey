# tk-rs - tk, turnkey's buck2 wrapper, in Rust (#216), built as a separate
# binary until it switches
load("@prelude//:rules.bzl", "rust_binary", "rust_test")

rust_binary(
    name = "tk-rs",
    srcs = glob(["src/**/*.rs"]),
    crate_root = "src/main.rs",
    edition = "2024",
    deps = [
        "//src/rust/buck2-args:buck2-args",
        "//src/rust/deps-cells:deps-cells",
        "//src/rust/gostd:gostd",
        "//src/rust/project-sync:project-sync",
        "//src/rust/rules-syncer:rules-syncer",
        "//src/rust/testcache:testcache",
    ],
    visibility = ["PUBLIC"],
)

rust_test(
    name = "tk-rs-test",
    srcs = glob(["src/**/*.rs"]),
    crate_root = "src/main.rs",
    edition = "2024",
    deps = [
        "//src/rust/buck2-args:buck2-args",
        "//src/rust/deps-cells:deps-cells",
        "//src/rust/gostd:gostd",
        "//src/rust/project-sync:project-sync",
        "//src/rust/rules-syncer:rules-syncer",
        "//src/rust/testcache:testcache",
        "rustdeps//vendor/tempfile:tempfile",
    ],
)
