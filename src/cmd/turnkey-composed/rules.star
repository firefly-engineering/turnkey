# turnkey-composed - FUSE composition daemon for Turnkey
load("@prelude//:rules.bzl", "rust_binary")

_IS_MACOS = host_info().os.is_macos

rust_binary(
    name = "turnkey-composed",
    srcs = glob(["src/**/*.rs"]),
    edition = "2024",
    linker_flags = [
        "-L/usr/local/lib",
    ] if _IS_MACOS else [],
    deps = [
        "//src/rust/composition:composition-full",
        "rustdeps//vendor/anyhow:anyhow",
        "rustdeps//vendor/clap:clap",
        "rustdeps//vendor/ctrlc:ctrlc",
        "rustdeps//vendor/env_logger:env_logger",
        "rustdeps//vendor/log:log",
        "rustdeps//vendor/serde:serde",
        "rustdeps//vendor/serde_json:serde_json",
    ],
    visibility = ["PUBLIC"],
)
