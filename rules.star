# Repository-root targets

# The repository's Foundry configuration and its generated remappings, as
# inputs of the Solidity rules: the solidity_library and solidity_test macros
# default their foundry_toml and remappings_txt attrs to these (named in the
# generated .buckconfig's [solidity]), and stage them at the repository root,
# so forge reads what native forge reads.
export_file(
    name = "foundry.toml",
    visibility = ["PUBLIC"],
)

export_file(
    name = "remappings.txt",
    visibility = ["PUBLIC"],
)
