load("@prelude//:rules.bzl", "python_binary", "python_test")

python_binary(
    name = "python-requests",
    main = "main.py",
    deps = [
        "pydeps//vendor/certifi:certifi",
        "pydeps//vendor/requests:requests",
    ],
    visibility = ["PUBLIC"],
)

python_test(
    name = "python-requests-test",
    srcs = ["test_requests.py"],
    deps = [
        "pydeps//vendor/certifi:certifi",
        "pydeps//vendor/requests:requests",
    ],
    visibility = ["PUBLIC"],
)
