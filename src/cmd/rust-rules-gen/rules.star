# rust-rules-gen - generate a vendored Rust crate's rules.star from its package slice
load("@prelude//:rules.bzl", "rust_binary", "rust_test")

rust_binary(
    name = "rust-rules-gen",
    srcs = glob(["src/**/*.rs"]),
    edition = "2024",
    deps = [
        "rustdeps//vendor/anyhow:anyhow",
        "rustdeps//vendor/clap:clap",
        "rustdeps//vendor/serde:serde",
        "rustdeps//vendor/serde_json:serde_json",
        "rustdeps//vendor/toml:toml",
    ],
    visibility = ["PUBLIC"],
)

rust_test(
    name = "rust-rules-gen-test",
    srcs = glob(["src/**/*.rs"]),
    edition = "2024",
    deps = [
        "rustdeps//vendor/anyhow:anyhow",
        "rustdeps//vendor/clap:clap",
        "rustdeps//vendor/serde:serde",
        "rustdeps//vendor/serde_json:serde_json",
        "rustdeps//vendor/toml:toml",
        "rustdeps//vendor/tempfile:tempfile",
    ],
    # The select() keys follow the conditions module's shared test cases,
    # embedded from where testdata/ links to them
    mapped_srcs = {"//src/go/pkg/conditions:split-vectors": "testdata/split-vectors.json"},
)
