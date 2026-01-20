# nix-prefetch-cached - caching wrapper for nix-prefetch-url
load("@prelude//:rules.bzl", "rust_binary")

rust_binary(
    name = "nix-prefetch-cached",
    srcs = glob(["src/**/*.rs"]),
    edition = "2024",
    deps = [
        "rustdeps//vendor/anyhow:anyhow",
        "rustdeps//vendor/clap:clap",
        "//src/rust/prefetch-cache:prefetch-cache",
    ],
    visibility = ["PUBLIC"],
)
