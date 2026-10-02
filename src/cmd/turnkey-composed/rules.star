# turnkey-composed - FUSE composition daemon for Turnkey
load("@prelude//:rules.bzl", "rust_binary")

rust_binary(
    name = "turnkey-composed",
    srcs = glob(["src/**/*.rs"]),
    edition = "2024",
    linker_flags = select({
        "config//os:linux": [],
        "config//os:macos": ["-L/usr/local/lib"],
    }),
    deps = [
        "//src/rust/composition:composition-full",
        "//src/rust/nix-eval:nix-eval",
        "rustdeps//vendor/anyhow:anyhow",
        "rustdeps//vendor/clap:clap",
        "rustdeps//vendor/ctrlc:ctrlc",
        "rustdeps//vendor/env_logger:env_logger",
        "rustdeps//vendor/libc:libc",
        "rustdeps//vendor/log:log",
        "rustdeps//vendor/serde:serde",
        "rustdeps//vendor/serde_json:serde_json",
    ],
    visibility = ["PUBLIC"],
)
