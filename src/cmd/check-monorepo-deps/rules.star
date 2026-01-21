load("@prelude//:rules.bzl", "python_binary")

python_binary(
    name = "check-monorepo-deps",
    main = "__main__.py",
    visibility = ["PUBLIC"],
)
