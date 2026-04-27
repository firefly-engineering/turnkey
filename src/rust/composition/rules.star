# composition - CompositionBackend trait for FUSE and symlink backends
load("@prelude//:rules.bzl", "rust_library", "rust_test")

# Detect platform for conditional FUSE backend selection
_IS_MACOS = host_info().os.is_macos

# Base library without optional features
rust_library(
    name = "composition",
    srcs = glob(["src/**/*.rs"]),
    edition = "2024",
    deps = [
        "rustdeps//vendor/log:log",
        "rustdeps//vendor/thiserror:thiserror",
    ],
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
    deps = [
        "rustdeps//vendor/libc:libc",
        "rustdeps//vendor/log:log",
        # Use versioned target to match notify-debouncer-mini's dependency
        "rustdeps//vendor/notify@8.2.0:notify",
        "rustdeps//vendor/notify-debouncer-mini:notify-debouncer-mini",
        "rustdeps//vendor/thiserror:thiserror",
    ] + ([] if _IS_MACOS else [
        "rustdeps//vendor/fuser:fuser",
    ]),
    visibility = ["PUBLIC"],
)

rust_test(
    name = "composition-test",
    srcs = glob(["src/**/*.rs"]),
    edition = "2024",
    deps = [
        "rustdeps//vendor/log:log",
        "rustdeps//vendor/tempfile:tempfile",
        "rustdeps//vendor/thiserror:thiserror",
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
