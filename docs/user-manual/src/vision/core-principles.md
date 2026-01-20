# Core Principles

Turnkey is built on four core principles that guide every design decision. These principles often exist in tension with each other, and Turnkey's value lies in finding the right balance.

## 1. Native Tool Compatibility

**Your existing commands should just work.**

When you run `go build`, it should build your Go code. When you run `cargo test`, it should test your Rust code. LSP servers should provide autocomplete. IDEs should find definitions. This isn't a compromise - it's a requirement.

### How It Works

Turnkey provides transparent wrappers (`tw`) around native tools that:

- Pass through all commands unchanged by default
- Watch for dependency file changes (go.mod, Cargo.lock, etc.)
- Automatically regenerate build system dependency cells when needed
- Never block or modify the developer's primary workflow

```bash
# The 'tw' wrapper is transparent
tw go get github.com/foo/bar    # Works exactly like 'go get'
                                 # But also updates build system deps if go.mod changed

# Or use 'go' directly - it still works
go build ./...                   # Normal Go build, no Buck2 involved
```

### Why This Matters

- **Zero learning curve** for basic workflows
- **IDE integrations continue working** - gopls, rust-analyzer, pyright all function normally
- **Existing scripts and CI remain valid** - no migration required
- **Developers stay in their comfort zone** while infrastructure improves beneath them

## 2. Monorepo Benefits Without Monorepo Storage

**Get unified versioning without storing the world in your repository.**

Traditional monorepos store all code in one repository, enabling atomic changes and unified versioning. But this comes with costs:

- Massive repository size
- Complex code ownership
- Slow git operations
- Storage of third-party code

Turnkey provides the *benefits* of a monorepo without these costs through **virtual cells**.

### How It Works

```
your-repo/
├── src/                    # Your source code
├── go.mod                  # Normal Go module
├── Cargo.toml              # Normal Cargo workspace
└── .turnkey/
    ├── godeps/            # Virtual cell: Go dependencies
    ├── rustdeps/          # Virtual cell: Rust dependencies
    └── prelude/           # Virtual cell: Build system prelude
```

The `.turnkey/` directory contains **cells** - the build system's unit of code organization. These cells are:

- Generated from your lock files (go.sum, Cargo.lock, etc.)
- Deterministically reproducible via Nix
- Treated as source code by the build system (enabling caching and incrementality)
- Never committed to git (they're derived data)

### The Result

- **Atomic changes** across your code and its dependencies
- **Unified versioning** - one lock file controls one version
- **Hermetic builds** - Nix ensures reproducibility
- **Fast git operations** - repository stays small

## 3. Incremental Build and Test

**Only rebuild and retest what actually changed.**

Modern CI/CD often wastes enormous resources rebuilding unchanged code. A small typo fix shouldn't trigger a full rebuild of the entire project.

### How It Works

The incremental build system tracks fine-grained dependencies between:

- Source files
- Build rules
- Test targets
- Generated artifacts

When a file changes, the build system determines the minimal set of actions needed:

```bash
# Edit a single Go file
vim pkg/utils/helper.go

# The build system only rebuilds affected targets
tk build //...    # Rebuilds only what depends on helper.go
tk test //...     # Runs only tests that might be affected
```

Combined with **remote caching**, this means:

- CI builds are fast because most artifacts are cached
- Local builds benefit from CI's cached artifacts
- AI agents can iterate quickly with sub-second feedback

### Why This Matters for AI

AI coding assistants (like Claude Code) benefit enormously from fast builds:

- Quick iterations mean more experiments per session
- Fast test feedback enables test-driven development
- Immediate error messages allow rapid course correction

Turnkey's incremental builds make AI-assisted development practical at scale.

## 4. Continuum of Experience

**Scale from prototype to enterprise without rewrites.**

Software projects exist on a spectrum:

| Stage | Needs |
|-------|-------|
| Prototype | Quick iteration, minimal ceremony |
| Startup | Fast CI, some reproducibility |
| Growth | Caching, parallelism, reliability |
| Enterprise | Compliance, governance, audit trails |

Traditional build systems force you to choose: simple but limited, or powerful but complex. Turnkey provides a **continuum**:

### Level 1: Just Use Native Tools

```bash
go build ./...
cargo test
pytest
```

No turnkey involvement. Everything works normally.

### Level 2: Add Hermetic Tooling

```toml
# toolchain.toml
[toolchains]
go = {}
rust = {}
python = {}
```

Now your tools are versioned by Nix. "Works on my machine" disappears.

### Level 3: Enable Incremental Builds

```bash
tk build //...
tk test //...
```

Get incremental builds and caching. CI becomes faster.

### Level 4: Add Remote Caching

Share build artifacts across developers and CI. Builds that took 10 minutes now take 30 seconds.

### Level 5: Remote Execution

Distribute builds across a cluster. Massive parallelism. Enterprise scale.

### Why This Matters

You don't have to adopt everything at once. Start with Level 1 or 2. Move to higher levels as your needs grow. The underlying infrastructure supports your growth without requiring rewrites.

---

These four principles - native compatibility, virtual monorepo, incremental builds, and progressive adoption - form the foundation of Turnkey's design. In the next chapter, we'll see how the architecture implements these principles in practice.

> **Note:** Turnkey currently uses Buck2 as its incremental build system. The architecture is designed to potentially support other build systems like Bazel in the future.
