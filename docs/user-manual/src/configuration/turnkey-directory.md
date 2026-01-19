# The .turnkey Directory

Turnkey uses a `.turnkey` directory in your project root to store build artifacts, caches, and generated cells. This convention provides automatic isolation from language toolchains.

## Why .turnkey?

The `.turnkey` directory serves as the **isolation directory** for Buck2 builds. By using a dot-prefixed name, we get automatic exclusion from most language toolchains:

| Tool | Behavior | Configuration Needed |
|------|----------|---------------------|
| Go | Ignores directories starting with `.` or `_` | None (built-in) |
| Cargo | Doesn't auto-discover crates in dot directories | None (built-in) |
| pytest | Automatically ignores dot directories | None (built-in) |
| Jest | Requires explicit configuration | Yes |
| Vitest | Requires explicit configuration | Yes |

This means Go won't try to compile generated Buck2 cells, Cargo won't discover them as workspace members, and pytest won't scan them for tests.

## Directory Structure

```
.turnkey/
├── books/           # mdbook serve output (gitignored)
├── prelude/         # Symlink to Buck2 prelude derivation
├── toolchains/      # Symlink to generated toolchains cell
├── godeps/          # Symlink to Go dependencies cell
├── rustdeps/        # Symlink to Rust dependencies cell
└── jsdeps/          # Symlink to JavaScript dependencies cell
```

The symlinks point to Nix store paths containing the generated Buck2 cells.

## Buck2 Configuration

The `.buckconfig` sets the isolation directory:

```ini
[buck2]
isolation_dir = .turnkey
```

This tells Buck2 to store all build outputs under `.turnkey/buck-out/` instead of the default `buck-out/`.

## The tk Command

The `tk` command wraps `buck2` and automatically translates the `--isolation-dir` flag to use `.turnkey`-prefixed directories:

```bash
# These are equivalent:
tk --isolation-dir=foo build //...
buck2 --isolation-dir=.turnkey-foo build //...
```

This allows multiple isolated builds while maintaining the dot-prefix convention.

## JavaScript/TypeScript Configuration

Unlike Go, Cargo, and pytest, JavaScript test runners need explicit configuration to ignore dot directories.

### Jest

Add to your `jest.config.js`:

```javascript
module.exports = {
  testPathIgnorePatterns: [
    '/node_modules/',
    '/buck-out/',
    '/\\.'  // Ignore all dot-prefixed directories
  ],
};
```

Or in `package.json`:

```json
{
  "jest": {
    "testPathIgnorePatterns": [
      "/node_modules/",
      "/buck-out/",
      "/\\."
    ]
  }
}
```

### Vitest

Add to your `vitest.config.ts`:

```typescript
import { defineConfig } from 'vitest/config';

export default defineConfig({
  test: {
    exclude: [
      '**/node_modules/**',
      '**/buck-out/**',
      '**/.*/**'  // Ignore all dot-prefixed directories
    ],
  },
});
```

## Migration Notes

If you're migrating from a project that used `buck-out/` directly:

1. **One-time cache invalidation**: Buck2 caches are stored per isolation directory. Switching to `.turnkey` means a clean rebuild on first run.

2. **Update .gitignore**: Ensure `.turnkey/` is in your `.gitignore`:
   ```
   .turnkey/
   ```

3. **Update CI scripts**: If CI scripts reference `buck-out/`, update them to `.turnkey/buck-out/`.

## Multiple Isolation Directories

For advanced use cases (parallel builds, different configurations), you can use multiple isolation directories:

```bash
# Development build
tk build //...

# Release build with different isolation
tk --isolation-dir=release build //...
# Creates .turnkey-release/

# CI build
tk --isolation-dir=ci build //...
# Creates .turnkey-ci/
```

Each isolation directory maintains its own:
- Buck2 daemon
- Build cache
- Output artifacts

This is useful for:
- Running multiple Buck2 daemons simultaneously
- Keeping CI caches separate from local development
- Testing different build configurations
