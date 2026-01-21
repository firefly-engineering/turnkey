load("@prelude//:rules.bzl", "python_binary")

python_binary(
    name = "check-rust-edition",
    main = "__main__.py",
    visibility = ["PUBLIC"],
)
