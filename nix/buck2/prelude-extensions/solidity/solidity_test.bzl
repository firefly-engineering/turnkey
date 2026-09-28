# Copyright (c) Firefly Engineering and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

"""Solidity test rule implementation using Foundry's forge."""

load("@prelude//test_caching:test_caching.bzl", "test_caching_kwargs", "test_caching_opted_out")
load(":forge_project.bzl", "forge_command", "forge_config_attrs", "solidity_inputs")
load(":providers.bzl", "SolidityToolchainInfo")

def _fuzz_seed(label: str) -> int:
    """A fixed fuzz seed derived from the target label (32-bit FNV-1a).

    The same target always fuzzes the same cases, so a recorded pass certifies
    exactly the runs that produced it. Different targets get different seeds.
    """
    h = 2166136261
    for c in label.elems():
        h = ((h ^ ord(c)) * 16777619) % 4294967296
    return h

def _solidity_test_impl(ctx: AnalysisContext) -> list[Provider]:
    """Implementation of solidity_test rule.

    Runs `forge test --use $SOLC --offline` in a forge project staged from
    the root foundry.toml and remappings.txt, the test sources, every
    solidity_library dep's sources and the soldeps bundle
    (forge_project.bzl). The tests compile with the root's settings and
    remappings, the ones solidity_library and native forge use.
    """
    forge_args = ["test"]
    if ctx.attrs.verbosity > 0:
        forge_args.append("-" + "v" * ctx.attrs.verbosity)

    # Seed fuzzing from the label unless the target opts out of test result
    # caching, which is how a target asks for stochastic fuzzing.
    if not test_caching_opted_out(ctx.attrs.labels):
        forge_args.extend(["--fuzz-seed", str(_fuzz_seed(str(ctx.label.raw_target())))])
    if ctx.attrs.fork_url:
        forge_args.extend(["--fork-url", ctx.attrs.fork_url])
    if ctx.attrs.match_test:
        forge_args.extend(["--match-test", ctx.attrs.match_test])
    if ctx.attrs.match_contract:
        forge_args.extend(["--match-contract", ctx.attrs.match_contract])
    if ctx.attrs.gas_report:
        forge_args.append("--gas-report")

    test_cmd = forge_command(ctx, solidity_inputs(ctx), forge_args)

    # Create run info for test execution
    run_info = RunInfo(args = test_cmd)

    test_info_kwargs = {
        "type": "solidity",
        "command": [test_cmd],
    }

    return [
        DefaultInfo(),
        # Cacheable: forge runs the toolchain's solc offline, reads its
        # settings from the declared foundry.toml and its dependencies from
        # the declared soldeps bundle, and fuzzes with a seed fixed by the
        # label. A fork test reads chain state over the network, so it
        # always runs.
        ExternalRunnerTestInfo(**(
            test_info_kwargs if ctx.attrs.fork_url else test_caching_kwargs(test_info_kwargs)
        )),
        run_info,
    ]

solidity_test = rule(
    impl = _solidity_test_impl,
    attrs = {
        "srcs": attrs.list(
            attrs.source(),
            default = [],
            doc = "Solidity test source files (.t.sol)",
        ),
        "deps": attrs.list(
            attrs.dep(),
            default = [],
            doc = "Dependencies (solidity_library targets or filegroups from soldeps)",
        ),
        "fork_url": attrs.option(
            attrs.string(),
            default = None,
            doc = "RPC URL for forking mainnet/testnet state",
        ),
        "match_test": attrs.option(
            attrs.string(),
            default = None,
            doc = "Only run tests matching this regex pattern",
        ),
        "match_contract": attrs.option(
            attrs.string(),
            default = None,
            doc = "Only run tests in contracts matching this regex pattern",
        ),
        "gas_report": attrs.bool(
            default = False,
            doc = "Print gas usage report",
        ),
        "labels": attrs.list(
            attrs.string(),
            default = [],
            doc = "Target labels. `no-test-cache` also turns off the fixed fuzz seed.",
        ),
        "verbosity": attrs.int(
            default = 0,
            doc = "Verbosity level (0-5, maps to forge -v flags)",
        ),
        "_solidity_toolchain": attrs.toolchain_dep(
            default = "toolchains//:solc",
            providers = [SolidityToolchainInfo],
        ),
    } | forge_config_attrs(),
    doc = "Runs Solidity tests using Foundry's forge test, with the root foundry.toml's settings.",
)
