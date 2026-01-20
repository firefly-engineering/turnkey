# Example Go binary with external dependencies including assembly-based packages
# This target should build with both:
#   go build .              (native Go toolchain using go.mod)
#   buck2 build :go-hello-deps (Buck2 using Nix-managed deps cell)

# go_binary is auto-loaded from the prelude
go_binary(
    name = "go-hello-deps",
    srcs = ["main.go"],
    deps = [
        "godeps//vendor/github.com/google/uuid:uuid",
        "godeps//vendor/golang.org/x/sys/cpu:cpu",
    ],
    visibility = ["PUBLIC"],
)
