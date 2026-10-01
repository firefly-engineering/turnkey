# Test data fixtures
load("@prelude//:rules.bzl", "export_file")

# Rust/Cargo test data
export_file(
    name = "sample_cargo_lock",
    src = "sample_cargo.lock",
    visibility = ["PUBLIC"],
)

# Test cases shared by several crates and Nix checks (#204)

# The conditions core's label splits: src/rust/conditions, rust-rules-gen
# and the split-vectors flake check (nix/buck2/platforms.nix)
export_file(
    name = "split-vectors",
    src = "split-vectors.json",
    visibility = ["PUBLIC"],
)

# What tk test passes turnkey-test-runner, and the report it writes back:
# src/rust/testcache and src/cmd/turnkey-test-runner
export_file(
    name = "runner-contract",
    src = "runner-contract.json",
    visibility = ["PUBLIC"],
)

# How the dev shell describes the test result cache to tk:
# src/rust/testcache and the buck2-generators flake check
export_file(
    name = "shell-contract",
    src = "shell-contract.json",
    visibility = ["PUBLIC"],
)
