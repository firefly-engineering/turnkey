# pydeps-cell-rs - the Rust port of pydeps-cell (#212): the pydeps cell's
# rules.star files, with per-platform dependencies
load("@prelude//:rules.bzl", "rust_binary", "rust_test")

rust_binary(
    name = "pydeps-cell-rs",
    srcs = glob(["src/**/*.rs"]),
    edition = "2024",
    deps = [
        "//src/rust/conditions:conditions",
        "//src/rust/deps-gen-kit:deps-gen-kit",
        "//src/rust/gostd:gostd",
        "//src/rust/pep508:pep508",
        "rustdeps//vendor/anyhow:anyhow",
        "rustdeps//vendor/serde:serde",
        "rustdeps//vendor/serde_json:serde_json",
        "rustdeps//vendor/toml:toml",
    ],
    visibility = ["PUBLIC"],
)

rust_test(
    name = "pydeps-cell-rs-test",
    srcs = glob(["src/**/*.rs"]),
    edition = "2024",
    deps = [
        "//src/rust/conditions:conditions",
        "//src/rust/deps-gen-kit:deps-gen-kit",
        "//src/rust/gostd:gostd",
        "//src/rust/pep508:pep508",
        "rustdeps//vendor/anyhow:anyhow",
        "rustdeps//vendor/serde:serde",
        "rustdeps//vendor/serde_json:serde_json",
        "rustdeps//vendor/toml:toml",
    ],
)
