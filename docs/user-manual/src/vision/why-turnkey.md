# Why Turnkey

Modern software development faces a fundamental tension: we want the **simplicity of working with familiar tools** while also needing the **reproducibility and scalability of sophisticated build systems**.

Turnkey bridges this gap.

## The Problem

Consider a typical development scenario. You have a project that uses Go, some Rust libraries, a Python testing framework, and TypeScript for the frontend. Each language has its own:

- Package manager (go mod, cargo, pip/uv, npm/pnpm)
- Build conventions
- Test runners
- IDE integrations

This works fine for small projects. But as projects grow, you encounter challenges:

1. **"Works on my machine"** - Different developers have different tool versions
2. **Slow CI/CD** - Every change rebuilds everything, even unrelated code
3. **Dependency hell** - Conflicting versions across languages and packages
4. **AI agent friction** - Automated tools struggle with slow, non-incremental builds

The enterprise answer to these problems is typically a monorepo with a sophisticated build system like Bazel or Buck2. But adopting a monorepo means:

- Rewriting all your build logic
- Learning new command-line tools
- Breaking IDE integrations
- Significant upfront investment

## The Turnkey Solution

Turnkey takes a different approach: **keep your familiar tools working normally while adding build system benefits invisibly**.

```bash
# These still work exactly as expected
go build ./...
cargo test
pytest
npm run build

# But now you also have Buck2's power when you need it
buck2 build //...
buck2 test //...
```

The key insight is that most developers don't need to think about the build system most of the time. They want to:

- Write code
- Run tests
- Get fast feedback

Turnkey provides this while maintaining a **single source of truth** for dependencies and builds that enables advanced features like:

- Hermetic, reproducible builds
- Incremental compilation across languages
- Remote caching and execution
- Atomic changes across the entire codebase

## Who Is Turnkey For?

Turnkey is designed for teams that want:

**Enterprise-grade infrastructure** without abandoning their existing workflows. Your `go build` still works. Your IDE still works. Your junior developers don't need to learn Buck2 to be productive.

**A growth path** from prototype to production. Start with normal language tooling. Adopt Buck2 features incrementally as your needs grow. No big-bang rewrites.

**AI-friendly development** with fast feedback loops. AI coding assistants work better when builds are fast and incremental. Turnkey's caching means AI agents can iterate quickly.

**Reproducibility without ceremony**. Nix handles tool versioning. Buck2 handles build caching. You focus on writing code.

## The Turnkey Philosophy

1. **Tools should enhance, not replace** - Native commands work normally
2. **Complexity should be opt-in** - Start simple, add sophistication as needed
3. **Reproducibility is non-negotiable** - Same inputs always produce same outputs
4. **Fast feedback enables better code** - Incremental builds by default

In the following chapters, we'll explore the core principles that make this possible and how the architecture enables a seamless developer experience.
