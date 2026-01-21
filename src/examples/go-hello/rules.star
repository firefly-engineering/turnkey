# Auto-managed by turnkey. Hash: e3b0c44298fc1c14
# Manual sections marked with turnkey:preserve-start/end are not modified.

go_binary(
    name = "go-hello",
    srcs = ["main.go"],
    visibility = ["PUBLIC"],
)
