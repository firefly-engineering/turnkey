# project-sync - a turnkey project's .turnkey/sync.toml, and the deps sync
# it drives (tk and tw)
load("@prelude//:rules.bzl", "rust_library", "rust_test")

rust_library(
    name = "project-sync",
    srcs = glob(["src/**/*.rs"]),
    edition = "2024",
    deps = [
        "//src/rust/conditions:conditions",
        "//src/rust/gostd:gostd",
        "rustdeps//vendor/anyhow:anyhow",
        "rustdeps//vendor/libc:libc",
        "rustdeps//vendor/serde:serde",
        "rustdeps//vendor/toml:toml",
    ],
    visibility = ["PUBLIC"],
)

rust_test(
    name = "project-sync-test",
    srcs = glob(["src/**/*.rs"]),
    edition = "2024",
    deps = [
        "//src/rust/conditions:conditions",
        "//src/rust/gostd:gostd",
        "rustdeps//vendor/anyhow:anyhow",
        "rustdeps//vendor/libc:libc",
        "rustdeps//vendor/serde:serde",
        "rustdeps//vendor/tempfile:tempfile",
        "rustdeps//vendor/toml:toml",
    ],
)
