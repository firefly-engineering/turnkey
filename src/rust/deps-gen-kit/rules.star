# deps-gen-kit - what turnkey's deps generators share
load("@prelude//:rules.bzl", "rust_library", "rust_test")

rust_library(
    name = "deps-gen-kit",
    srcs = glob(["src/**/*.rs"]),
    edition = "2024",
    deps = [
        "rustdeps//vendor/anyhow:anyhow",
        "rustdeps//vendor/clap:clap",
        "rustdeps//vendor/serde:serde",
        "rustdeps//vendor/toml:toml",
        "//src/rust/gostd:gostd",
        "//src/rust/prefetch-cache:prefetch-cache",
    ],
    visibility = ["PUBLIC"],
)

rust_test(
    name = "deps-gen-kit-test",
    srcs = glob(["src/**/*.rs"]),
    edition = "2024",
    deps = [
        "rustdeps//vendor/anyhow:anyhow",
        "rustdeps//vendor/clap:clap",
        "rustdeps//vendor/serde:serde",
        "rustdeps//vendor/toml:toml",
        "//src/rust/gostd:gostd",
        "//src/rust/prefetch-cache:prefetch-cache",
    ],
)
