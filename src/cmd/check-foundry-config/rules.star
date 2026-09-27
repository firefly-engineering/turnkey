load("@prelude//:rules.bzl", "python_binary", "python_library", "python_test")

python_library(
    name = "lib",
    # Under the shared turnkey.* namespace, as turnkey.check_foundry_config;
    # the on-disk layout matches, so the pre-commit hook, which runs
    # __main__.py with python directly, imports it the same way
    srcs = ["turnkey/check_foundry_config.py"],
    base_module = "",
)

python_binary(
    name = "check-foundry-config",
    main = "__main__.py",
    base_module = "",
    visibility = ["PUBLIC"],
    deps = [":lib"],
)

python_test(
    name = "test",
    srcs = ["tests/test_check_foundry_config.py"],
    base_module = "tests",
    deps = [":lib"],
)
