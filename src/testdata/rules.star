# Test data fixtures
load("@prelude//:rules.bzl", "export_file")

# Rust/Cargo test data
export_file(
    name = "sample_cargo_lock",
    src = "sample_cargo.lock",
    visibility = ["PUBLIC"],
)
