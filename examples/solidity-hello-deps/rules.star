# Solidity example - ERC20 Token with OpenZeppelin dependencies
#
# This example demonstrates Solidity dependency management with external packages.
# To build this example, you need to:
#
# 1. Enable Solidity deps in your flake.nix:
#    turnkey.buck2.solidity = {
#      enable = true;
#      depsFile = ./solidity-deps.toml;
#    };
#
# 2. Ensure solidity-deps.toml has been generated from foundry.toml:
#    soldeps-gen --foundry-toml foundry.toml --output solidity-deps.toml
#
# 3. Re-enter your devenv shell to regenerate the soldeps cell

load("@prelude//solidity:solidity.bzl", "solidity_contract", "solidity_library", "solidity_test")

# Compile the MyToken contract with OpenZeppelin dependencies
solidity_library(
    name = "token_lib",
    srcs = ["src/MyToken.sol"],
    deps = [
        "//soldeps:openzeppelin_contracts",
    ],
    remappings = {
        "@openzeppelin/contracts/": "//soldeps:openzeppelin_contracts/contracts/",
    },
    optimizer = True,
    optimizer_runs = 200,
    visibility = ["PUBLIC"],
)

# Extract the MyToken contract artifacts
solidity_contract(
    name = "my_token",
    contract = "MyToken",
    lib = ":token_lib",
    visibility = ["PUBLIC"],
)

# Run Foundry tests with forge-std for assertions
solidity_test(
    name = "token_test",
    srcs = ["test/MyToken.t.sol"],
    deps = [
        ":token_lib",
        "//soldeps:forge_std",
    ],
    remappings = {
        "@openzeppelin/contracts/": "//soldeps:openzeppelin_contracts/contracts/",
        "forge-std/": "//soldeps:forge_std/src/",
    },
    fuzz_runs = 256,
    visibility = ["PUBLIC"],
)
