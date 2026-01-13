# Rust library
rust_library(
    name = "greeting",
    srcs = ["src/lib.rs"],
    edition = "2021",
    deps = [
        "rustdeps//vendor/serde:serde",
        "rustdeps//vendor/serde_json:serde_json",
    ],
    visibility = ["PUBLIC"],
)

rust_test(
    name = "greeting-test",
    srcs = ["src/lib.rs"],
    edition = "2021",
    deps = [
        "rustdeps//vendor/serde:serde",
        "rustdeps//vendor/serde_json:serde_json",
    ],
)
