# composition - CompositionBackend trait for FUSE and symlink backends
load("@prelude//:rules.bzl", "rust_library", "rust_test")

# Detect platform for conditional FUSE backend selection
_IS_MACOS = host_info().os.is_macos

# Common deps shared by all targets
_COMMON_DEPS = [
    "//src/rust/nix-eval:nix-eval",
    "rustdeps//vendor/dirs:dirs",
    "rustdeps//vendor/log:log",
    "rustdeps//vendor/serde:serde",
    "rustdeps//vendor/serde_json:serde_json",
    "rustdeps//vendor/thiserror:thiserror",
    "rustdeps//vendor/toml:toml",
]

# Base library without optional features
rust_library(
    name = "composition",
    srcs = glob(["src/**/*.rs"]),
    edition = "2024",
    deps = _COMMON_DEPS,
    visibility = ["PUBLIC"],
)

# Full-featured library with FUSE and watcher support
# - Linux: uses fuser crate (feature="fuse")
# - macOS: uses direct libfuse3 FFI (feature="fuse-t")
rust_library(
    name = "composition-full",
    crate = "composition",  # Keep the original crate name for imports
    srcs = glob(["src/**/*.rs"]),
    edition = "2024",
    rustc_flags = [
        "--cfg", "feature=\"fuse-t\"" if _IS_MACOS else "feature=\"fuse\"",
        "--cfg", "feature=\"watcher\"",
    ],
    exported_linker_flags = [
        "-L/usr/local/lib",
        "-lfuse3",
    ] if _IS_MACOS else [],
    deps = _COMMON_DEPS + [
        "rustdeps//vendor/libc:libc",
        # Use versioned target to match notify-debouncer-mini's dependency
        "rustdeps//vendor/notify@8.2.0:notify",
        "rustdeps//vendor/notify-debouncer-mini:notify-debouncer-mini",
    ] + ([] if _IS_MACOS else [
        "rustdeps//vendor/fuser:fuser",
    ]),
    visibility = ["PUBLIC"],
)

rust_test(
    name = "composition-test",
    srcs = glob(["src/**/*.rs"]),
    edition = "2024",
    deps = _COMMON_DEPS + [
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
