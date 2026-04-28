# nix-eval - Nix evaluation and build client
load("@prelude//:rules.bzl", "rust_library")

rust_library(
    name = "nix-eval",
    srcs = glob(["src/**/*.rs"]),
    edition = "2024",
    deps = [
        "rustdeps//vendor/log:log",
        "rustdeps//vendor/serde:serde",
        "rustdeps//vendor/serde_json:serde_json",
        "rustdeps//vendor/thiserror:thiserror",
    ],
    visibility = ["PUBLIC"],
)
