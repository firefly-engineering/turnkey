# rules-sync - keeps a project's rules.star deps in step with their
# sources, and reports what it did as JSON for tk
load("@prelude//:rules.bzl", "rust_binary", "rust_test")

rust_binary(
    name = "rules-sync",
    srcs = glob(["src/**/*.rs"]),
    crate_root = "src/main.rs",
    edition = "2024",
    deps = [
        "//src/rust/gostd:gostd",
        "//src/rust/project-sync:project-sync",
        "//src/rust/rules-syncer:rules-syncer",
    ],
    visibility = ["PUBLIC"],
)

rust_test(
    name = "rules-sync-test",
    srcs = glob(["src/**/*.rs"]),
    crate_root = "src/main.rs",
    edition = "2024",
    deps = [
        "//src/rust/gostd:gostd",
        "//src/rust/project-sync:project-sync",
        "//src/rust/rules-syncer:rules-syncer",
        "rustdeps//vendor/serde_json:serde_json",
        "rustdeps//vendor/tempfile:tempfile",
    ],
)
