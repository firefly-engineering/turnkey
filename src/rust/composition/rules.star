# composition - CompositionBackend trait for FUSE and symlink backends
load("@prelude//:rules.bzl", "rust_library", "rust_test")

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

# Full-featured library with fuse and watcher support
rust_library(
    name = "composition-full",
    crate = "composition",  # Keep the original crate name for imports
    srcs = glob(["src/**/*.rs"]),
    edition = "2024",
    rustc_flags = [
        "--cfg", "feature=\"fuse\"",
        "--cfg", "feature=\"watcher\"",
    ],
    deps = [
        "rustdeps//vendor/fuser:fuser",
        "rustdeps//vendor/libc:libc",
        "rustdeps//vendor/log:log",
        # Use versioned target to match notify-debouncer-mini's dependency
        "rustdeps//vendor/notify@8.2.0:notify",
        "rustdeps//vendor/notify-debouncer-mini:notify-debouncer-mini",
        "rustdeps//vendor/thiserror:thiserror",
    ],
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
