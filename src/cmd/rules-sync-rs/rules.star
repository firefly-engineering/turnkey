# rules-sync-rs - rules-sync's Rust port, whose binary is rules-sync
load("@prelude//:rules.bzl", "rust_binary", "rust_test")

rust_binary(
    name = "rules-sync-rs",
    srcs = glob(["src/**/*.rs"]),
    crate_root = "src/main.rs",
    edition = "2024",
    deps = [
        "//src/rust/gostd:gostd",
        "//src/rust/project-sync:project-sync",
        "//src/rust/rules-sync:rules-sync",
    ],
    visibility = ["PUBLIC"],
)

rust_test(
    name = "rules-sync-rs-test",
    srcs = glob(["src/**/*.rs"]),
    crate_root = "src/main.rs",
    edition = "2024",
    deps = [
        "//src/rust/gostd:gostd",
        "//src/rust/project-sync:project-sync",
        "//src/rust/rules-sync:rules-sync",
        "rustdeps//vendor/serde_json:serde_json",
        "rustdeps//vendor/tempfile:tempfile",
    ],
)
