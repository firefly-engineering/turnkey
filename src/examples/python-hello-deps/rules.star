# Python example with external package dependencies via pydeps cell
#
# Dependencies are declared in python-deps.toml and vendored by Nix.
# The pydeps cell is auto-generated and symlinked to .turnkey/pydeps

load("@prelude//:rules.bzl", "python_binary")

python_binary(
    name = "python-hello-deps",
    main = "hello.py",
    deps = [
        "pydeps//vendor/six:six",
    ],
    visibility = ["PUBLIC"],
)
