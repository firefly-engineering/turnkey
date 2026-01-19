# Troubleshooting

Common issues and solutions when using Turnkey.

## Shell Issues

### "attribute 'X' missing" when entering shell

**Cause:** A toolchain in `toolchain.toml` isn't in the registry.

**Solution:** Either remove the toolchain from `toolchain.toml` or add it to your registry in `flake.nix`.

### Changes to toolchain.toml not taking effect

**Cause:** Nix flake caching.

**Solution:**
1. Stage changes: `git add toolchain.toml`
2. Re-enter shell: `exit && nix develop`

## Build Issues

### "toolchain not found" error

**Cause:** The language toolchain wasn't generated.

**Solution:** Ensure the toolchain is:
1. Declared in `toolchain.toml`
2. Has a mapping in `nix/buck2/mappings.nix` (for custom toolchains)

### "missing BUCK file" or "missing rules.star"

**Cause:** Buck2 can't find build files.

**Solution:** Check that:
1. `.buckconfig` has `[buildfile] name = rules.star`
2. All cells have proper `.buckconfig` with buildfile settings

### Stale dependency errors

**Cause:** Dependency cells out of sync with lock files.

**Solution:**
```bash
tk sync
tk build //...
```

## Dependency Issues

### godeps cell missing packages

**Cause:** `go-deps.toml` out of date.

**Solution:**
```bash
godeps-gen > go-deps.toml
git add go-deps.toml
# Re-enter shell
```

### Rust feature conflicts

**Cause:** Conflicting feature requirements across crates.

**Solution:** Create `rust-features.toml` with explicit overrides:
```toml
[overrides]
problematic-crate = ["feature1", "feature2"]
```

## Getting Help

- Check [GitHub Issues](https://github.com/firefly-engineering/turnkey/issues)
- Enable verbose mode: `TURNKEY_VERBOSE=1 nix develop`
- Check Buck2 logs: `tk log show`
