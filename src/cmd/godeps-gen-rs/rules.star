# godeps-gen-rs - the Rust port of godeps-gen (#211): go-deps.toml from a
# Go module or workspace
load("@prelude//:rules.bzl", "rust_binary", "rust_test")

rust_binary(
    name = "godeps-gen-rs",
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
    name = "godeps-gen-rs-test",
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
        "rustdeps//vendor/tempfile:tempfile",
    ],
    # The godeps fixtures src/go/pkg/godeps's integration test reads too,
    # embedded from where testdata/ links to them
    mapped_srcs = {"//src/testdata:godeps_fixtures": "testdata"},
)
