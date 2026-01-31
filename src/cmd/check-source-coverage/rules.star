load("@prelude//:rules.bzl", "python_binary")

python_binary(
    name = "check-source-coverage",
    main = "__main__.py",
    visibility = ["PUBLIC"],
)
