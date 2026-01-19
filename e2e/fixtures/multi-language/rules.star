# Go binary
go_binary(
    name = "hello-go",
    srcs = ["main.go"],
    deps = ["godeps//vendor/github.com/google/uuid:uuid"],
    visibility = ["PUBLIC"],
)
