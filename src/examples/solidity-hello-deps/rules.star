# Auto-managed by turnkey. Hash: 446de74d3a15cad3
# Manual sections marked with turnkey:preserve-start/end are not modified.

load("@prelude//solidity:solidity.bzl", "solidity_contract", "solidity_library", "solidity_test")

solidity_library(
    name = "token_lib",
    srcs = ["src/MyToken.sol"],
    deps = [
        # turnkey:auto-start
        "soldeps//:openzeppelin_contracts",
        # turnkey:auto-end
    ],
    visibility = ["PUBLIC"],
)

solidity_contract(
    name = "my_token",
    deps = [
        # turnkey:auto-start
        "soldeps//:openzeppelin_contracts",
        # turnkey:auto-end
    ],
    visibility = ["PUBLIC"],
)

solidity_test(
    name = "token_test",
    srcs = ["test/MyToken.t.sol"],
    deps = [
        # turnkey:auto-start
        "soldeps//:openzeppelin_contracts",
        # turnkey:auto-end
    ],
    visibility = ["PUBLIC"],
)
