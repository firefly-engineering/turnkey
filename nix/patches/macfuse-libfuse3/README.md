# macFUSE libfuse3 patches

Patches to macFUSE's vendored `Library-3/lib/fuse.c` (libfuse 3.18.2 + macFUSE
Darwin extensions) for known bugs that affect filesystems registered with
`darwin_extensions_enabled = 1`.

To use these patches, build a local libfuse against macFUSE's
`Library-3` source and run our daemon against it via `DYLD_LIBRARY_PATH`.
The macFUSE-shipped `/usr/local/lib/libfuse3.4.dylib` does **not** include
these fixes as of macFUSE 5.2.0.

## 0001-darwin-attr-overflow-fix.patch

Fixes an `__stack_chk_fail` (SIGABRT) that hits any FS registered with
`fuse_operations` of Darwin signatures (i.e. `getattr` taking
`*fuse_darwin_attr` rather than POSIX `*struct stat`) once the kernel /
FSKit dispatches enough work through it.

Two helper functions in `Library-3/lib/fuse.c` allocate a vanilla
`struct stat` (144 B on Darwin) on their stack and call
`fuse_fs_getattr(...)` (the unsuffixed name aliases to
`fuse_fs_getattr$DARWIN` via `DARWIN_SYMBOL`, but its `op.getattr.vanilla`
view of the union still hands the FS the small `struct stat` pointer).
A Darwin-signature FS writes a 192 B `fuse_darwin_attr` into that 144 B
buffer — 48 byte stack overrun, canary trips:

* `open_auto_cache` (fuse.c:~4096) — called from `fuse_lib_open` when
  `f->conf.auto_cache` is non-zero, on every file open.
* `hidden_name` (fuse.c:~2795) — called from `fuse_lib_unlink` when
  `f->conf.hard_remove` is zero and the file is currently open, on every
  unlink-while-open path.

The patch gates each call on `fuse_fs_darwin_extensions_enabled(f->fs)`
and uses `fuse_fs_getattr$DARWIN` with a stack-allocated
`fuse_darwin_attr` when set. Symmetric to how `fuse_lib_getattr` and
`fuse_lib_setattr` already split into `$DARWIN` and vanilla variants.

Identified via `tracking-4vl.7`. Should be filed upstream against
[macfuse/macfuse](https://github.com/macfuse/macfuse) with the same
analysis once we have a minimal C repro.
