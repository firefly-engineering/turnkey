# Copyright (c) Firefly Engineering and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

"""Solidity contract rule implementation."""

load(":providers.bzl", "SolidityContractInfo", "SolidityLibraryInfo", "SolidityToolchainInfo")

def _solidity_contract_impl(ctx: AnalysisContext) -> list[Provider]:
    """Implementation of solidity_contract rule.

    Extracts one contract's artifacts from a solidity_library's forge
    artifacts directory.
    """
    toolchain = ctx.attrs._solidity_toolchain[SolidityToolchainInfo]
    if not toolchain.jq:
        fail("Solidity toolchain does not have jq configured. Required for solidity_contract.")

    lib_info = ctx.attrs.lib[SolidityLibraryInfo]
    contract_name = ctx.attrs.contract

    # Declare output artifacts
    abi_file = ctx.actions.declare_output("{}.abi".format(contract_name))
    bytecode_file = ctx.actions.declare_output("{}.bin".format(contract_name))
    deployed_bytecode_file = ctx.actions.declare_output("{}.bin-runtime".format(contract_name))
    metadata_file = ctx.actions.declare_output("{}.metadata.json".format(contract_name))

    # forge writes one <File>.sol/<Contract>.json per contract it compiled,
    # the library's imports included, so a name alone can be ambiguous. The
    # artifact to extract is the one whose compilation target is this
    # contract in one of the library's own sources (named relative to the
    # repository root, where foundry.toml sits). The outputs are what
    # solc's --abi, --bin, --bin-runtime and --metadata write: compact JSON,
    # unprefixed hex and the raw metadata string.
    extract_script = ctx.actions.write(
        "extract.sh",
        """#!/usr/bin/env bash
set -euo pipefail

JQ="$1"
ARTIFACTS_DIR="$2"
CONTRACT_NAME="$3"
ABI_OUT="$4"
BIN_OUT="$5"
BIN_RUNTIME_OUT="$6"
METADATA_OUT="$7"
shift 7
# The rest: the library's sources, relative to the repository root
SRCS_JSON=$(printf '%s\\n' "$@" | "$JQ" -R . | "$JQ" -s .)

ARTIFACT=""
while IFS= read -r candidate; do
    if "$JQ" -e --arg name "$CONTRACT_NAME" --argjson srcs "$SRCS_JSON" \\
        '.metadata.settings.compilationTarget | to_entries | any(.value == $name and (.key | IN($srcs[])))' \\
        "$candidate" > /dev/null; then
        if [[ -n "$ARTIFACT" ]]; then
            echo "error: more than one of the library's sources defines a contract named $CONTRACT_NAME; split the library so each solidity_library defines it once" >&2
            exit 1
        fi
        ARTIFACT="$candidate"
    fi
done < <(find "$ARTIFACTS_DIR" -name "$CONTRACT_NAME.json" -not -path "*/build-info/*")

if [[ -z "$ARTIFACT" ]]; then
    echo "error: no contract $CONTRACT_NAME in the library's sources: $*" >&2
    exit 1
fi

"$JQ" -cj '.abi' "$ARTIFACT" > "$ABI_OUT"
"$JQ" -j '.bytecode.object | ltrimstr("0x")' "$ARTIFACT" > "$BIN_OUT"
"$JQ" -j '.deployedBytecode.object | ltrimstr("0x")' "$ARTIFACT" > "$BIN_RUNTIME_OUT"
"$JQ" -ej '.rawMetadata' "$ARTIFACT" > "$METADATA_OUT"
""",
        is_executable = True,
    )

    ctx.actions.run(
        cmd_args(
            extract_script,
            toolchain.jq.args,
            lib_info.output_dir,
            contract_name,
            abi_file.as_output(),
            bytecode_file.as_output(),
            deployed_bytecode_file.as_output(),
            metadata_file.as_output(),
            lib_info.srcs.keys(),
        ),
        category = "solidity_extract",
        identifier = ctx.label.name,
    )

    contract_info = SolidityContractInfo(
        contract_name = contract_name,
        abi = abi_file,
        bytecode = bytecode_file,
        deployed_bytecode = deployed_bytecode_file,
        metadata = metadata_file,
    )

    return [
        DefaultInfo(default_outputs = [abi_file, bytecode_file]),
        contract_info,
    ]

solidity_contract = rule(
    impl = _solidity_contract_impl,
    attrs = {
        "contract": attrs.string(
            doc = "Name of the contract to extract from compiled sources",
        ),
        "lib": attrs.dep(
            providers = [SolidityLibraryInfo],
            doc = "The solidity_library target containing the contract",
        ),
        "_solidity_toolchain": attrs.toolchain_dep(
            default = "toolchains//:solc",
            providers = [SolidityToolchainInfo],
        ),
    },
    doc = "Extracts a specific contract's artifacts (ABI, bytecode) from a compiled solidity_library.",
)
