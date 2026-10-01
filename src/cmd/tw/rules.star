# tw - native tool wrapper with auto-sync: runs a native tool, and syncs
# the deps files it changes
load("@prelude//:rules.bzl", "rust_binary", "rust_test")

rust_binary(
    name = "tw",
    srcs = glob(["src/**/*.rs"]),
    edition = "2024",
    deps = [
        "//src/rust/gostd:gostd",
        "//src/rust/project-sync:project-sync",
        "rustdeps//vendor/anyhow:anyhow",
        "rustdeps//vendor/sha2:sha2",
    ],
    visibility = ["PUBLIC"],
)

rust_test(
    name = "tw-test",
    srcs = glob(["src/**/*.rs"]),
    edition = "2024",
    deps = [
        "//src/rust/gostd:gostd",
        "//src/rust/project-sync:project-sync",
        "rustdeps//vendor/anyhow:anyhow",
        "rustdeps//vendor/sha2:sha2",
        "rustdeps//vendor/tempfile:tempfile",
    ],
)
