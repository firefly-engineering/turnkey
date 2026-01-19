# Example Go binary target for E2E testing
go_binary(
    name = "hello",
    srcs = ["main.go"],
    deps = ["godeps//vendor/github.com/google/uuid:uuid"],
    visibility = ["PUBLIC"],
)
