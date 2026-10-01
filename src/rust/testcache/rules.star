# testcache - tk test's reuse policy, and the local test result cache it
# runs
load("@prelude//:rules.bzl", "rust_library", "rust_test")

rust_library(
    name = "testcache",
    srcs = glob(["src/**/*.rs"]),
    edition = "2024",
    deps = [
        "//src/rust/buck2-args:buck2-args",
        "//src/rust/gostd:gostd",
        "rustdeps//vendor/libc:libc",
        "rustdeps//vendor/rustls:rustls",
        "rustdeps//vendor/rustls-native-certs:rustls-native-certs",
        "rustdeps//vendor/serde:serde",
        "rustdeps//vendor/serde_json:serde_json",
        "rustdeps//vendor/tempfile:tempfile",
    ],
    visibility = ["PUBLIC"],
)

rust_test(
    name = "testcache-test",
    # The TLS test embeds a certificate no system trusts
    srcs = glob(["src/**/*.rs", "testdata/*.pem"]),
    edition = "2024",
    deps = [
        "//src/rust/buck2-args:buck2-args",
        "//src/rust/gostd:gostd",
        "rustdeps//vendor/libc:libc",
        "rustdeps//vendor/rustls:rustls",
        "rustdeps//vendor/rustls-native-certs:rustls-native-certs",
        "rustdeps//vendor/serde:serde",
        "rustdeps//vendor/serde_json:serde_json",
        "rustdeps//vendor/tempfile:tempfile",
    ],
    # The contracts shared with the runner and the shell (src/testdata),
    # embedded from where testdata/ links to them
    mapped_srcs = {
        "//src/testdata:runner-contract": "testdata/runner-contract.json",
        "//src/testdata:shell-contract": "testdata/shell-contract.json",
    },
)
