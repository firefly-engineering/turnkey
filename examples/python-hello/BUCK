load("@prelude//:rules.bzl", "python_binary", "python_test")

python_binary(
    name = "python-hello",
    main = "hello.py",
    visibility = ["PUBLIC"],
)

python_test(
    name = "python-hello-test",
    srcs = ["test_hello.py"],
    visibility = ["PUBLIC"],
)
