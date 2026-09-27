# composition - CompositionBackend trait for FUSE and symlink backends
load("@prelude//:rules.bzl", "rust_library", "rust_test")

# Base library without optional features
rust_library(
    name = "composition",
    srcs = glob(["src/**/*.rs"]),
    edition = "2024",
    deps = [
        "//src/rust/nix-eval:nix-eval",
        "rustdeps//vendor/dirs:dirs",
        "rustdeps//vendor/log:log",
        "rustdeps//vendor/serde:serde",
        "rustdeps//vendor/serde_json:serde_json",
        "rustdeps//vendor/thiserror:thiserror",
        "rustdeps//vendor/toml:toml",
    ],
    visibility = ["PUBLIC"],
)

# Full-featured library with FUSE and watcher support
# - Linux: uses fuser crate (feature="fuse")
# - macOS: uses direct libfuse3 FFI (feature="fuse-t")
# Sync writes the features and deps these Cargo features expand to.
rust_library(
    name = "composition-full",
    crate = "composition",  # Keep the original crate name for imports
    srcs = glob(["src/**/*.rs"]),
    edition = "2024",
    cargo_features = ["watcher"] + select({
        "config//os:linux": ["fuse"],
        "config//os:macos": ["fuse-t"],
    }),
    exported_linker_flags = select({
        "config//os:linux": [],
        "config//os:macos": [
            "-L/usr/local/lib",
            "-lfuse3",
        ],
    }),
    # notify is pinned to the version notify-debouncer-mini depends on
    deps = [
        "//src/rust/nix-eval:nix-eval",
        "rustdeps//vendor/dirs:dirs",
        "rustdeps//vendor/libc:libc",
        "rustdeps//vendor/log:log",
        "rustdeps//vendor/notify-debouncer-mini:notify-debouncer-mini",
        "rustdeps//vendor/notify@8.2.0:notify",
        "rustdeps//vendor/serde:serde",
        "rustdeps//vendor/serde_json:serde_json",
        "rustdeps//vendor/thiserror:thiserror",
        "rustdeps//vendor/toml:toml",
    ] + select({
        "config//os:linux": ["rustdeps//vendor/fuser:fuser"],
        "config//os:macos": [],
    }),
    visibility = ["PUBLIC"],
    features = ["watcher"] + select({
        "config//os:linux": ["fuse"],
        "config//os:macos": ["fuse-t"],
    }),
)

rust_test(
    name = "composition-test",
    srcs = glob(["src/**/*.rs"]),
    edition = "2024",
    deps = [
        "//src/rust/nix-eval:nix-eval",
        "rustdeps//vendor/dirs:dirs",
        "rustdeps//vendor/log:log",
        "rustdeps//vendor/serde:serde",
        "rustdeps//vendor/serde_json:serde_json",
        "rustdeps//vendor/thiserror:thiserror",
        "rustdeps//vendor/toml:toml",
        "rustdeps//vendor/tempfile:tempfile",
    ],
)

# Integration tests
rust_test(
    name = "integration-tests",
    srcs = glob(["tests/**/*.rs"]),
    edition = "2024",
    deps = [
        ":composition",
        "rustdeps//vendor/tempfile:tempfile",
    ],
)
