# pep508 - Python dependency specifiers (PEP 508), parsed, and their
# environment markers evaluated
load("@prelude//:rules.bzl", "rust_library", "rust_test")

rust_library(
    name = "pep508",
    srcs = glob(["src/**/*.rs"]),
    edition = "2024",
    deps = [
        "//src/rust/conditions:conditions",
        "//src/rust/gostd:gostd",
        "rustdeps//vendor/anyhow:anyhow",
        "rustdeps//vendor/serde:serde",
    ],
    visibility = ["PUBLIC"],
)

rust_test(
    name = "pep508-test",
    srcs = glob(["src/**/*.rs"]),
    edition = "2024",
    deps = [
        "//src/rust/conditions:conditions",
        "//src/rust/gostd:gostd",
        "rustdeps//vendor/anyhow:anyhow",
        "rustdeps//vendor/serde:serde",
        "rustdeps//vendor/serde_json:serde_json",
    ],
    # The PEP 508 cases src/go/pkg/pep508 runs too, embedded from where
    # testdata/ links to them
    mapped_srcs = {"//src/go/pkg/pep508:pep508-vectors": "testdata/pep508-vectors.json"},
)
