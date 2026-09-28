# Does the write-once deps-cell layout work through `turnkey-composed` and under remote execution?

Research for [Can turnkey-composed and remote execution serve the write-once rustdeps layout?](https://github.com/firefly-engineering/turnkey/issues/151),
part of map [Rust dependency cell: correct feature resolution and cheaper BUCK
generation](https://github.com/firefly-engineering/turnkey/issues/82). It takes
the layout decided in [ADR 0004](../adr/0004-deps-cells-are-write-once-directories.md)
as given and asks what it means for the FUSE composition layer and for remote
execution (RE). Gathered on 2026-09-28 from:

- turnkey's own code at `b4e516e9` (the ADR 0004 commit);
- buck2 at the pinned commit
  [`6507dd15`](https://github.com/facebook/buck2/tree/6507dd157a6f81a810c48583edf1758dd0c337c5)
  ([`nix/buck2/buck2-source.nix` L28-L36](../../nix/buck2/buck2-source.nix#L28-L36)),
  read with `git show` from a local clone. Every buck2 link is a permalink at
  that commit;
- the REAPI proto at
  [`bazelbuild/remote-apis@adbf4a27`](https://github.com/bazelbuild/remote-apis/blob/adbf4a27c86fbea4a37637a6cbcacef372406fe7/build/bazel/remote/execution/v2/remote_execution.proto),
  the revision the earlier RE notes cite;
- the earlier research in this directory, which this note builds on and does
  not repeat: [remote-execution-and-caching.md](remote-execution-and-caching.md),
  [nix-paths-on-re-workers.md](nix-paths-on-re-workers.md),
  [local-re-api-servers.md](local-re-api-servers.md),
  [macfuse-fskit-limits.md](macfuse-fskit-limits.md) and
  [rustdeps-per-crate-generation.md](rustdeps-per-crate-generation.md) §3.3,
  §8 and §9.

Nothing was mounted, no RE server was started and nothing was built. This is
source reading only. **(verified)** means the cited source shows it.
*(inferred)* marks a conclusion drawn from sources rather than stated in them.
*(not determined)* marks something no source here settles.

The ticket asks three things:

1. Can `turnkey-composed` serve the write-once layout, and would buck2 still
   see the store links as external absolute symlinks through the mount? What
   would have to change, and is FUSE needed for deps cells at all?
2. How does buck2 send an input reached through an absolute store symlink to
   RE? Does per-package keying hold for remote action digests, and what must a
   worker provide?
3. What else reads `.turnkey/<cell>` in the FUSE or RE path?

## TL;DR

- **FUSE today never serves a deps cell's contents on macOS.** The macOS
  backend presents each cell as a *symlink* to its store path, so the kernel
  follows it into `/nix/store` and FUSE is bypassed. The Linux backend
  presents the cell as a pass-through directory. It passes nested symlinks
  through unchanged, absolute targets included. **(verified)**
- **Serving the write-once directory as an `external/` cell does not work
  unchanged.**
  - On macOS the cell would become a symlink to the *repo's* directory.
    buck2 would then key every package under one machine-specific path, and it
    would not watch the alias files behind it. *(inferred)*
  - On Linux, per-package keying survives, because each `_store/<basename>`
    comes through as an absolute symlink. But alias files changed in the repo
    raise no event on the mount, so a running daemon reads them stale.
    *(inferred)*
- **The FUSE cell-transition machinery does not help, because it is not
  wired.**
  - After a rebuild, the daemon discards the new cell paths. `refresh()` is a
    no-op, and the code carries a `TODO: update backend's cell paths`.
  - Nothing calls `apply_pending_updates` outside tests.
  - No kernel invalidation is ever sent, and FSKit does not support it anyway.
  **(verified)**
- **FUSE is not needed for deps cells under this layout.**
  - The layout solves the two problems FUSE was meant to solve for cells:
    stale reads, and a refresh that needs a shell reload.
  - A fixed mount path does not enter buck2's keys. Store-link targets are
    `/nix/store/…` on every machine.
- **RE: buck2 sends each store link as a REAPI `SymlinkNode` with an absolute
  target.** It never uploads the contents. It also never checks the server's
  `symlink_absolute_path_strategy`. Both are unchanged from today. **(verified)**
- **Per-package keying carries over to remote action digests.** The input root
  holds only the action's own inputs. So an unchanged package keeps the same
  `SymlinkNode` (name and target), and its compile keeps the same digest while
  its dependencies' outputs are unchanged. *(inferred from source; not run
  against a server)*
- **Workers need less, not more.**
  - An action names only the store paths of the packages it reads, instead of
    the whole 552 MB `rustdeps-cell`.
  - Those per-package paths are built locally, so they must be pushed to the
    service's Nix cache. The same is true of today's cell. *(inferred)*
  - turnkey has a design for this ([nix-paths-on-re-workers.md](nix-paths-on-re-workers.md))
    but no executor.
- **Other consumers:**
  - `cellfresh` drops `rustdeps` from its state and restarts the daemon once,
    at migration. It does nothing under a FUSE mount. **(verified)**
  - The daemon's `.turnkey/toolchains` readlink is unaffected.
  - The daemon's `*-cell` flake-package discovery would keep serving the old
    monolithic cell unless it changes.

## 1. The layout, and the two buck2 facts it rests on

[ADR 0004](../adr/0004-deps-cells-are-write-once-directories.md) makes
`.turnkey/rustdeps` a real directory in the project. It holds:

- `_store/<store-basename>`: absolute symlinks to per-package store paths,
  created or deleted but never retargeted;
- `vendor/<name>@<version>/rules.star` and `vendor/<name>/rules.star`: real
  files of `alias()` rules pointing at `//_store/<basename>:<target>`;
- a real `.buckconfig`.

It rests on two buck2 facts:

- **Keying.** buck2 reads metadata one path component at a time from the
  project root. At the first symlink whose target is absolute, it stops and
  returns `ExternalSymlink(target, rest)`
  ([`io/fs.rs` L252-L262, L310-L316, L357-L360](https://github.com/facebook/buck2/blob/6507dd157a6f81a810c48583edf1758dd0c337c5/app/buck2_common/src/io/fs.rs#L252-L262)).
  **(verified)**
  - Files are content-hashed only when no symlink was met on the way
    ([L269-L298](https://github.com/facebook/buck2/blob/6507dd157a6f81a810c48583edf1758dd0c337c5/app/buck2_common/src/io/fs.rs#L269-L298)).
  - So for `.turnkey/rustdeps/_store/<b>/src/lib.rs`, the first absolute
    symlink is `_store/<b>`, which gives one key per package.
- **Staleness.** A running daemon does not invalidate what it read through a
  symlink that is later retargeted. This was measured in
  [rustdeps-per-crate-generation.md §9.2](rustdeps-per-crate-generation.md#92-️-buck2-does-not-notice-a-retargeted-directory-symlink).
  Everything that changes on a bump is therefore a real file that buck2's file
  watcher sees.

Both facts matter below: whatever sits between buck2 and the files must
preserve the first one and not reintroduce the second.

## 2. What `turnkey-composed` does with a cell today

### 2.1 Where cells come from

- **Discovery is `nix build` of every `*-cell` flake package.** The `-cell`
  suffix is stripped, and each resulting store path becomes a cell
  ([`discover.rs` L21-L54](../../src/rust/composition/src/discover.rs#L21-L54)).
  **(verified)**
- The flake-parts module exports those packages from each language's cell
  derivation
  ([`nix/flake-parts/turnkey/default.nix` L437-L454](../../nix/flake-parts/turnkey/default.nix#L437-L454)).
  **(verified)**
- **The toolchains cell is found differently**: by `read_link` on
  `.turnkey/toolchains`, and only if the link is a symlink into `/nix/store/`
  ([`discover.rs` L86-L96](../../src/rust/composition/src/discover.rs#L86-L96)).
  **(verified)**
- **A cell is one path.** `FsCore` keeps `cell_paths: name → PathBuf`. A child
  path resolves to `cell_source.join(child)`
  ([`fs_core.rs` L832-L859](../../src/rust/composition/src/fuse/fs_core.rs#L832-L859)).
  **(verified)**
- The generated mount-root `.buckconfig` declares `<cell> =
  external/<cell>` and `root = root`
  ([`layout.rs` L331-L352](../../src/rust/composition/src/layout.rs#L331-L352)).
  The default prefix is `external`
  ([`config.rs` L106](../../src/rust/composition/src/config.rs#L106)).
  **(verified)**

### 2.2 How a cell is presented: the two backends differ

| | Linux (`fuser`) | macOS (libfuse3 FFI; macFUSE, FUSE-T compatible) |
|---|---|---|
| `external/<cell>` itself | A **directory**. `lookup` answers with `fs::metadata` (which follows symlinks) of the cell's store path ([`filesystem.rs` L185-L202](../../src/rust/composition/src/fuse/filesystem.rs#L185-L202)) | A **symlink** whose target is the store path. `getattr` fills `S_IFLNK`, and `readlink` returns the store path ([`operations.rs` L210-L217, L438-L440](../../src/rust/composition/src/fuse/fuse_t/operations.rs#L210-L217)). The code comment says this is deliberate: the kernel follows it "bypassing FUSE entirely" |
| Children of the cell | Pass-through. `lookup`/`getattr` use `fs::symlink_metadata`, so a nested symlink is reported as a symlink ([`filesystem.rs` L239-L257, L289-L304](../../src/rust/composition/src/fuse/filesystem.rs#L239-L257)) | `CellChild` is handled the same way ([`operations.rs` L229-L241](../../src/rust/composition/src/fuse/fuse_t/operations.rs#L229-L241)). In practice it is never reached through the mount, because the kernel has already left for `/nix/store` at the cell symlink *(inferred)* |
| `readlink` of a nested symlink | Returns `fs::read_link` verbatim. An absolute target stays absolute ([`filesystem.rs` L686-L699](../../src/rust/composition/src/fuse/filesystem.rs#L686-L699)) | Same, verbatim ([`operations.rs` L450-L454](../../src/rust/composition/src/fuse/fuse_t/operations.rs#L450-L454)) |
| Mount options | Read-only (`MountOption::RO`) ([`backend.rs` L156-L158](../../src/rust/composition/src/fuse/backend.rs#L156-L158)). Attribute and entry TTL 1 s by default ([`performance.rs` L44-L53](../../src/rust/composition/src/performance.rs#L44-L53)) | `entry_timeout` and `attr_timeout` 300 s, `kernel_cache`, `auto_cache` ([`operations.rs` L483-L495](../../src/rust/composition/src/fuse/fuse_t/operations.rs#L483-L495)) |

All of the above is **(verified)** unless marked. Consequences:

- **Symlinks are never resolved or flattened by the daemon.** It reports what
  `symlink_metadata` and `read_link` say. An absolute symlink inside a
  pass-through directory reaches buck2 as an absolute symlink, and buck2
  applies the rule in §1 to it. *(inferred from the two code paths above and
  `io/fs.rs` L310-L316)*
- **On macOS, today's FUSE cell keys exactly like today's plain symlink cell.**
  buck2 meets `external/rustdeps -> /nix/store/<hash>-rustdeps-cell` as the
  first absolute symlink. The whole-cell keying of
  [rustdeps-per-crate-generation.md §8.1](rustdeps-per-crate-generation.md#81-todays-cell-the-whole-closure-recompiles)
  applies unchanged *(inferred)*.
- **On macOS the policy layer and the edit overlay never see cell traffic**
  *(inferred)*. Both hang off `CellChild` resolution, and the kernel never
  sends such a path, because the cell is a symlink out of the mount. On Linux
  the mount is read-only, so the edit overlay's write path cannot run there
  either *(inferred from `MountOption::RO`)*.

### 2.3 Cell transitions: designed, but not wired

- **The state machine exists.**
  - `trigger_update`, `build_complete_with_updates` and
    `take_pending_updates` are in
    [`state.rs` L234, L317, L354](../../src/rust/composition/src/state.rs#L234).
  - `FsCore::apply_pending_updates` swaps `cell_paths` and drops cached inode
    mappings under the old path
    ([`fs_core.rs` L660-L723](../../src/rust/composition/src/fuse/fs_core.rs#L660-L723)).
  **(verified)**
- **Nothing drives it.** Outside unit and integration tests, no code calls
  `trigger_update`, `build_complete_with_updates` or `apply_pending_updates`
  (a `grep` over `src/`). **(verified)**
- **A manifest change rebuilds the cells and then throws the result away.**
  - Both daemon loops call `discover::build_all_cells`, then
    `backend.refresh()`
    ([`main.rs` L410-L433](../../src/cmd/turnkey-composed/src/main.rs#L410-L433),
    [L708-L731](../../src/cmd/turnkey-composed/src/main.rs#L708-L731)).
    The second loop is marked `// TODO: update backend's cell paths with new
    store paths`.
  - `refresh()` is "currently a no-op" on both backends
    ([Linux `backend.rs` L248-L260](../../src/rust/composition/src/fuse/backend.rs#L248-L260),
    [macOS `fuse_t/backend.rs` L339-L345](../../src/rust/composition/src/fuse/fuse_t/backend.rs#L339-L345)).
  - A mounted cell therefore keeps its first store path until the daemon
    restarts. **(verified)**
- **No kernel invalidation is sent.** There is no call to any
  `notify_inval_*` / `Notifier` API in `src/rust/composition` or
  `src/cmd/turnkey-composed`. **(verified)**
  - It would not help buck2 on Linux anyway. Invalidation drops kernel caches
    but raises no inotify event
    ([fuse-composition-layer.md, "Why inotify doesn't work automatically"](../architecture/fuse-composition-layer.md#why-inotify-doesnt-work-automatically)).
  - On macFUSE FSKit the notification API is not implemented
    ([macfuse-fskit-limits.md §1](macfuse-fskit-limits.md#what-makes-the-kernel-send-forget-and-can-a-daemon-encourage-it)).
- **`.turnkey/.cell-targets` is never written by the daemon.** The
  architecture doc proposes that the daemon write it after each transition
  ([fuse-composition-layer.md, "Recommended approach"](../architecture/fuse-composition-layer.md#recommended-approach)).
  Only `cellfresh` writes it
  ([`cellfresh.go` L25, L142](../../src/go/pkg/cellfresh/cellfresh.go#L25)).
  **(verified)**

## 3. Serving the write-once layout through FUSE

The cell's source is no longer one store path. It is the materialized
directory in the repo, which the materializer keeps in line with the cell
index. There are two ways to put it in front of buck2 on the mount.

### 3.1 F1: keep `rustdeps = external/rustdeps`, backed by the repo directory

| | Linux | macOS |
|---|---|---|
| What buck2 meets | `external/rustdeps` is a directory. `_store/<b>` is an absolute symlink to `/nix/store/<b>` | `external/rustdeps` is a symlink to `/<repo>/.turnkey/rustdeps` |
| Keying | **Per package kept.** The first absolute symlink is `_store/<b>` *(inferred)* | **Broken.** The first absolute symlink is the cell itself. Every file becomes `ExternalSymlink("/<repo>/.turnkey/rustdeps", rest)`. In an action's input root this becomes a single `SymlinkNode` at `external/rustdeps` that points at a laptop path, and the rest is dropped (`without_remaining_path`, [`directory.rs` L657-L664](https://github.com/facebook/buck2/blob/6507dd157a6f81a810c48583edf1758dd0c337c5/app/buck2_execute/src/directory.rs#L657-L664)). The key no longer names the package, and RE cannot resolve it *(inferred)* |
| Alias files and `.buckconfig` | Regular files on the mount, content-hashed. But the materializer writes them in the repo, not through the mount (which is read-only). No inotify event reaches the mount path, so a running daemon keeps the old alias package. That is the §9.2 failure again, now for real files *(inferred)* | Behind the external symlink, so they are not watched at all *(inferred)* |

**F1 is unusable on macOS and stale on Linux.** On macOS it would need the
cell to be presented as a directory, not a symlink. That is a behaviour change
to `fuse_getattr`/`fuse_readlink` for this cell kind.

### 3.2 F2: point the mount's buck2 cell at the source pass-through

The mount already serves the repo at `root/`, including
`root/.turnkey/rustdeps`, through `SourceChild`. `SourceChild` uses
`symlink_metadata` and verbatim `read_link` on both backends
([`operations.rs` L229-L241, L450-L454](../../src/rust/composition/src/fuse/fuse_t/operations.rs#L229-L241);
[`filesystem.rs` L228-L236](../../src/rust/composition/src/fuse/filesystem.rs#L228-L236)).
**(verified)** If `Buck2Layout` wrote `rustdeps = root/.turnkey/rustdeps` for
a write-once cell:

- **Keying.** `_store/<b>` reaches buck2 as an absolute symlink on both
  platforms, so each package gets its own key *(inferred)*.
- **The cell index is not needed by the daemon.** The layout only needs to know
  which cells are write-once. The materializer stays the only owner of the
  directory, as the ADR requires.
- **Staleness is the FUSE layer's existing source-edit problem, not a new
  one.** A file changed in the repo, rather than through the mount, raises no
  event on the mount. That is true of every source file under `root/` today.
  The architecture doc's Strategy 2 (a journal-backed watcher) is the proposed
  fix, and it is unbuilt
  ([fuse-composition-layer.md, "Notification strategies"](../architecture/fuse-composition-layer.md#notification-strategies)).
  Until then, a bump under FUSE needs a daemon restart. That loses the "no
  restart" half of ADR 0004's gain, though not the per-package keying.
  *(inferred)*
  - buck2 also offers `file_watcher = fs_hash_crawler`, which re-hashes the
    whole repository on every command. It is documented as "Useful for tests
    on unreliable filesystems, but probably not much elsewhere"
    ([`fs_hash_crawler.rs` L49-L51](https://github.com/facebook/buck2/blob/6507dd157a6f81a810c48583edf1758dd0c337c5/app/buck2_file_watcher/src/fs_hash_crawler.rs#L49-L51),
    selected in [`file_watcher.rs` L92-L139](https://github.com/facebook/buck2/blob/6507dd157a6f81a810c48583edf1758dd0c337c5/app/buck2_file_watcher/src/file_watcher.rs#L92-L139)).
    **(verified)** It is not a real option at this size *(inferred)*.
  - macOS adds 300 s attribute and entry caching on top (§2.2). A file
    changed behind the daemon's back can read stale for up to that long, and
    FSKit cannot be told to drop it *(inferred)*.
- **The `root` cell would contain the `rustdeps` cell's directory without a
  cell boundary on disk.** buck2's cell resolver should assign paths under
  `root/.turnkey/rustdeps` to `rustdeps`, because it is the more specific cell
  *(not determined for recursive `root//...` patterns: whether buck2 descends
  into a nested cell's directory when expanding `...` of the enclosing cell)*.
  The non-FUSE layout already nests `.turnkey/rustdeps` inside `root = .`
  ([`.buckconfig` `[cells]`](../../nix/buck2/buckconfig.nix)), so this is the
  same shape turnkey uses outside FUSE *(inferred)*.

### 3.3 What the composition layer would need to change

| Area | Today | For write-once cells |
|---|---|---|
| Cell source | One store path from `nix build .#<cell>-cell` ([`discover.rs` L21-L54](../../src/rust/composition/src/discover.rs#L21-L54)) | No store path. Either skip the cell in discovery and map it to `root/.turnkey/<cell>` (F2), or serve the repo directory as a pass-through directory (F1, Linux only) |
| Cell presentation on macOS | Symlink to the source ([`operations.rs` L210-L217](../../src/rust/composition/src/fuse/fuse_t/operations.rs#L210-L217)) | Must not be a symlink for a repo-backed cell (§3.1). F2 avoids the question |
| `Buck2Layout` | `<cell> = external/<cell>` for every discovered cell ([`layout.rs` L344-L352](../../src/rust/composition/src/layout.rs#L344-L352)) | `<cell> = <source_dir_name>/.turnkey/<cell>` for write-once cells (F2) |
| Flake exports | `rustdeps-cell` is the monolithic cell | If `rustdeps-cell` stays the monolithic cell, the daemon keeps serving the old layout and keying to FUSE users. If it becomes the cell index (a file), discovery would try to serve a file as a cell directory *(inferred)*. Either way, discovery must learn which cells are write-once |
| Transitions | Not wired (§2.3) | Nothing to transition: the materializer changes files in place. The state machine and policy layer have no role for these cells |
| Change notification | None | Needed for alias files, as for any source file (§3.2) |
| Edit overlay and patches | Patch names follow `vendor/<name>@<version>/…` inside the merged cell ([fuse-composition-layer.md L322-L329](../architecture/fuse-composition-layer.md#L322-L329)) | Sources live under `_store/<basename>/`, which is behind an absolute symlink into the read-only store. Writes there cannot be intercepted by the mount. Patches have to move to the per-package derivation ([rustdeps-per-crate-generation.md §9.5](rustdeps-per-crate-generation.md#95-not-covered)) *(inferred)* |

### 3.4 Does the FUSE layer's own invalidation help or hurt?

- **It cannot help with the problem the layout exists for.** The staleness in
  §1 is inside buck2's daemon. Kernel invalidation (`notify_inval_*`) only
  drops kernel caches. It raises no watcher event on Linux, and FSKit does not
  implement it (§2.3). The "Transitioning" state swaps a cell's backing path
  under a stable mount path. That is the retarget ADR 0004 forbids, done
  invisibly, so it would hurt if it were ever wired for a deps cell *(inferred)*.
- **It is harmless as it stands**, because it is not wired (§2.3).

### 3.5 Is FUSE needed for deps cells?

The FUSE layer was introduced for the reasons below
([fuse-composition-layer.md L5-L9, L139-L144, L480-L488](../architecture/fuse-composition-layer.md#L5-L9);
[fuse-composition-research.md "Problem Statement"](fuse-composition-research.md#problem-statement)).
Checked against the write-once layout:

| Motivation | Status for deps cells under ADR 0004 |
|---|---|
| "Cells must be explicitly refreshed", "requires re-entering `nix develop`" | Solved without FUSE by the materializer ([CONTEXT.md "Cell materialization"](../../CONTEXT.md#deps-cells)). The shell-reload bug of [rustdeps-per-crate-generation.md §8.2](rustdeps-per-crate-generation.md#82-️-the-cell-does-not-follow-rust-depstoml-until-the-shell-is-forced-to-reload) is a separate issue for the index build *(inferred)* |
| "Consistency guarantees when underlying Nix derivations are updating" | Solved by write-once store links plus real alias files. The FUSE transition code is not wired (§2.3) |
| "Fixed mount locations for predictable remote caching" | Not needed. buck2 keys sources by project-relative path, and checkout location does not matter ([remote-execution-and-caching.md §3.3](remote-execution-and-caching.md#33-what-turnkeys-actions-put-in-the-key-and-what-they-leave-out)). Store-link targets are `/nix/store/<b>`, the same on every machine for the same inputs and system |
| "Transparent external dependency editing" | Unreachable for store-link content (§3.3), and already unreachable for cells on macOS (§2.2) |
| Pluggable layouts (Bazel) | Unrelated to deps-cell content |

**So no: under this layout, FUSE has no reason to exist for deps cells.**
*(inferred)* Its remaining value lies elsewhere: the `root/` view, `bin/`,
VCS wrappers and other build systems. Keys also differ between FUSE and
non-FUSE checkouts, because source paths gain the `root/` prefix and cells move
from `.turnkey/` to `external/`. The two therefore never share cache entries,
with or without this change *(inferred from §2.1 and the project-relative
keying)*.

## 4. Remote execution

### 4.1 How buck2 sends an input reached through a store link

- **The input root holds only the action's inputs.**
  `CommandExecutionPaths::new` builds the input directory from the action's
  `inputs` and fingerprints it
  ([`request.rs` L223-L260](https://github.com/facebook/buck2/blob/6507dd157a6f81a810c48583edf1758dd0c337c5/app/buck2_execute/src/execute/request.rs#L223-L260)).
  **(verified)**
- **An `ExternalSymlink` becomes one symlink entry, at the link's own path.**
  `insert_entry` strips `rest` from the input's path and inserts the symlink
  without it ([`directory.rs` L623-L667](https://github.com/facebook/buck2/blob/6507dd157a6f81a810c48583edf1758dd0c337c5/app/buck2_execute/src/directory.rs#L623-L667)).
  **(verified)**
  - For `.turnkey/rustdeps/_store/<b>/src/lib.rs`, that entry is at
    `.turnkey/rustdeps/_store/<b>`, with target `/nix/store/<b>`.
  - All files of one package collapse onto that one entry.
- **On the wire it is a REAPI `SymlinkNode` whose target is the absolute path,
  verbatim** ([`directory.rs` L200-L208](https://github.com/facebook/buck2/blob/6507dd157a6f81a810c48583edf1758dd0c337c5/app/buck2_execute/src/directory.rs#L200-L208);
  `target_str` is the raw target, [`external_symlink.rs` L128-L130](https://github.com/facebook/buck2/blob/6507dd157a6f81a810c48583edf1758dd0c337c5/app/buck2_common/src/external_symlink.rs#L128-L130)).
  **(verified)**
  - The proto allows absolute targets: "it can be an absolute path starting
    with `/`"
    ([`SymlinkNode`, L1126-L1141](https://github.com/bazelbuild/remote-apis/blob/adbf4a27c86fbea4a37637a6cbcacef372406fe7/build/bazel/remote/execution/v2/remote_execution.proto#L1126-L1141)).
  - The node sits in its parent `Directory`'s `symlinks`
    ([L1048-L1060](https://github.com/bazelbuild/remote-apis/blob/adbf4a27c86fbea4a37637a6cbcacef372406fe7/build/bazel/remote/execution/v2/remote_execution.proto#L1048-L1060)).
    It is therefore part of the Merkle tree whose root is the `Action`'s
    `input_root_digest`
    ([L674-L687](https://github.com/bazelbuild/remote-apis/blob/adbf4a27c86fbea4a37637a6cbcacef372406fe7/build/bazel/remote/execution/v2/remote_execution.proto#L674-L687)).
- **buck2 does not follow the link and does not upload the package's files.**
  There is no `File` node for anything under `_store/<b>`. This is unchanged
  from [remote-execution-and-caching.md §3.2](remote-execution-and-caching.md#32-buck2-at-6507dd15).
  **(verified)**

### 4.2 The absolute-symlink capability

- A server advertises `CacheCapabilities.symlink_absolute_path_strategy`
  ([L2416-L2434](https://github.com/bazelbuild/remote-apis/blob/adbf4a27c86fbea4a37637a6cbcacef372406fe7/build/bazel/remote/execution/v2/remote_execution.proto#L2416-L2434)).
  With `DISALLOWED`, it "will return an `INVALID_ARGUMENT` on input symlinks
  with absolute targets". With `ALLOWED`, it "will allow symlink targets to
  escape the input root tree"
  ([L2375-L2389](https://github.com/bazelbuild/remote-apis/blob/adbf4a27c86fbea4a37637a6cbcacef372406fe7/build/bazel/remote/execution/v2/remote_execution.proto#L2375-L2389)).
  **(verified)**
- **buck2's OSS client does not read that field.** `fetch_rbe_capabilities`
  keeps only `supported_compressors` and `max_batch_total_size_bytes`
  ([`re_grpc/src/client.rs` L352-L399](https://github.com/facebook/buck2/blob/6507dd157a6f81a810c48583edf1758dd0c337c5/remote_execution/oss/re_grpc/src/client.rs#L352-L399)).
  A `DISALLOWED` server therefore fails the action at execution time instead
  of buck2 refusing up front. **(verified)**
- **The layout changes the number of absolute symlinks, not their kind.**
  Today every Rust action already sends one (`.turnkey/rustdeps`). A server
  that accepts today's cell accepts the new layout *(inferred)*.

### 4.3 Does per-package keying hold for remote action digests?

Yes *(inferred from §4.1; not run against a server)*:

- A `rustc` action for package P has as inputs P's sources, all under
  `_store/<b_P>`, plus its dependencies' outputs in `buck-out`, which are
  content-hashed. Its input root therefore holds one `SymlinkNode`
  `_store/<b_P> → /nix/store/<b_P>`, plus `File` nodes for the dependency
  outputs.
- **A bump of another package Q leaves P's input root unchanged, unless Q is
  in P's dependency closure.** The name and target of P's node depend only on
  `<b_P>`, which is write-once and a function of P's inputs alone
  ([rustdeps-per-crate-generation.md §8.4](rustdeps-per-crate-generation.md#84-what-this-means-for-the-decision)).
  The `_store` directory in P's input root lists only P's node, so the
  directory digest does not move either.
- **argv changes the same way.** The crate root is the project-relative path
  `.turnkey/rustdeps/_store/<b_P>/…`. The label behind the alias is
  `rustdeps//_store/<b_P>:<target>`. Both are per package *(inferred; the
  exact rustc argv was not inspected)*.
- **Today's cell has the opposite property.** Each Rust action's input root
  holds `.turnkey/rustdeps → /nix/store/<cell>`, so every bump re-keys every
  vendored compile, locally and remotely. That is the 1184-action result of
  [§8.1](rustdeps-per-crate-generation.md#81-todays-cell-the-whole-closure-recompiles),
  which a shared cache would inherit as a full miss *(inferred)*.
- **The same holds for test result keys.** turnkey's test runner records
  results under buck2's action digest
  ([`cache.rs` L5-L6, L54-L55](../../src/cmd/turnkey-test-runner/src/cache.rs#L5-L6)),
  so a test that reads package files at run time is keyed per package too
  *(inferred)*.

### 4.4 What a worker must provide

- **Every store path named by a `SymlinkNode`, at the same absolute path.**
  The key names the path; the worker must supply it
  ([remote-execution-and-caching.md §3.1](remote-execution-and-caching.md#31-reapi),
  [nix-paths-on-re-workers.md §3.1](nix-paths-on-re-workers.md#31-the-channels)).
- **Discovery gets simpler.** Option D1 of
  [nix-paths-on-re-workers.md §4](nix-paths-on-re-workers.md#4-discovering-the-needed-paths)
  (scan `SymlinkNode` targets) now finds exactly the packages an action reads,
  instead of the whole cell.
  - Store basenames also appear in labels (`rustdeps//_store/<b>:…`). The
    recommended parser starts at `/nix/store/`, so it will not pick those up
    as false positives *(inferred)*.
- **Transfer shrinks.** Today a Rust action needs the 552 MB `rustdeps-cell`,
  and a new copy after every bump
  ([nix-paths-on-re-workers.md §3.3](nix-paths-on-re-workers.md#33-sizes-turnkeys-own-inputs)).
  With per-package paths, a worker fetches each package once. After a bump it
  fetches only the changed packages *(inferred)*.
  - Whether per-package paths have store references is *(not determined)*.
    The prototype's outputs are byte-identical to the cell's `vendor/<crate>`
    ([§9.1](rustdeps-per-crate-generation.md#91-what-was-built)), and today's
    cell has none, so probably not.
- **The paths must be in the service's Nix cache.** `rustcrate-*` and
  `dep-rust-*` are built by turnkey, not by nixpkgs, so no public substituter
  has them.
  - The client must push them, as it would have to push today's cell.
  - If the cell index is a store file whose contents name every package path,
    Nix's reference scan makes them its references. Pushing the index's
    closure would then push the cell in one step *(inferred)*.
  - A path missing from the cache is a precondition failure that names the
    missing paths, as designed in
    [nix-paths-on-re-workers.md §4](nix-paths-on-re-workers.md#4-discovering-the-needed-paths).
- **turnkey handles none of this yet for any cell.** There is a design
  ([nix-paths-on-re-workers.md](nix-paths-on-re-workers.md),
  [request-driven-reapi-executor.md](request-driven-reapi-executor.md)) and
  a local test result cache (the `[buck2_re_client]` section in
  [`nix/buck2/buckconfig.nix`](../../nix/buck2/buckconfig.nix)). There is no
  executor, and every `CommandExecutorConfig` in the repo has
  `remote_enabled = False`
  ([`test_caching.bzl` L108](../../nix/buck2/prelude-extensions/test_caching/test_caching.bzl#L108)).
  **(verified by grep)** So nothing breaks. The layout only changes what a
  future executor has to fetch.

### 4.5 Does anything that worked with today's single-cell symlink break?

Nothing found *(inferred)*:

- The alias `rules.star` files and the cell's `.buckconfig` are real files,
  but only the evaluator reads them. They are not action inputs, and they
  never reach the worker.
- No rule in the repo names a file inside `rustdeps` by path. The only
  references are `rustdeps//vendor/<pkg>:<target>` labels from the generator
  ([`generator.py` L95, L102](../../src/python/buck/turnkey/buck/generator.py#L95)).
  An alias package has no sources, so a path-based reference would fail. None
  exists today. **(verified by grep)**
- The one thing that cannot work is §3.1's macOS FUSE variant. There, the
  `SymlinkNode` target would be a repo path on a laptop.

## 5. Other consumers of `.turnkey/<cell>`

- **`cellfresh` (`tk`'s daemon-restart check).**
  - It records `os.Readlink` of every `.turnkey/*` entry that points into
    `/nix/store/`, and restarts the daemon when the set changes
    ([`cellfresh.go` L84-L117, L164-L183](../../src/go/pkg/cellfresh/cellfresh.go#L84-L117)).
  - A real `.turnkey/rustdeps` directory fails `Readlink`, so it leaves the
    set. The first `tk` run after migration reports `- .turnkey/rustdeps
    (removed)` and restarts the daemon once. After that, `rustdeps` changes
    never trigger a restart, which is what ADR 0004 intends. **(verified from
    code; not run)**
  - The other cells, the prelude and `.buckconfig` stay covered.
- **`cellfresh` under a FUSE mount does nothing.** `tk` takes the first
  ancestor holding `.buckconfig` or `.turnkey/sync.toml` as the root
  ([`syncconfig.go` L217-L231](../../src/go/pkg/syncconfig/syncconfig.go#L217-L231),
  [`main.go` L466-L475](../../src/cmd/tk/main.go#L466-L475)). At the mount
  root that is the virtual `.buckconfig`, and `<mount>/.turnkey` does not
  exist. So `readSymlinkTargets` finds nothing, and the check is skipped
  ([`cellfresh.go` L36-L42](../../src/go/pkg/cellfresh/cellfresh.go#L36-L42)).
  **(verified from code)**
  - The same root-finding means a materializer run from inside the mount
    would look for `<mount>/.turnkey/rustdeps`. It has to target the real
    repo *(inferred)*.
- **The daemon's `.turnkey/toolchains` readlink** is unaffected: ADR 0004
  covers deps cells only, and the toolchains cell stays a managed symlink
  ([`buck2.nix` L173-L195](../../nix/devenv/turnkey/buck2.nix#L173-L195)).
  If it ever became a directory, `is_symlink()` would be false and the daemon
  would silently drop the cell
  ([`discover.rs` L88-L89](../../src/rust/composition/src/discover.rs#L88-L89)).
  **(verified)**
- **The composition `SymlinkBackend`** creates `<mount_point>/<cell>` links by
  removing and re-creating them, which is a retarget
  ([`symlink.rs` L71-L86, L122-L135](../../src/rust/composition/src/symlink.rs#L71-L86)).
  For a write-once cell it would bring back whole-cell keying and stale reads.
  It must skip such cells, as F2 does *(inferred)*.
- **`tk compose edit`/`patch`** name patches after `vendor/…` paths in the
  merged cell (§3.3). This needs rework together with per-package patches
  *(inferred)*.

## 6. Not verified

- **Nothing here was run.** No mount, no RE server, no build. Every behavioural
  claim about buck2 through the mount, and about remote digests, is read off
  source.
- *(not determined)* Whether buck2's `root//...` expansion descends into a
  directory that is also another cell's root. This matters for F2, where
  `rustdeps = root/.turnkey/rustdeps` nests inside `root = root`.
- *(not determined)* Whether FSEvents (buck2's `notify` watcher on macOS)
  reports changes on a macFUSE volume at all, including changes made through
  the mount.
- *(not determined)* The exact `rustc` argv for an aliased vendored target,
  and whether any string in it other than project-relative paths and labels
  differs between today's cell and the new layout.
- *(not determined)* Whether per-package store paths have Nix references (for
  example, fixups that link native libraries).
- *(inferred, not tested)* That on Linux the kernel resolves a pass-through
  absolute symlink against the process root, so `/nix/store/<b>` is read from
  the real store, outside the mount.
- *(inferred)* That the macOS edit overlay and policy layer never see cell
  paths, because the kernel follows the cell symlink.
- *(inferred)* That no REAPI server turnkey has looked at is `DISALLOWED` by
  default. The earlier notes say only that localhost workers resolve absolute
  targets against the host store
  ([local-re-api-servers.md](local-re-api-servers.md)). No server's default
  was checked here.

## 7. Recommendation (input for [#95](https://github.com/firefly-engineering/turnkey/issues/95))

1. **Don't route write-once deps cells through FUSE.** The layout solves what
   FUSE was meant to solve for cells (§3.5), and the FUSE transition path is
   unwired and would retarget (§2.3, §3.4).
2. **Keep FUSE users working with the smallest change (F2, §3.2).** Have
   discovery skip write-once cells, and have `Buck2Layout` map them to
   `root/.turnkey/<cell>`. That keeps per-package keying on both platforms.
   It leaves FUSE users with the mount's existing change-notification gap:
   after a bump they need a daemon restart until a journal-backed watcher
   exists. Two conditions for accepting this:
   - settle the nested-cell question in §6 with a small local experiment
     (needs no mount: nest a cell directory inside `root` in a scratch
     project);
   - never present a repo-backed cell as a symlink on macOS (§3.1).
3. **Stop exporting the monolithic `rustdeps-cell` as the thing FUSE serves**
   once the materializer lands. Otherwise FUSE users silently keep the old
   layout's whole-cell keying (§3.3). If the export becomes the cell index,
   discovery must not treat it as a cell directory.
4. **Remote execution needs nothing new from #95.** Record in the #95 design
   that:
   - store links go over the wire as absolute `SymlinkNode`s, as today;
   - per-package keying extends to remote and test-result digests (§4.3);
   - a future executor fetches per package instead of per cell (§4.4).

   One requirement follows for the build-service work, not for #95: whatever
   pushes to the service's Nix cache must push every package path in the
   current cell index. Pushing the index's closure does that, if the index
   references them.
5. **Carry two follow-ups outside #95:** `cellfresh`'s one-time restart at
   migration is the intended behaviour and needs no code (§5). The
   `SymlinkBackend` and `tk compose` patch naming need adjusting when
   per-package patches land (§5, §3.3).
