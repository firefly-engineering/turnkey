# Copyright (c) Firefly Engineering and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

"""Solidity contract rule implementation."""

load(":providers.bzl", "SolidityContractInfo", "SolidityLibraryInfo")

def _solidity_contract_impl(ctx: AnalysisContext) -> list[Provider]:
    """Implementation of solidity_contract rule.

    Extracts specific contract artifacts from a compiled solidity_library.
    """
    # Get the library dependency
    lib_dep = ctx.attrs.lib
    if SolidityLibraryInfo not in lib_dep:
        fail("lib must be a solidity_library target")

    lib_info = lib_dep[SolidityLibraryInfo]
    contract_name = ctx.attrs.contract

    # Declare output artifacts
    abi_file = ctx.actions.declare_output("{}.abi".format(contract_name))
    bytecode_file = ctx.actions.declare_output("{}.bin".format(contract_name))
    deployed_bytecode_file = ctx.actions.declare_output("{}.bin-runtime".format(contract_name))
    metadata_file = ctx.actions.declare_output("{}.metadata.json".format(contract_name))

    # Create extraction script
    extract_script = ctx.actions.declare_output("extract.sh")

    script_content = """#!/usr/bin/env bash
set -euo pipefail

ARTIFACTS_DIR="$1"
CONTRACT_NAME="$2"
ABI_OUT="$3"
BIN_OUT="$4"
BIN_RUNTIME_OUT="$5"
METADATA_OUT="$6"

# Find contract files in artifacts directory
# solc outputs files as: ContractName.abi, ContractName.bin, etc.
# or SourceFile_ContractName.abi if multiple contracts in one file

find_contract_file() {
    local ext="$1"
    local pattern

    # First try exact match
    if [[ -f "$ARTIFACTS_DIR/$CONTRACT_NAME.$ext" ]]; then
        echo "$ARTIFACTS_DIR/$CONTRACT_NAME.$ext"
        return 0
    fi

    # Try with source file prefix (SourceFile_ContractName.ext)
    pattern=$(find "$ARTIFACTS_DIR" -name "*_$CONTRACT_NAME.$ext" -o -name "$CONTRACT_NAME.$ext" 2>/dev/null | head -1)
    if [[ -n "$pattern" && -f "$pattern" ]]; then
        echo "$pattern"
        return 0
    fi

    # Check combined-json output
    if [[ -f "$ARTIFACTS_DIR/combined.json" ]]; then
        echo "COMBINED"
        return 0
    fi

    echo ""
    return 1
}

# Extract from individual files or combined.json
ABI_FILE=$(find_contract_file "abi")
BIN_FILE=$(find_contract_file "bin")
BIN_RUNTIME_FILE=$(find_contract_file "bin-runtime")
METADATA_FILE=$(find_contract_file "metadata.json")

if [[ "$ABI_FILE" == "COMBINED" ]]; then
    # Extract from combined.json
    if command -v jq &> /dev/null; then
        # Use jq if available
        jq -r ".contracts | to_entries[] | select(.key | endswith(\\":$CONTRACT_NAME\\")) | .value.abi" "$ARTIFACTS_DIR/combined.json" > "$ABI_OUT"
        jq -r ".contracts | to_entries[] | select(.key | endswith(\\":$CONTRACT_NAME\\")) | .value.bin" "$ARTIFACTS_DIR/combined.json" > "$BIN_OUT"
        jq -r ".contracts | to_entries[] | select(.key | endswith(\\":$CONTRACT_NAME\\")) | .value[\\"bin-runtime\\"]" "$ARTIFACTS_DIR/combined.json" > "$BIN_RUNTIME_OUT"
        jq -r ".contracts | to_entries[] | select(.key | endswith(\\":$CONTRACT_NAME\\")) | .value.metadata" "$ARTIFACTS_DIR/combined.json" > "$METADATA_OUT"
    else
        # Fallback: use grep/sed (less reliable but works without jq)
        echo "Warning: jq not found, using fallback extraction" >&2
        grep -o '"abi":[^}]*' "$ARTIFACTS_DIR/combined.json" | head -1 | sed 's/"abi"://' > "$ABI_OUT"
        grep -o '"bin":"[^"]*"' "$ARTIFACTS_DIR/combined.json" | head -1 | sed 's/"bin":"//;s/"//' > "$BIN_OUT"
        grep -o '"bin-runtime":"[^"]*"' "$ARTIFACTS_DIR/combined.json" | head -1 | sed 's/"bin-runtime":"//;s/"//' > "$BIN_RUNTIME_OUT"
        echo "{}" > "$METADATA_OUT"
    fi
else
    # Copy from individual files
    if [[ -n "$ABI_FILE" && -f "$ABI_FILE" ]]; then
        cp "$ABI_FILE" "$ABI_OUT"
    else
        echo "[]" > "$ABI_OUT"
        echo "Warning: ABI file not found for $CONTRACT_NAME" >&2
    fi

    if [[ -n "$BIN_FILE" && -f "$BIN_FILE" ]]; then
        cp "$BIN_FILE" "$BIN_OUT"
    else
        echo "" > "$BIN_OUT"
        echo "Warning: Bytecode file not found for $CONTRACT_NAME" >&2
    fi

    if [[ -n "$BIN_RUNTIME_FILE" && -f "$BIN_RUNTIME_FILE" ]]; then
        cp "$BIN_RUNTIME_FILE" "$BIN_RUNTIME_OUT"
    else
        echo "" > "$BIN_RUNTIME_OUT"
    fi

    if [[ -n "$METADATA_FILE" && -f "$METADATA_FILE" ]]; then
        cp "$METADATA_FILE" "$METADATA_OUT"
    else
        echo "{}" > "$METADATA_OUT"
    fi
fi
"""

    ctx.actions.write(
        extract_script,
        script_content,
        is_executable = True,
    )

    # Build extraction command
    extract_cmd = cmd_args(extract_script)
    extract_cmd.add(lib_info.output_dir)
    extract_cmd.add(contract_name)
    extract_cmd.add(abi_file.as_output())
    extract_cmd.add(bytecode_file.as_output())
    extract_cmd.add(deployed_bytecode_file.as_output())
    extract_cmd.add(metadata_file.as_output())

    ctx.actions.run(
        cmd_args(extract_cmd, hidden = [lib_info.output_dir]),
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
    },
    doc = "Extracts a specific contract's artifacts (ABI, bytecode) from a compiled solidity_library.",
)
