# The parity harness

`nix run .#parity -- <tool>` builds the Go package and the Rust package of
a tool, runs both on the same inputs, and diffs what they produce. It is
the switch gate for every port in
[#207](https://github.com/firefly-engineering/turnkey/issues/207), as
decided on [#204](https://github.com/firefly-engineering/turnkey/issues/204),
and the porting session's development loop until then. It is run by hand:
it is not a flake check and has no CI job.

It is temporary. Each tool's switch deletes that tool's case file, and the
`tk` switch deletes this directory and `apps.parity` in `flake.nix`.

```bash
nix run .#parity -- --list                         # the tools and their cases
nix run .#parity -- <tool>                         # Go package against Rust package
nix run .#parity -- --self-check <tool>            # Go package against itself
nix run .#parity -- <tool> --case <case>
nix run .#parity -- <tool> --rust target/debug/<tool>  # a cargo build
nix run .#parity -- <tool> --keep                  # keep the scratch dir to look at
```

It exits 0 when every case is clean, 1 when a case differs, and 2 on a
usage or case-file error. A clean `--self-check` shows the case is
deterministic: the same build run twice gives the same result.

## What is compared

For each case, each side gets a fresh scratch root:

```
<root>/work   the case's inputs, copied; the default working directory
<root>/out    empty, for a tool that writes outside its project (a deps cell's $out)
```

Every copied entry is made writable and given the same mtime (2000-01-01),
so mtime staleness reads the same on both sides. Then the harness runs the
side's binary with the case's `args` and compares:

- **the exit code**, always, and against the case's `exit` when it sets one,
  so two sides failing the same way don't pass as a match;
- **the files written** under `<root>`, always: which entries each side
  added, changed or removed;
- **the content** of every file written, and of every declared output
  (a file, or every file under a directory), as `bytes` unless an output
  says otherwise;
- **stdout**, when an output declares `stream = "stdout"`;
- **the child processes**, when the case has stubs: the ordered call log.

stderr (logs, error messages) is never diffed; when a case differs, the
tail of each side's stderr is printed to be read by eye.

Before comparing, each side's own paths are rewritten to placeholders:
its scratch root to `<root>`, its private dir (stubs, `TMPDIR`) to
`<private>`, its package to `<pkg>`, and the hash of every
`/nix/store/<hash>-<name>` path to `<hash>`, so a cell built by the Rust
`buckgen` (a different derivation) compares equal to the Go one.

### Compare modes

| `compare`  | Compares                                                        |
| ---------- | --------------------------------------------------------------- |
| `bytes`    | the normalised bytes (the default, and the bar for generated files) |
| `toml`     | the parsed TOML (`tomllib`): key order and formatting don't count |
| `json`     | the parsed JSON                                                 |
| `starlark` | Python's AST of the file (Starlark's syntax is a subset of Python's): layout and comments don't count |

A parsed mode is allowed for an output only once the port's ticket says
why byte-identical output is out of reach (#204).

### Stubs: `tw`, `tk` and other tools that launch processes

A case's `stubs` names the commands to replace. The harness writes one
recording stub per name into a directory that comes first on the `PATH`
it sets on the launched tool; after it come only coreutils, `go`, `git`
and `python3` from nixpkgs and turnkey's `deps-extract` (for what isn't
stubbed, like `godeps-gen`'s `go list` or rules sync's extractor).

Each call to a stub is recorded: the command, its argv, its cwd, and the
environment variables that differ from the environment the harness gave
the tool (so, the ones the tool set, changed or unset). The stub then
replays the first response whose `args` (exact) or `args_prefix` match,
or exits 0 silently when none does. A response can also write files,
which is how a stubbed `go get` changes `go.mod` for `tw` to notice.

The tool's environment is not the caller's: it is `HOME`, `TMPDIR`
(private to the side), `LANG`, `SSL_CERT_FILE`, `PATH`, what the case's
`inherit_env` copies from the caller, and the case's `env`. So a dev
shell's `TURNKEY_REAL_GO` can't route `tw` around the stubs.

## Case files

One file per tool, `cases/<tool>.toml`; the file name is the tool name
`nix run .#parity -- <tool>` takes. Inputs too large to inline in it, which
nothing else in the tree has, go in `cases/<tool>/` (the harness only reads
the `.toml` files directly under `cases/`), and go away with the case file.
The example below is godeps-gen's, whose case file its switch (#211)
deleted.

```toml
[tool]
go = "godeps-gen"        # the flake package of the Go version (packages.<name>)
rust = "godeps-gen-rs"   # the flake package of the Rust port; absent until it exists
bin = "godeps-gen"       # the program under <package>/bin; default: the tool name

[[cases]]
name = "e2e-greenfield-go"                 # unique within the file
description = "optional, for the reader"
inputs = { work = "e2e/fixtures/greenfield-go" }   # scratch path = path in turnkey's tree
# exclude = ["node_modules"]               # names skipped when copying, besides .git, .jj, ...
files."work/.turnkey/sync.toml" = "..."    # files laid over the inputs
cwd = "work"                               # where the tool runs (default work)
args = ["--no-prefetch", "-o", "go-deps.toml"]   # after the binary; {root}, {work}, {out} expand
env = { GOTOOLCHAIN = "local" }            # {root}, {work}, {out} expand here too
inherit_env = ["GOMODCACHE", "GOPROXY"]    # copied from the caller when set
exit = 0                                   # the exit code both sides must have
timeout = 600                              # seconds
outputs = [
  { path = "work/go-deps.toml" },                    # compare = "bytes"
  { stream = "stdout", compare = "toml" },
  { path = "out/cell" },                             # a directory: every file under it
  { path = "out/cell/rules.star", compare = "starlark" },  # the most specific path wins
]

[[cases.stubs.go]]                         # responses of the `go` stub, first match wins
args_prefix = ["get"]
exit = 0
stdout = "..."
stderr = "..."
write."work/go.mod" = "module example.com/x\n"

[[cases.stubs.godeps-gen]]                 # no keys: exit 0, no output
```

Inputs come from the turnkey source tree the flake was evaluated from: the
Go tests' `testdata/`, `e2e/fixtures/*`, the Go coverage fixture, or `"."`
for turnkey's own repository. An absolute `/nix/store/...` path is read
from the store as it is, e.g. a Go module zip's source (fixed-output, so
its path is the same everywhere) or GOROOT's trees.

### Tools that run inside a deps-cell derivation

`buckgen` (`nix/lib/deps-cell/adapters/go.nix`) and `pydeps-cell`
(`nix/lib/deps-cell/adapters/python.nix`) run inside a derivation, but
each is one command on files: the case reproduces that command, with the
cell's `$out` as `{out}`. Put the derivation's inputs in `inputs` or
`files` (`pydeps-cell` reads `$out/pydeps-cell.json`, so that one is
`files."out/pydeps-cell.json"`), and declare `out` (the cell contents) and
any `--targets-out` / `--imports-out` file as outputs:

```toml
[[cases]]
name = "turnkey-python-deps"
inputs = { "work/python-deps.toml" = "python-deps.toml" }
files."out/pydeps-cell.json" = '{"platforms": [...], "python_version": "3.13"}'
args = ["{out}", "{work}/python-deps.toml"]
outputs = [{ path = "out" }]
```

## Wiring a port

1. Add the Rust package, as #207 says: `nix/packages/<tool>-rs.nix`, built
   with `rustPlatform.buildRustPackage` over `cargoLib.workspaceProjection`
   like `nix/packages/rustdeps-gen.nix`, wrapped like the Go package (the
   same `PATH` additions), with its binary named after the tool. Expose it
   as `packages.<tool>-rs` in `flake.nix`, outside the shell.
2. Point the case file at it: `rust = "<tool>-rs"` under `[tool]`. Add
   the case file first if the tool has none, and add the cases the port
   needs.
3. `nix run .#parity -- --self-check <tool>` must be clean (the cases are
   deterministic), then `nix run .#parity -- <tool>` must be clean before
   the switch. While porting, `--rust target/debug/<tool>` skips the Nix
   build.
4. The switch commit deletes `cases/<tool>.toml`.

## Known caveats

- Go's `os/exec` sets `PWD` in a child's environment when it runs it in a
  directory (`tw`'s post-commands, the syncer's generators). The call log
  records that, so a Rust port that doesn't set it too differs there.
- Only the `.drv` paths of the packages are embedded in the harness. If
  `nix run` reuses a cached evaluation and the `.drv` files were garbage
  collected since, the build fails: run it again with `--no-eval-cache`, or
  after any change to the flake.
