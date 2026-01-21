# Auto-managed by turnkey. Hash: 3355f6fa0e604a7d
# Manual sections marked with turnkey:preserve-start/end are not modified.

load("@prelude//:rules.bzl", "go_binary")

go_binary(
    name = "godeps-gen",
    srcs = ["main.go"],
    deps = [
        # turnkey:auto-start
        "//src/go/pkg/godeps:godeps",
        # turnkey:auto-end
    ],
    visibility = ["PUBLIC"],
)
