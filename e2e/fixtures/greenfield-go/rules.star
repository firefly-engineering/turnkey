# Auto-managed by turnkey. Hash: e53f730ccb631bf4
# Manual sections marked with turnkey:preserve-start/end are not modified.

go_binary(
    name = "hello",
    srcs = ["main.go"],
    deps = [
        # turnkey:auto-start
        "godeps//vendor/github.com/google/uuid:uuid",
        # turnkey:auto-end
    ],
    visibility = ["PUBLIC"],
)
