# Auto-managed by turnkey. Hash: e647113dbd838d4a
# Manual sections marked with turnkey:preserve-start/end are not modified.

go_binary(
    name = "go-hello-deps",
    srcs = ["main.go"],
    deps = [
        # turnkey:auto-start
        "godeps//vendor/github.com/google/uuid:uuid",
        "godeps//vendor/golang.org/x/sys/cpu:cpu",
        # turnkey:auto-end
    ],
    visibility = ["PUBLIC"],
)
