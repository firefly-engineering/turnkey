# pydeps-gen - generate python-deps.toml from pyproject.toml
load("@prelude//:rules.bzl", "rust_binary", "rust_test")

rust_binary(
    name = "pydeps-gen",
    srcs = glob(["src/**/*.rs"]),
    edition = "2024",
    deps = [
        # turnkey:auto-start
        "//src/rust/deps-gen-kit:deps-gen-kit",
        "//src/rust/pep508:pep508",
        "rustdeps//vendor/anyhow:anyhow",
        "rustdeps//vendor/clap:clap",
        "rustdeps//vendor/serde:serde",
        "rustdeps//vendor/serde_json:serde_json",
        "rustdeps//vendor/toml:toml",
        "rustdeps//vendor/ureq:ureq",
        # turnkey:auto-end
        # turnkey:preserve-start
        "rustdeps//vendor/ring@0.17.14:ring_core_0_17_14__",
        # turnkey:preserve-end
    ],
    visibility = ["PUBLIC"],
)

rust_test(
    name = "pydeps-gen-test",
    srcs = glob(["src/**/*.rs"]),
    crate_root = "src/main.rs",
    edition = "2024",
    deps = [
        # turnkey:auto-start
        "//src/rust/deps-gen-kit:deps-gen-kit",
        "//src/rust/pep508:pep508",
        "rustdeps//vendor/anyhow:anyhow",
        "rustdeps//vendor/clap:clap",
        "rustdeps//vendor/serde:serde",
        "rustdeps//vendor/serde_json:serde_json",
        "rustdeps//vendor/toml:toml",
        "rustdeps//vendor/ureq:ureq",
        # turnkey:auto-end
        # turnkey:preserve-start
        "rustdeps//vendor/ring@0.17.14:ring_core_0_17_14__",
        # turnkey:preserve-end
    ],
)
