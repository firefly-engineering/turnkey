# buckgen - the rules.star files of a Go module in the Go deps cell
load("@prelude//:rules.bzl", "rust_binary", "rust_test")

rust_binary(
    name = "buckgen",
    srcs = glob(["src/**/*.rs"]),
    edition = "2024",
    deps = [
        "//src/rust/conditions:conditions",
        "//src/rust/deps-gen-kit:deps-gen-kit",
        "//src/rust/goparse:goparse",
        "//src/rust/gostd:gostd",
        "rustdeps//vendor/serde:serde",
        "rustdeps//vendor/serde_json:serde_json",
    ],
    visibility = ["PUBLIC"],
)

rust_test(
    name = "buckgen-test",
    srcs = glob(["src/**/*.rs"]),
    edition = "2024",
    deps = [
        "//src/rust/conditions:conditions",
        "//src/rust/deps-gen-kit:deps-gen-kit",
        "//src/rust/goparse:goparse",
        "//src/rust/gostd:gostd",
        "rustdeps//vendor/serde:serde",
        "rustdeps//vendor/serde_json:serde_json",
        "rustdeps//vendor/tempfile:tempfile",
    ],
)
