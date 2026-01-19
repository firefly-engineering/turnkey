# Turnkey Documentation

## Documentation Structure

Turnkey documentation is organized into two mdbook manuals:

- **[User Manual](./user-manual/)** - For users of Turnkey
  - Getting started guides
  - Configuration reference
  - Workflows and language support
  - CLI reference

- **[Developer Manual](./developer-manual/)** - For Turnkey contributors
  - Architecture documentation
  - Nix module internals
  - Extending Turnkey
  - Contributing guidelines

## Serving Documentation Locally

```bash
# Serve user manual
tk run //docs/user-manual

# Serve developer manual
tk run //docs/developer-manual

# Build both
tk build //docs:all
```

## Legacy Documents

The following documents in this directory have been migrated to the manuals and are kept for reference:

| Document | Migrated To |
|----------|-------------|
| `dependency-management.md` | User Manual: workflows/dependencies.md |
| `project_initialization.md` | User Manual: getting-started/project-setup.md |
| `tk.md` | User Manual: reference/cli.md |
| `native-tool-wrappers.md` | User Manual: reference/cli.md |
| `buck2_cell_resolution.md` | Developer Manual: architecture/buck2.md |
| `buck2_prelude_customization.md` | Developer Manual: extending/prelude-extensions.md |
| `buck2-prelude-compatibility.md` | Developer Manual: extending/prelude-extensions.md |
| `go-deps-cell-design.md` | Developer Manual: modules/buck2-cells.md |
| `rust-dependency-handling.md` | Developer Manual: extending/dependency-generators.md |

The following are research/design documents kept as historical reference:

- `design-use-turnkey.md` - Design document for use_turnkey direnv function
- `fuse-composition-research.md` - Research on FUSE-based repository composition
