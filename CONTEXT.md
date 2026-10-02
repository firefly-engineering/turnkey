# Turnkey

Turnkey is a toolchain-as-code framework for Nix flakes. It turns declarative toolchain and dependency declarations into development shells and Buck2 cells for consuming repositories.

## Language

### Buck2

**Pinned buck2 release**:
The one buck2 release a turnkey revision ships: the binary, the prelude built with it, and the buck2 source revision they come from, always moved together. Consumers get it by choosing a turnkey revision, never by declaring buck2 themselves.
_Avoid_: buck2 version (as a consumer setting), declared buck2, supported buck2 versions

### Rules sync

**Conditional attribute**:
A target attribute whose value depends on the build configuration, written as a plain value or as `[...] + select({...})` over the platforms turnkey builds for. Its value in a configuration is the plain part followed by the branch that applies, each label once; in a configuration no branch applies to, the plain part alone. Rules sync, the language plug-ins and the cell generators all read and write it the same way.
_Avoid_: select attribute, variant (which is only the attributes a language reads to resolve deps)

**Language plug-in**:
One language's part of rules sync: which rule kinds are its, which files make its targets stale, which configuration dimensions a package's deps depend on, and what a package's deps resolve to in one configuration. It says what a target wants; rules sync alone decides what is written.
_Avoid_: mapper (the module that holds the plug-ins), language adapter

**Keep policy**:
Why rules sync leaves an existing dep in place that no plug-in wants: it is a same-package (`:name`) or preserved dep, an import couldn't be mapped so the wanted deps are incomplete, it is in the package of a dep sync doesn't own, or it stands for a wanted label, as a Rust dep pinning a crate's version does. The plug-in supplies the parts that are its language's.
_Avoid_: preserve (which is only the `turnkey:preserve` section)

### Dependencies

**Deps generator**:
The tool that reads one language's lock file and writes the deps file (`rust-deps.toml`, `go-deps.toml`, ...) that language's deps cell is built from. `tk sync` runs it from the language's sync rule. Its own code is the lock file parsing and the record it writes; the flags (`--output`, prefetching on unless `--no-prefetch`, `--no-cache`), the prefetching of Nix hashes and the file's header are the same for every generator, and live in deps-gen-kit.
_Avoid_: deps-gen tool, lockfile converter

**Fixup**:
What turnkey supplies for one dependency, in its own ecosystem's identity (a crate, a Python distribution, a Go module), in place of the dependency's own build step or to correct its source, so it builds under Buck2 without running that step.
_Avoid_: override, crate override, build-script shim

**Fixup set**:
A collection of fixups a repository brings as one unit, possibly spanning several languages: turnkey's built-ins, an organization's shared registry, a third party's, or the repository's own.
_Avoid_: fixup registry (for one set), overrides

**Overlay**:
The fields a fixup adds only on some platforms: those of one OS, one CPU, or one OS and CPU pair. An overlay is always declarative, never a build script.
_Avoid_: per-platform fixup, platform override

**Project root**:
The nearest directory, from where `tk` or `tw` runs, holding a `.buckconfig` or a `.turnkey/sync.toml`; a turnkey shell writes both there. Outside one, `tk` runs from the working directory and `tw` runs the tool untouched.
_Avoid_: repo root, workspace root

**Workspace projection**:
A language workspace narrowed to a subset of its members: the root manifest keeps only those members and the shared declarations they inherit, and the lock keeps only the packages they reach. Turnkey builds each of its own Nix-built tools from its projection, computed when Nix evaluates, so the tool is rebuilt only when its projection changes, not on every change to the repo's workspace. Internal to turnkey's Nix lib; each language needs its own projector.
_Avoid_: pruned workspace, pruned source (which name how it is made, not what it is)

**Wrapped tool**:
A language's native tool (`go`, `cargo`, `uv`, named by the language record) that the shell runs through `tw`. When one of its mutating subcommands changes the content of a file its wrapper rule watches, `tw` runs the rule's post-commands and syncs the rule's deps rule; it always exits as the tool did. Change is judged by content, where `tk sync` judges staleness by mtime.
_Avoid_: shim, tool wrapper (which is the shell script, not the tool)

### Deps cells

**Deps cell**:
The buck2 cell holding one language's third-party packages (`rustdeps`, `godeps`, ...). It is a real directory in the project, kept in line with its cell index.
_Avoid_: cell derivation, vendor cell

**Cell index**:
The Nix-built list of a deps cell's locked packages: each one's store path and target names, and the version aliases. A deps cell has exactly one current cell index.
_Avoid_: manifest, cell derivation

