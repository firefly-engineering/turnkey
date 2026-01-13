# cargo-prune-workspace - prune Cargo.toml workspace members
load("@prelude//:rules.bzl", "go_binary")

go_binary(
    name = "cargo-prune-workspace",
    srcs = ["main.go"],
    deps = [
        "godeps//vendor/github.com/pelletier/go-toml/v2:v2",
    ],
    visibility = ["PUBLIC"],
)
