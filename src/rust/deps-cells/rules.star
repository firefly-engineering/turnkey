# deps-cells - the deps cells under .turnkey: materialized from their cell
# indexes, kept fresh in buck2's daemon, and edited into patches
load("@prelude//:rules.bzl", "rust_library", "rust_test")

rust_library(
    name = "deps-cells",
    srcs = glob(["src/**/*.rs"]),
    edition = "2024",
    deps = [
        "//src/rust/gostd:gostd",
        "rustdeps//vendor/serde:serde",
        "rustdeps//vendor/serde_json:serde_json",
        "rustdeps//vendor/sha2:sha2",
        "rustdeps//vendor/tempfile:tempfile",
        "rustdeps//vendor/walkdir:walkdir",
    ],
    visibility = ["PUBLIC"],
)

rust_test(
    name = "deps-cells-test",
    srcs = glob(["src/**/*.rs"]),
    edition = "2024",
    deps = [
        "//src/rust/gostd:gostd",
        "rustdeps//vendor/serde:serde",
        "rustdeps//vendor/serde_json:serde_json",
        "rustdeps//vendor/sha2:sha2",
        "rustdeps//vendor/tempfile:tempfile",
        "rustdeps//vendor/walkdir:walkdir",
    ],
)
