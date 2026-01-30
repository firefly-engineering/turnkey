load("@prelude//:rules.bzl", "python_binary")

python_binary(
    name = "check-js-test-config",
    main = "__main__.py",
    visibility = ["PUBLIC"],
)
