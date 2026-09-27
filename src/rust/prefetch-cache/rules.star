# prefetch-cache - shared cache for Nix prefetch hashes
load("@prelude//:rules.bzl", "rust_library", "rust_test")

rust_library(
    name = "prefetch-cache",
    srcs = glob(["src/**/*.rs"]),
    edition = "2024",
    deps = [
        "rustdeps//vendor/anyhow:anyhow",
        "rustdeps//vendor/chrono:chrono",
        "rustdeps//vendor/dirs:dirs",
        "rustdeps//vendor/serde:serde",
        "rustdeps//vendor/serde_json:serde_json",
    ],
    visibility = ["PUBLIC"],
)

rust_test(
    name = "prefetch-cache-test",
    srcs = glob(["src/**/*.rs"]),
    edition = "2024",
    deps = [
        "rustdeps//vendor/anyhow:anyhow",
        "rustdeps//vendor/chrono:chrono",
        "rustdeps//vendor/dirs:dirs",
        "rustdeps//vendor/serde:serde",
        "rustdeps//vendor/serde_json:serde_json",
        "rustdeps//vendor/tempfile:tempfile",
    ],
)
