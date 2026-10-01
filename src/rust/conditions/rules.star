# conditions - the build configurations deps depend on, and the select()
# turnkey writes for them
load("@prelude//:rules.bzl", "rust_library", "rust_test")

rust_library(
    name = "conditions",
    srcs = glob(["src/**/*.rs"]),
    edition = "2024",
    deps = [
        "//src/rust/deps-gen-kit:deps-gen-kit",
        "//src/rust/gostd:gostd",
        "rustdeps//vendor/serde:serde",
    ],
    visibility = ["PUBLIC"],
)

rust_test(
    name = "conditions-test",
    srcs = glob(["src/**/*.rs"]),
    edition = "2024",
    deps = [
        "//src/rust/deps-gen-kit:deps-gen-kit",
        "//src/rust/gostd:gostd",
        "rustdeps//vendor/serde:serde",
        "rustdeps//vendor/serde_json:serde_json",
    ],
    # The shared split cases (src/testdata), embedded from where testdata/
    # links to them
    mapped_srcs = {"//src/testdata:split-vectors": "testdata/split-vectors.json"},
)
