# godeps-gen - go-deps.toml from a Go module or workspace
load("@prelude//:rules.bzl", "rust_binary", "rust_test")

rust_binary(
    name = "godeps-gen",
    srcs = glob(["src/**/*.rs"]),
    edition = "2024",
    deps = [
        "//src/rust/deps-gen-kit:deps-gen-kit",
        "//src/rust/gomod:gomod",
        "//src/rust/gostd:gostd",
        "rustdeps//vendor/anyhow:anyhow",
        "rustdeps//vendor/clap:clap",
        "rustdeps//vendor/serde:serde",
        "rustdeps//vendor/serde_json:serde_json",
    ],
    visibility = ["PUBLIC"],
)

rust_test(
    name = "godeps-gen-test",
    # The golden test embeds testdata/godeps with include_str!
    srcs = glob(["src/**/*.rs", "testdata/**/*"]),
    edition = "2024",
    deps = [
        "//src/rust/deps-gen-kit:deps-gen-kit",
        "//src/rust/gomod:gomod",
        "//src/rust/gostd:gostd",
        "rustdeps//vendor/anyhow:anyhow",
        "rustdeps//vendor/clap:clap",
        "rustdeps//vendor/serde:serde",
        "rustdeps//vendor/serde_json:serde_json",
        "rustdeps//vendor/tempfile:tempfile",
    ],
)
