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

## Historical Reference

Research and design documents kept for historical reference:

- `design-use-turnkey.md` - Design document for use_turnkey direnv function
- `fuse-composition-research.md` - Research on FUSE-based repository composition
- `designs/` - Design documents for planned features
- `plans/` - Implementation plans
