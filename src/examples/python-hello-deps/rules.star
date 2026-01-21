# Auto-managed by turnkey. Hash: f388892cc619788f
# Manual sections marked with turnkey:preserve-start/end are not modified.

load("@prelude//:rules.bzl", "python_binary")

python_binary(
    name = "python-hello-deps",
    main = "hello.py",
    deps = [
        "pydeps//vendor/six:six",
    ],
    visibility = ["PUBLIC"],
)
