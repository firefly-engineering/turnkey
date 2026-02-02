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

## FUSE Issues

### FUSE not available on Linux

**Cause:** FUSE kernel module not loaded or `/dev/fuse` missing.

**Solution:**
```bash
# Load FUSE module
sudo modprobe fuse

# Verify
ls /dev/fuse
```

If persistent, add `fuse` to `/etc/modules-load.d/`.

### "FUSE-T not installed" on macOS

**Cause:** FUSE-T package not installed.

**Solution:**
```bash
brew install macos-fuse-t/homebrew-cask/fuse-t
```

### Mount point already in use

**Cause:** Previous daemon didn't unmount cleanly.

**Solution:**
```bash
# Force unmount
tk compose down --force

# Or manually
fusermount3 -uz /firefly/myproject  # Linux
umount -f /firefly/myproject         # macOS
```

### "Permission denied" on mount

**Cause:** User not in `fuse` group or mount point permissions.

**Solution:**
```bash
# Add user to fuse group (Linux)
sudo usermod -aG fuse $USER
# Log out and back in

# Check mount point permissions
sudo mkdir -p /firefly/myproject
sudo chown $USER:$USER /firefly/myproject
```

### Daemon won't start

**Cause:** Various issues with daemon lifecycle.

**Solution:**
```bash
# Check for existing processes
pgrep -f turnkey-composed

# Kill stale processes
pkill -9 -f turnkey-composed

# Remove stale socket
rm -f /run/turnkey-composed/*.sock

# Start with debug logging
TURNKEY_FUSE_DEBUG=1 tk compose up
```

### Files appear stale or missing

**Cause:** Dependency cells updating or policy blocking access.

**Solution:**
```bash
# Check daemon status
tk compose status

# Force refresh
tk compose refresh

# If in "building" state, wait or use lenient policy
TURNKEY_ACCESS_POLICY=lenient tk build //...
```

### "Resource temporarily unavailable" (EAGAIN)

**Cause:** CI policy returning errors during updates.

**Solution:**
- Wait for the build to complete
- Switch to development policy for interactive use
- Add retry logic in CI scripts

### Build hangs waiting for FUSE

**Cause:** Strict policy blocking during long Nix builds.

**Solution:**
```bash
# Check what's blocking
tk compose status --verbose

# Use lenient policy for quick iteration
TURNKEY_ACCESS_POLICY=lenient tk build //...

# Or increase timeout
TURNKEY_BLOCK_TIMEOUT=600 tk build //...
```

### Edits not persisting after restart

**Cause:** Edits stored in overlay, need to generate patches.

**Solution:**
```bash
# Generate patches before stopping
tk compose patch

# Then stop
tk compose down
```

### Container/Docker issues

**Cause:** FUSE requires privileged access in containers.

**Solution:**
```bash
# Run container with FUSE access
docker run --device /dev/fuse --cap-add SYS_ADMIN ...

# Or disable FUSE and use symlinks
TURNKEY_FUSE_BACKEND=symlink tk build //...
```

## Getting Help

- Check [GitHub Issues](https://github.com/firefly-engineering/turnkey/issues)
- Enable verbose mode: `TURNKEY_VERBOSE=1 nix develop`
- Check Buck2 logs: `tk log show`
- FUSE debug logs: `TURNKEY_FUSE_DEBUG=1 tk compose up`
