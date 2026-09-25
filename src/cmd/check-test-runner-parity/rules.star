load("@prelude//:rules.bzl", "python_binary")

python_binary(
    name = "check-test-runner-parity",
    main = "__main__.py",
    visibility = ["PUBLIC"],
)