**Locked package**:
The unit a deps generator locks at one version, and a deps cell stores behind one store link: a crate, a Go module, a Python distribution, a Solidity package (an npm package or a git dependency). For Rust, Python and Solidity it is also one buck2 package. A Go module holds many Go packages, each its own buck2 package.
_Avoid_: dependency (which also names the edge), module (outside Go)

**Store link**:
A symlink in a deps cell to one locked package's store path, named after that store path. It is only ever created or deleted, never retargeted.
_Avoid_: crate symlink, vendor link

**Alias package**:
A package in a deps cell whose targets forward to the same-named targets of a buck2 package inside a store link: the store link's root for a crate, a Python distribution or a Solidity package, a subdirectory for a Go package. Version names (`anyhow@1.0.100`) and unversioned names (`anyhow`, `github.com/spf13/cobra`) are alias packages. Go, Python and Solidity lock one version per name, so they have only unversioned ones: one per Go package, one per distribution, one per Solidity package.
_Avoid_: alias symlink, version symlink

**Root package**:
The package at a deps cell's root, addressed as `<cell>//:<target>`. When a language has one (Solidity's per-package targets and its bundle), it names packages only through their alias packages, and it changes whenever the package list does. Rust and Go cells have none.
_Avoid_: root BUCK, cell BUCK file

**Package slice**:
What one package's build file needs from the build's resolution of its lock file, on each platform the project builds for: its features and its dependencies resolved to exact versions, each with the platforms it applies on, and the name the package's code uses for each dependency where the package name wouldn't give it. "The build's resolution" is what the language's own build tool would compile for that platform. It is not the lock file's resolution, which can be larger: Cargo's lock file keeps optional dependencies that a weak dependency feature only names. A Python distribution's slice is its requirements and those of the extras something asks it for, each kept on the platforms its marker holds on for the toolchain's Python, and each one the deps cell holds. A package's store path depends on its own slice and on nothing global.
_Avoid_: crate slice, feature slice, unified features (which is only the features part), lock file resolution

**Cell materialization**:
Bringing a deps cell in line with its cell index: adding missing store links, rewriting alias packages whose target changed, and removing what the index no longer names. What does it is the materializer.
_Avoid_: sync (which is `tk sync` running the deps generators), link update, retarget

### Testing

**Test result caching**:
Not re-running a test whose inputs are identical to those of a recorded run, and reporting the recorded result instead, whether the test last ran locally or remotely. Buck2 reuses build action results in both cases but test results only when a remote worker ran the test; closing that gap is the point.
_Avoid_: test caching, incremental testing, build caching (which is the existing, separate behaviour for build actions)

**Target determination**:
Choosing, from a change, which tests *could* be affected before anything is built. Distinct from test result caching, which decides after the build whether a test's inputs actually changed.
_Avoid_: test selection, affected tests

**Recorded result**:
A test's pass, stored under its result key so that a later run can report it instead of running the test. Only passes are ever recorded results; a failure is always run again.
_Avoid_: cached failure, test cache entry

**Result key**:
Everything the test process can see (its inputs, argv, declared environment, timeout, working directory and platform), plus the versions of buck2 and of the caching tool. Two runs share a recorded result only if their result keys are equal.
_Avoid_: cache key, test hash

**Reuse policy**:
The rules, kept outside the result key, for when a recorded result may be read or written: only passes, only under `tk`, not for targets labelled `no-test-cache`, not read when a re-run is forced, and written only into the local cache. `tk` chooses the mode for each run, the test-caching helper keeps `no-test-cache` targets out of caching, and the test runner only obeys the mode it is given, recording only passes. Changing the policy never splits the recorded results.

**Forced re-run**:
A `tk --rerun test` run: every test runs instead of reusing a recorded result, and fresh passes are still recorded into the local cache. Distinct from the `no-test-cache` label, which keeps a target out of caching altogether.
_Avoid_: `--no-test-cache` (the label's name), cache bypass

**Hit**:
A test run answered by a recorded result instead of running the test. Every hit is visible as such to the person running the tests. Its opposite is simply that the test ran.
_Avoid_: cached pass, cache hit (buck2 uses that for build actions too)

**Cache-safe rule**:
A test rule none of whose own behaviour lets a test read something outside its result key. Only a cache-safe rule has its targets' results recorded. Hazards that belong to one target, not to the rule, are fixed in that target or opt it out, and never make the rule unsafe.
