# Auto-managed by turnkey. Hash: 8338370964de6cbf
# Manual sections marked with turnkey:preserve-start/end are not modified.

load("@prelude//:rules.bzl", "go_binary")

go_binary(
    name = "cargo-prune-workspace",
    srcs = ["main.go"],
    deps = [
        # turnkey:auto-start
        "godeps//vendor/github.com/pelletier/go-toml/v2:v2",
        # turnkey:auto-end
    ],
    visibility = ["PUBLIC"],
)
