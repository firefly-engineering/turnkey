load("@prelude//:rules.bzl", "python_binary", "rust_test")

python_binary(
    name = "check-test-caching",
    main = "__main__.py",
    visibility = ["PUBLIC"],
)

# The check passes -c turnkey.check_test_caching_re_profile=true to give
# re-profile-test a remote_execution profile. Otherwise it has none, so it
# still runs locally under `buck2 test //...`.
config_setting(
    name = "re-profile",
    values = {"turnkey.check_test_caching_re_profile": "true"},
)

rust_test(
    name = "re-profile-test",
    srcs = ["fixture.rs"],
    crate_root = "fixture.rs",
    remote_execution = select({
        ":re-profile": {
            "capabilities": {"platform": "check-test-caching"},
            "use_case": "check-test-caching",
        },
        "DEFAULT": None,
    }),
)

rust_test(
    name = "re-disabled-test",
    srcs = ["fixture.rs"],
    crate_root = "fixture.rs",
    remote_execution = "disabled",
)
