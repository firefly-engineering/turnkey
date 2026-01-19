# Solidity example - ERC20 Token with OpenZeppelin dependencies

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
