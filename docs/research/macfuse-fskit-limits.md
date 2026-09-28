# What limits and known bugs in macFUSE FSKit does a buck2 build hit?

Research for [#142](https://github.com/firefly-engineering/turnkey/issues/142),
part of map [#85](https://github.com/firefly-engineering/turnkey/issues/85)
(reliable full builds through the FUSE mount). It feeds
[#29](https://github.com/firefly-engineering/turnkey/issues/29) (FSKit volume
disappears after ~10K ops) and
[#25](https://github.com/firefly-engineering/turnkey/issues/25) (ENXIO from
buck2's materializer on `symlink()`, benchmark blocked). Gathered on
2026-09-28 from:

- macFUSE GitHub releases 5.0.0 to 5.4.0, the newest being
  [5.4.0](https://github.com/macfuse/macfuse/releases/tag/macfuse-5.4.0)
  (2026-09-07);
- the macFUSE issue tracker (every issue matching `FSKit`, read in full where
  relevant);
- the macFUSE wiki (git history checked for staleness);
- macFUSE's open-source libfuse3 fork,
  [`macfuse/library`](https://github.com/macfuse/library), at `00e9044`. That is
  the `Library-3` submodule pinned by tag `macfuse-5.4.0`. The FSKit module
  (`FSModule`) and the kernel extension are closed source
  ([Open Source Status](https://github.com/macfuse/macfuse/wiki/Open-Source-Status));
- Apple's FSKit reference documentation and the macOS 15.4 to 26.6 release
  notes;
- turnkey's own code at `main` (`66506398`).

Nothing was mounted or run. **(verified)** means a primary source states it,
or the cited source code shows it. *(inferred)* means a conclusion drawn from
sources rather than stated in them. *(unverified)* means no source settles it.

## TL;DR

- **Require macFUSE ≥ 5.4.0.** The 5.2.0 install that #29 and #25 were measured
  on predates fixes for several FSKit-backend bugs that a buck2 build walks
  straight into:
  - silent zero-filling of 1–14 byte writes;
  - `exec()` returning EIO until the file has been read;
  - a module crash on a 0-byte READ reply, which kills the volume;
  - a race on cached volume statistics that crashed the extension;
  - GETATTR sent for node ids that were never looked up.

  Until #29 and #25 have been re-run on 5.4.0, their numbers describe a
  backend with known crashers.
- **macFUSE documents no node-id cap, memory budget or op-count watchdog.**
  Nothing in the releases, wiki or tracker describes one. In fact the FSKit
  backend does *not yet* honour `daemon_timeout` (see
  [macfuse#1201](https://github.com/macfuse/macfuse/issues/1201)), so the
  kext's 60-second "eject on timeout" is not what unmounts the volume. The
  most likely cause of the #29 unmount, given the fixes above, is an
  FSModule crash or fskitd-driven teardown in 5.2.0. *(inferred)*
- **FORGET follows vnode reclaim, not `entry_timeout`.** FSKit guarantees a
  `reclaimItem` "after the upper layers no longer reference that item" (Apple
  docs). The low FORGET:LOOKUP ratio in #29 is therefore expected and not a
  leak. Nothing a daemon can do on FSKit today reliably produces FORGET:
  - the FUSE notification API (`notify_inval_entry` and the rest) is
    documented as not implemented for FSKit;
  - `entry_timeout` only controls revalidation.
- **ENXIO** is how macFUSE reports a device that has been "marked as dead due
  to a timeout or error" (maintainer, macfuse#1136). XNU also returns it for
  operations on a revoked vnode (macfuse#1160). There is no known macFUSE bug
  about relative symlink targets. The #25 ENXIO most plausibly comes from a
  dead or revoked volume or vnode, which could be the #29 teardown seen from
  the other side, not from symlink target parsing. *(inferred)*
- **Two turnkey-side bugs came up during the read:**
  - `fuse_setattr` returns 0 without doing anything. libfuse's code shows that
    this silently drops every `chmod`, `truncate` and `utimens` that arrives as
    a SETATTR.
  - `auto_cache = 1` keeps turnkey on the libfuse stack-overrun path. That
    path is still unfixed in 5.4.0's libfuse3, so the local patch in
    `nix/patches/macfuse-libfuse3/` is still needed unless `auto_cache` is
    turned off and `hard_remove` turned on.

## 1. Does FSKit or macFUSE force-unmount a volume?

### What the sources say

- **No documented cap on live node ids, memory budget or op-count limit.**
  None appears in any release note from 5.0.0 to 5.4.0
  ([releases](https://github.com/macfuse/macfuse/releases)), in the wiki
  [FUSE Backends › Limitations](https://github.com/macfuse/macfuse/wiki/FUSE-Backends)
  or in any `FSKit` issue in the tracker. The FSModule source is closed, so
  "no documented cap" is as far as public sources go. **(verified absence
  of documentation; the existence of a limit is (unverified))**
- **`daemon_timeout` is not honoured by the FSKit backend.** On the kernel
  backend, a request unanswered for `daemon_timeout` seconds (default 60)
  ejects the volume
  ([Mount Options › daemon_timeout](https://github.com/macfuse/macfuse/wiki/Mount-Options#daemon_timeout)).
  For FSKit the maintainer states: "The FSKit backend does not yet honor the
  `daemon_timeout` mount-time option. In addition, some channel-layer error
  scenarios are not yet handled correctly."
  ([macfuse#1201](https://github.com/macfuse/macfuse/issues/1201),
  open against 5.4.0). If the server dies, the volume *hangs*; it does not
  unmount. His workaround for that case is: "one reliable way to force-unmount
  all FSKit volumes is to kill the `fskitd` process." **(verified)**
- **FSModule crashes kill the volume, and 5.2.0 → 5.4.0 fixed several:**
  - 5.3.0: "addressing a race condition when accessing cached volume
    statistics that could cause the file system extension to crash"
    ([5.3.0 notes](https://github.com/macfuse/macfuse/releases/tag/macfuse-5.3.0)).
    #29's counters show 1226 STATFS calls in one run.
  - 5.3.1: "fixing a bug in open file handle management"
    ([5.3.1 notes](https://github.com/macfuse/macfuse/releases/tag/macfuse-5.3.1)).
  - 5.4.0: a 0-byte READ reply crashed the extension with `EXC_BREAKPOINT`,
    after which "the volume is then dead for every process using it"
    ([macfuse#1188](https://github.com/macfuse/macfuse/issues/1188)). The same
    release fixed "item lock ordering and lock state tracking, validating file
    system replies, and releasing lookup references and file handles on error
    paths", and added "Include fatal error messages in crash reports"
    ([5.4.0 notes](https://github.com/macfuse/macfuse/releases/tag/macfuse-5.4.0)).

  **(verified)**
- **Crash reports land in `~/Library/Logs/DiagnosticReports/io.macfuse.app.fsmodule.macfuse-*.ips`**
  ([macfuse#1188](https://github.com/macfuse/macfuse/issues/1188), attached
  `.ips` files). Since 5.2.0, `macfuse log stream` streams the
  `com.apple.FSKit`, `com.apple.LiveFS` and `io.macfuse` log subsystems
  ([5.2.0 notes](https://github.com/macfuse/macfuse/releases/tag/macfuse-5.2.0)).
  #29 only looked in `/var/log`, which is not where either of these goes.
  **(verified)**
- **Apple's teardown order:** by the time `deactivate` runs, "FSKit has already
  performed a reclaim call to release all other file nodes", has issued a
  sync, and has called `unmount()`
  ([`deactivate(options:replyHandler:)`](https://developer.apple.com/documentation/fskit/fsvolume/operations/deactivate(options:replyhandler:))).
  **(verified)**

### What the #29 symptom does and does not prove

- In macFUSE's libfuse3, `fuse_session_destroy` calls the file system's
  `destroy` op whenever `got_init && !got_destroy`
  ([`lib/fuse_lowlevel.c:4616-4623`](https://github.com/macfuse/library/blob/00e9044/lib/fuse_lowlevel.c#L4616-L4623)).
  turnkey's `fuse_destroy` callback only reports metrics
  (`src/rust/composition/src/fuse/fuse_t/operations.rs:502`), and `backend.rs`
  always calls `fuse_destroy` after the loop returns (`backend.rs:279-280`).
  So "our destroy callback ran" does **not** show that a `FUSE_DESTROY`
  message arrived. It only shows that the channel closed and the loop ended.
  **(verified from source)**
- Which side closed the channel (an FSModule crash, fskitd deactivating the
  volume, or an outside `umount`/DiskArbitration eject) is not settled.
  The crash reports and `macfuse log stream` above will answer it.
  *(unverified)*

### What makes the kernel send FORGET, and can a daemon encourage it?

- **FSKit side:** "FSKit guarantees that for every `FSItem` returned by the
  volume, a corresponding reclaim operation occurs after the upper layers no
  longer reference that item"
  ([`reclaimItem(_:replyHandler:)`](https://developer.apple.com/documentation/fskit/fsvolume/commonoperations/reclaimitem(_:replyhandler:))).
  macFUSE 5.3.1 turns these into `FUSE_BATCH_FORGET` ("Improve performance
  when enumerating directories by using `FUSE_BATCH_FORGET`",
  [5.3.1 notes](https://github.com/macfuse/macfuse/releases/tag/macfuse-5.3.1)).
  **(verified)** "Upper layers no longer reference" means the kernel's vnode
  cache has let the vnode go. That happens under vnode-cache pressure, not on
  a timer, so thousands of live node ids during a build are normal. The same
  holds on Linux, where FORGET follows inode eviction, not dentry timeout.
  *(inferred)*
- **`entry_timeout`/`attr_timeout`** are documented as "the timeout in seconds
  for which name lookups will be cached" (`include/fuse.h`, `struct
  fuse_config`,
  [macfuse/library](https://github.com/macfuse/library/blob/00e9044/include/fuse.h#L159-L163)).
  They govern revalidation (more LOOKUP/GETATTR traffic), not vnode lifetime.
  Dropping them to 0–1 s would raise op volume and would not produce FORGETs.
  *(inferred)* Since 5.4.0 the FSKit module caches item attributes "based on
  the validity timeouts returned by the file system server"
  ([5.4.0 notes](https://github.com/macfuse/macfuse/releases/tag/macfuse-5.4.0)).
  Before 5.4.0 the FSKit backend did not use them for attribute caching at
  all. **(verified)**
- **`fuse_lowlevel_notify_inval_entry` / `fuse_invalidate_path` do not work on
  FSKit.** The wiki's FSKit limitations list: "The FUSE notification API is
  not supported, yet"
  ([FUSE Backends](https://github.com/macfuse/macfuse/wiki/FUSE-Backends)).
  The maintainer on [macfuse#1165](https://github.com/macfuse/macfuse/issues/1165)
  (open) says: "The unsolicited notification FUSE API has not been implemented
  for the FSKit backend, yet … due to FSKit still lacking support for
  processing 'external' file system events." **(verified)** Idea 2 in #29
  therefore buys nothing on FSKit. On the *kernel* backend, INVAL_ENTRY was a
  no-op from 4.9.1 until 5.3.1
  ([macfuse#1160](https://github.com/macfuse/macfuse/issues/1160)).
- **libfuse's own node table has no cap either.** Nodes stay until FORGET,
  or longer with `-o remember=T` / `noforget`
  ([`include/fuse.h` `remember`](https://github.com/macfuse/library/blob/00e9044/include/fuse.h#L193-L203)).
  At ~12K nodes, memory is not a concern. **(verified; the memory figure is
  (inferred))**

## 2. Relative-target `symlink()` returning ENXIO (#25)

- **No macFUSE issue or release note mentions symlink target handling on
  FSKit.** The tracker search covered `FSKit`, `ENXIO`, `Device not
  configured` and `symlink`. **(verified absence)**
- **What ENXIO means in macFUSE:** "The 'Device not configured' (`ENXIO`)
  error indicates that the device has been marked as dead due to a timeout or
  error" (maintainer, [macfuse#1136](https://github.com/macfuse/macfuse/issues/1136),
  about the kernel backend). On XNU, operations on a revoked vnode, or on a
  child whose `v_parent` was revoked, also fail with `ENXIO`
  ([macfuse#1160 comment](https://github.com/macfuse/macfuse/issues/1160)).
  **(verified)**
- **Where the target string goes:** Apple's
  [`createSymbolicLink(named:inDirectory:attributes:linkContents:replyHandler:)`](https://developer.apple.com/documentation/fskit/fsvolume/operations/createsymboliclink(named:indirectory:attributes:linkcontents:replyhandler:))
  passes `linkContents` as an opaque `FSFileName` and documents only `EEXIST`
  as a specified error. In macFUSE's libfuse3, `fuse_lib_symlink$DARWIN` calls
  the file system's `symlink`, then `lookup_path$DARWIN` → `getattr` on the new
  link
  ([`lib/fuse.c:3886-3889`](https://github.com/macfuse/library/blob/00e9044/lib/fuse.c#L3886-L3889)).
  turnkey's `fuse_getattr` uses `symlink_metadata`, so it does not follow the
  (dangling-from-the-backing-dir) relative target. Nothing on the userspace
  side can produce ENXIO for a relative target. **(verified from source)**
- **Conclusion:** the ENXIO most plausibly comes from the volume or parent
  vnode already being dead or revoked when buck2's materializer issued the
  call. That can be the tail of the #29 teardown or an FSModule error path
  in 5.2.0, rather than anything about relative targets. *(inferred)*

## 3. Behaviour a single Linux + macOS implementation has to paper over

All **(verified)** unless marked otherwise.

| Area | macFUSE FSKit behaviour | Source |
|---|---|---|
| Node ids | Up to 5.3.0 the FSKit backend used the inode number as the FUSE node id and sent GETATTR for READDIR `ino`s never returned by LOOKUP. From 5.3.1 the node id + generation is the handle, and READDIR entries get a `FUSE_LOOKUP`. | [#1166](https://github.com/macfuse/macfuse/issues/1166), [#1167](https://github.com/macfuse/macfuse/issues/1167), [5.3.1](https://github.com/macfuse/macfuse/releases/tag/macfuse-5.3.1) |
| readdir | High-level libfuse without `use_ino` reports `FUSE_UNKNOWN_INO` in dirents ([`fuse.c:4447-4452`](https://github.com/macfuse/library/blob/00e9044/lib/fuse.c#L4447-L4452)). On ≤5.3.0 FSKit would treat that as a node id. The maintainer says macFUSE "expects directory listings to be stable (as in sorted)" across concurrent readers, and non-zero readdir offsets are more frequent under FSKit. 5.4.0 validates cookies/verifiers. | [#1131](https://github.com/macfuse/macfuse/issues/1131), [5.4.0](https://github.com/macfuse/macfuse/releases/tag/macfuse-5.4.0) |
| Darwin attrs | With Darwin extensions, ops take `fuse_darwin_attr` (192 B) rather than `struct stat` (144 B). `utimens` gets three timestamps (atime, mtime, backup time). `crtime` was renamed `btime` in 5.1.0. | [5.1.0](https://github.com/macfuse/macfuse/releases/tag/macfuse-5.1.0) |
| setattr | `fuse_lib_setattr$DARWIN` calls `op.setattr` first. Any return other than `-ENOSYS` counts as "handled as a whole" and skips chmod/chown/truncate/utimens. Since 5.4.0 FSKit skips SETATTR when "no supported attributes remain". | [`fuse.c:3534-3549`](https://github.com/macfuse/library/blob/00e9044/lib/fuse.c#L3534-L3549), [5.4.0](https://github.com/macfuse/macfuse/releases/tag/macfuse-5.4.0) |
| rename | ≤5.3.3 FSKit expected `renamex_np(2)` support; 5.4.0 falls back and uses `RENAME_EXCL` only when `FUSE_DARWIN_CAP_RENAME_EXT` is negotiated. libfuse3 rename decoding was buggy in 5.2.0 (with the cap) and ≤5.3.3 (without it). | [#1191](https://github.com/macfuse/macfuse/issues/1191), [5.2.0](https://github.com/macfuse/macfuse/releases/tag/macfuse-5.2.0), [5.4.0](https://github.com/macfuse/macfuse/releases/tag/macfuse-5.4.0) |
| exec | XNU does not open files it execs. On ≤5.3.3 FSKit, `exec()` gave EIO until the file had been read; fixed in 5.4.0 by opening a handle on demand. | [#1181](https://github.com/macfuse/macfuse/issues/1181) |
| Writes | 5.3.3 (not bisected earlier) delivered **1–14 byte writes zero-filled**; fixed in 5.4.0. | [#1187](https://github.com/macfuse/macfuse/issues/1187) |
| Short reads | A 0-byte reply to a non-zero READ crashed the module (≤5.3.3). On 5.4.0 `read(2)` instead reports success without filling the buffer, an FSKit-side issue the maintainer thinks macOS 27 fixes (open). | [#1188](https://github.com/macfuse/macfuse/issues/1188), [#1196](https://github.com/macfuse/macfuse/issues/1196) |
| Opens | "Files are always opened in read/write mode." | [FUSE Backends](https://github.com/macfuse/macfuse/wiki/FUSE-Backends) |
| Caller context | `fuse_context_t` is unavailable. 5.4.0 forwards the caller's effective uid/gid on macOS 27 only. | [FUSE Backends](https://github.com/macfuse/macfuse/wiki/FUSE-Backends), [5.4.0](https://github.com/macfuse/macfuse/releases/tag/macfuse-5.4.0) |
| Mount options | "Most mount options previously handled by the kernel are not implemented, yet." `noappledouble` is confirmed unimplemented, and turnkey passes it (`backend.rs:190`), so `._` files may appear in buck-out. `local` is experimental. | [FUSE Backends](https://github.com/macfuse/macfuse/wiki/FUSE-Backends), [#1162](https://github.com/macfuse/macfuse/issues/1162), [5.1.0](https://github.com/macfuse/macfuse/releases/tag/macfuse-5.1.0) |
| Notifications | Not implemented on FSKit (see §1). | [#1165](https://github.com/macfuse/macfuse/issues/1165) |
| Server death | The volume hangs until the module process or `fskitd` is killed (open). | [#1201](https://github.com/macfuse/macfuse/issues/1201) |
| Mount point | The wiki says mount points outside `/Volumes` are unsupported, but that page was last edited 2025-10-30 (pre-5.1). turnkey mounts under `/tmp` and it works (#25), so treat the wiki line as stale. *(inferred)* 5.4.0 creates missing mount points; paths with spaces or `%`-encoded characters fail until 5.4.1. | [FUSE Backends](https://github.com/macfuse/macfuse/wiki/FUSE-Backends), [#1198](https://github.com/macfuse/macfuse/issues/1198) |
| Negative lookups | turnkey had to force `negative_timeout = 0` because FSKit followed up the `ino=0` negative entry with a GETATTR on node 0 (`operations.rs:485-493`). This is the same identifier shortcut fixed in 5.3.1, so it may no longer be needed. *(inferred, re-test)* | [#1167](https://github.com/macfuse/macfuse/issues/1167) |

## 4. What changed since 5.2.0, and which version to require

Releases after 5.2.0 (2026-04-09):
[5.3.0](https://github.com/macfuse/macfuse/releases/tag/macfuse-5.3.0),
[5.3.1](https://github.com/macfuse/macfuse/releases/tag/macfuse-5.3.1),
[5.3.2](https://github.com/macfuse/macfuse/releases/tag/macfuse-5.3.2) (all
pre-release),
[5.3.3](https://github.com/macfuse/macfuse/releases/tag/macfuse-5.3.3)
(stable, 2026-07-04),
[5.4.0](https://github.com/macfuse/macfuse/releases/tag/macfuse-5.4.0)
(latest, 2026-09-07). The ones that matter to a buck2 workload:

- **5.3.0:** a new `MFChannel` message API, zero-copy reads and writes, "up to
  15 times faster" reads, and the volume-statistics race crash fixed.
- **5.3.1:** node id + generation as the handle (#1166, #1167), LOOKUP after
  READDIR, an open-file-handle bug fixed, `FUSE_BATCH_FORGET`, and kext
  INVAL_ENTRY fixed (#1160).
- **5.3.3:** daemonisation and teardown changes, and interruptible
  `MFChannel` receives. It also regressed sshfs daemonising
  ([#1180](https://github.com/macfuse/macfuse/issues/1180), open), which is
  irrelevant to turnkey because it runs in the foreground from a thread.
  *(inferred)*
- **5.4.0:** the small-write corruption (#1187), exec EIO (#1181) and 0-byte
  READ crash (#1188) fixes, TTL-based attribute caching, rename fixes, lock
  ordering fixes, and fatal messages in crash reports.
- **Announced, not yet released:** 5.4.1 (mount points with spaces,
  [#1198](https://github.com/macfuse/macfuse/issues/1198)) and 5.5.0 (unsigned
  Intel binaries SIGILL at mount,
  [#1205](https://github.com/macfuse/macfuse/issues/1205)).

**Recommendation: require ≥ 5.4.0.** Silent data corruption (#1187) and exec
EIO (#1181) alone rule out everything older for a build tool that writes small
outputs and executes what it builds. Check the version at mount time.
turnkey already detects the FSKit module and its version in `backend.rs`.
libfuse3 is linked from the installed macFUSE (`nix/packages/turnkey-composed.nix`),
so library and module move together. But `bindings.rs` hard-codes
`fuse_config` layout offsets, so bump the tested version and the bindings'
layout assertions together. *(inferred)*

### The local libfuse patch is still needed in 5.4.0

The stack overrun that `nix/patches/macfuse-libfuse3/0001-darwin-attr-overflow-fix.patch`
fixes is still there at the 5.4.0 `Library-3` commit:

- `open_auto_cache` still allocates a vanilla `struct stat` and calls
  `fuse_fs_getattr`
  ([`lib/fuse.c:4130-4159`](https://github.com/macfuse/library/blob/00e9044/lib/fuse.c#L4130-L4159));
- so does `hidden_name`
  ([`lib/fuse.c:2829-2859`](https://github.com/macfuse/library/blob/00e9044/lib/fuse.c#L2829-L2859)).

**(verified)** Both paths are opt-outs:

- `open_auto_cache` runs only with `auto_cache`, which turnkey sets
  (`operations.rs:495`);
- `hidden_name` runs only when `hard_remove` is 0 (the libfuse default) and an
  open file is unlinked.

Setting `auto_cache = 0` and `hard_remove = 1` avoids the overrun with the stock
dylib. *(inferred from source; re-test)* The patch has not been filed upstream
yet (per its README), and filing it remains worthwhile.

## 5. Turnkey-side findings from this read

- **`fuse_setattr` swallows every SETATTR.** It returns 0 and does nothing
  (`operations.rs:788-804`). libfuse treats that as "handled as a whole" and
  skips the per-attribute handlers
  ([`fuse.c:3534-3549`](https://github.com/macfuse/library/blob/00e9044/lib/fuse.c#L3534-L3549)).
  So `chmod` (including buck2 making outputs executable), size truncation and
  `utimens` are dropped silently, and the real `fuse_chmod`/`fuse_truncate`
  handlers never run. **(verified from source)** Fix: apply mode, size and
  times in `fuse_setattr` directly, or return `-ENOSYS` from it for the bits
  it does not handle, and implement `utimens`.
- `fuse_loop_mt(fuse, 0)` binds to `fuse_loop_mt_31` with `clone_fd = 0`
  (`bindings.rs:527-528`). That is fine, but it cannot set `max_idle_threads`.
  Not a known factor in any failure.

## Concrete things to try

**For the force-unmount (#29):**

1. Upgrade to macFUSE 5.4.0 and re-run the #29 build before anything else.
2. During the run, keep
   `/Library/Filesystems/macfuse.fs/Contents/Resources/macfuse.app/Contents/MacOS/macfuse log stream > /tmp/fskit.log`
   going. After the unmount, check
   `~/Library/Logs/DiagnosticReports/io.macfuse.app.fsmodule.macfuse-*.ips`
   and fskitd reports. That tells an FSModule crash apart from a deliberate
   deactivate.
3. Log whether `FUSE_DESTROY` actually arrived, not just that `destroy` ran.
4. Drop ideas 1 and 2 from #29:
   - short `entry_timeout` only adds LOOKUP/GETATTR traffic;
   - notifications are unimplemented on FSKit.
5. If it still reproduces on 5.4.0, reduce it with the stock
   [`LoopbackFS-libfuse3-C`](https://github.com/macfuse/demo) demo, as
   macfuse#1187 and #1188 did, and file upstream with the `.ips`.

**For the ENXIO symlink (#25):**

1. On 5.4.0, `ln -s ../../../x <mount>/buck-out/…/link` by hand on a fresh
   mount. Then do the same on the stock LoopbackFS demo. This separates
   "relative symlink" from "volume already dead".
2. Check whether the ENXIO in a buck2 run coincides with the #29 teardown.
   Correlate the time with `macfuse log stream`.
3. Fix `fuse_setattr` and switch to `auto_cache = 0` / `hard_remove = 1`
   before re-benchmarking, so the benchmark isn't measuring dropped chmods or
   the patched-vs-stock lib.
