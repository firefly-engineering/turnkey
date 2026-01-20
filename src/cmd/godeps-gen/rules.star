# godeps-gen - generate go-deps.toml from go.mod/go.sum
load("@prelude//:rules.bzl", "go_binary")

go_binary(
    name = "godeps-gen",
    srcs = ["main.go"],
    deps = [
        "//src/go/pkg/godeps:godeps",
    ],
    visibility = ["PUBLIC"],
)
