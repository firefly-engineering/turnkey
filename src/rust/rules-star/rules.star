# rules-star - rules.star files read, edited and written back, rewriting
# only what changed
load("@prelude//:rules.bzl", "rust_library", "rust_test")

rust_library(
    name = "rules-star",
    srcs = glob(["src/**/*.rs"]),
    edition = "2024",
    deps = [
        "//src/rust/conditions:conditions",
        "//src/rust/deps-gen-kit:deps-gen-kit",
        "//src/rust/gostd:gostd",
        "rustdeps//vendor/starlark_syntax:starlark_syntax",
    ],
    visibility = ["PUBLIC"],
)

rust_test(
    name = "rules-star-test",
    # The syntax trees go.starlark.net builds, embedded with include_str!
    srcs = glob(["src/**/*.rs", "testdata/**/*"]),
    edition = "2024",
    deps = [
        "//src/rust/conditions:conditions",
        "//src/rust/deps-gen-kit:deps-gen-kit",
        "//src/rust/gostd:gostd",
        "rustdeps//vendor/serde:serde",
        "rustdeps//vendor/serde_json:serde_json",
        "rustdeps//vendor/starlark_syntax:starlark_syntax",
    ],
)
