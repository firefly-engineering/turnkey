# Auto-managed by turnkey. Hash: e3b0c44298fc1c14
# Manual sections marked with turnkey:preserve-start/end are not modified.

load("@prelude//:rules.bzl", "python_binary", "python_test")

python_binary(
    name = "python-hello",
    visibility = ["PUBLIC"],
)

python_test(
    name = "python-hello-test",
    srcs = ["test_hello.py"],
    visibility = ["PUBLIC"],
)
