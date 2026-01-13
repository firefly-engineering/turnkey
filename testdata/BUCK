# Test data fixtures
load("@prelude//:rules.bzl", "export_file", "filegroup")

# Godeps integration test fixtures
filegroup(
    name = "godeps_fixtures",
    srcs = glob(["godeps/**/*"]),
    visibility = ["PUBLIC"],
)

# Rust/Cargo test data
export_file(
    name = "sample_cargo_lock",
    src = "sample_cargo.lock",
    visibility = ["PUBLIC"],
)
