# Turnkey

Turnkey is a polyglot development environment and build system designed for seamless integration of Go, Rust, and Python projects. It leverages **Nix** for hermetic environments and **Buck2** for fast, incremental builds.

## Getting Started

### Prerequisites

1.  **Nix**: Install Nix (with flake support enabled).
2.  **Direnv**: Install `direnv` and hook it into your shell.

### Setup

1.  Clone the repository.
2.  Enter the directory and allow direnv to load the environment:
    ```bash
    direnv allow
    ```
    This will automatically download and configure all necessary tools (Go, Rust, Python, Buck2, etc.) in a hermetic environment.

3.  Initialize the issue tracker:
    ```bash
    bd onboard
    ```

## Tools

Turnkey provides two main CLI tools to streamline development:

### `tk` - The Build Wrapper

`tk` is a smart wrapper around **Buck2**. It ensures that your build graph and generated files (like `rules.star` and dependency cells) are always up-to-date before running any build commands.

**Usage:**
- `tk build //...`: Build all targets (syncs automatically).
- `tk test //...`: Run all tests.
- `tk sync`: Explicitly sync generated files.
- `tk check`: Check for staleness (useful for CI).

### `tw` - The Tool Wrapper

`tw` is a wrapper for native language tools (`go`, `cargo`, `uv`). It monitors dependency files (like `go.mod`, `Cargo.toml`) and automatically triggers a sync if they change after running a command. This keeps your Nix files and Buck2 rules in sync with your native package managers.

**Usage:**
- `tw go get github.com/pkg/errors`: Add a Go dependency and sync.
- `tw cargo add serde`: Add a Rust dependency and sync.
- `tw uv add requests`: Add a Python dependency and sync.

## Workflow

This project uses **beads** (`bd`) for distributed issue tracking.

- **Find work**: `bd ready`
- **Claim work**: `bd update <id> --status in_progress`
- **Submit work**: `bd close <id>` then `git push`

See `AGENTS.md` for detailed workflow instructions.
