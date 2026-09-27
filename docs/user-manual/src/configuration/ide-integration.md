# IDE Integration

This guide explains how to configure your IDE to work seamlessly with Turnkey's automatic dependency synchronization.

## Overview

Turnkey can automatically update `rules.star` files when you modify source code imports. While this happens automatically when running `tk build`, you can also configure your IDE to trigger sync on file save for immediate feedback.

## VS Code

### Run on Save Extension

Install the [Run on Save](https://marketplace.visualstudio.com/items?itemName=emeraldwalk.RunOnSave) extension, then add to your workspace `.vscode/settings.json`:

```json
{
  "emeraldwalk.runonsave": {
    "commands": [
      {
        "match": "\\.(go|rs|py|ts|tsx|sol)$",
        "cmd": "tk rules sync --quiet ${fileDirname}"
      }
    ]
  }
}
```

This runs `tk rules sync` on the directory containing the modified file whenever you save a source file.

### Task-based Approach

Alternatively, create a VS Code task in `.vscode/tasks.json`:

```json
{
  "version": "2.0.0",
  "tasks": [
    {
      "label": "Sync rules.star",
      "type": "shell",
      "command": "tk rules sync",
      "presentation": {
        "reveal": "silent",
        "panel": "shared"
      },
      "problemMatcher": []
    }
  ]
}
```

Then bind it to a keyboard shortcut in `keybindings.json`:

```json
{
  "key": "ctrl+shift+s",
  "command": "workbench.action.tasks.runTask",
  "args": "Sync rules.star"
}
```

## JetBrains IDEs (IntelliJ, GoLand, PyCharm, etc.)

### File Watchers

1. Go to **Settings** > **Tools** > **File Watchers**
2. Click **+** to add a new watcher
3. Configure:
   - **Name**: `Turnkey Rules Sync`
   - **File type**: `Go files` (or your language)
   - **Scope**: `Project Files`
   - **Program**: `tk`
   - **Arguments**: `rules sync --quiet $FileDir$`
   - **Output paths to refresh**: `$FileDir$/rules.star`
   - **Working directory**: `$ProjectFileDir$`

4. Under **Advanced Options**:
   - Check: "Trigger the watcher on external changes"
   - Uncheck: "Auto-save edited files to trigger the watcher"

### External Tools

Alternatively, set up an external tool:

1. Go to **Settings** > **Tools** > **External Tools**
2. Click **+** to add:
   - **Name**: `Sync rules.star`
   - **Program**: `tk`
   - **Arguments**: `rules sync`
   - **Working directory**: `$ProjectFileDir$`

3. Assign a keyboard shortcut in **Keymap** settings

## Neovim

Add to your Neovim configuration:

```lua
-- Auto-run tk rules sync on save for supported file types
vim.api.nvim_create_autocmd("BufWritePost", {
  pattern = { "*.go", "*.rs", "*.py", "*.ts", "*.tsx", "*.sol" },
  callback = function()
    local file_dir = vim.fn.expand("%:p:h")
    vim.fn.jobstart({ "tk", "rules", "sync", "--quiet", file_dir }, {
      on_exit = function(_, code)
        if code ~= 0 then
          vim.notify("tk rules sync failed", vim.log.levels.WARN)
        end
      end,
    })
  end,
})
```

## Emacs

Add to your Emacs configuration:

```elisp
(defun turnkey-sync-rules ()
  "Run tk rules sync on the current file's directory."
  (when (and buffer-file-name
             (string-match-p "\\.\\(go\\|rs\\|py\\|ts\\|tsx\\|sol\\)$" buffer-file-name))
    (let ((default-directory (file-name-directory buffer-file-name)))
      (start-process "tk-rules-sync" nil "tk" "rules" "sync" "--quiet" "."))))

(add-hook 'after-save-hook #'turnkey-sync-rules)
```

## Configuration Options

### Module Options

Rules sync is configured through turnkey's Buck2 options in your
`flake.nix`, which generate the `[rules]` section of `.turnkey/sync.toml`
(a generated file: don't edit it):

```nix
turnkey.toolchains.buck2.rules = {
  enabled = true;    # Enable rules.star sync (default: false)
  autoSync = true;   # Auto-sync before tk build (default: true)
  strict = false;    # Fail if rules would change - for CI (default: false)
};
```

Sync finds each language's internal targets from its own manifest
(`go.mod`, `Cargo.toml`, the uv workspace) and uses turnkey's deps cells
(`godeps`, `rustdeps`, `pydeps`, `jsdeps`, `soldeps`). The platforms it
resolves deps for come from `buck2.platforms` (see
[Platform-Conditional Deps](#platform-conditional-deps)).

### Command Line Options

```bash
tk rules sync              # Sync only stale files (git-based detection)
tk rules sync --force      # Force sync all files
tk rules sync --verbose    # Show detailed output
tk rules sync --dry-run    # Show what would change without writing
tk rules check             # Check if any files need sync (exit 1 if stale)
tk rules check --force     # Check all files, not just git-changed
```

## Staleness Detection

Turnkey uses intelligent staleness detection to minimize unnecessary work:

1. **Git-based** (default): Only checks directories with uncommitted source file changes
2. **Mtime-based** (with `--force`): Compares modification times of source files vs rules.star

This means `tk rules sync` is nearly instant in most cases, making it suitable for on-save hooks.

## Preservation Markers

If you have manual dependencies that shouldn't be auto-managed, use preservation markers:

```python
go_binary(
    name = "my-app",
    srcs = ["main.go"],
    deps = [
        # turnkey:auto-start
        "godeps//vendor/github.com/google/uuid:uuid",
        # turnkey:auto-end
        # turnkey:preserve-start
        # Manual override for special case
        "//special:dep",
        # turnkey:preserve-end
    ],
)
```

Dependencies between `preserve-start` and `preserve-end` markers are never modified by sync.

### Opting a Target Out

To keep sync away from one target entirely, for example to work around a
problem, put a `# turnkey:no-sync` comment on its own line right before the
rule:

```python
# Links a hand-built native library sync knows nothing about
# turnkey:no-sync
rust_library(
    name = "my-lib-native",
    deps = _COMMON_DEPS + ["//third-party/native:lib"],
)
```

Sync never changes an opted-out target. `tk rules sync -v` and
`tk rules check -v` list them as `OPTED OUT:`.

## Platform-Conditional Deps

Some deps are only needed on some platforms. Sync resolves every target's
deps on each platform the project builds for, so what it writes is the same
whichever machine runs it. The platforms are `buck2.platforms`, the flake's
`systems` by default:

```nix
turnkey.toolchains.buck2.platforms = [ "x86_64-linux" "aarch64-darwin" ];
```

They reach sync through the `[conditions]` section of `.turnkey/sync.toml`,
in Buck2's names:

```toml
[conditions]
settings = "toolchains//conditions"

[[conditions.platforms]]
os = "linux"
cpu = "x86_64"
```

Deps every platform needs are written as a plain list. The others are
written as a `select()` after it:

```python
rust_library(
    name = "my-lib",
    deps = [
        # turnkey:auto-start
        "rustdeps//vendor/libc:libc",
        # turnkey:auto-end
    ] + select({
        "config//os:linux": ["rustdeps//vendor/fuser:fuser"],
        "config//os:macos": [],
    }),
)
```

- A key is the smallest one that says exactly where the deps apply:
  `config//os:<os>` when they differ only by OS (or `config//cpu:<cpu>` by
  CPU alone), otherwise one of the toolchains cell's `config_setting`s
  combining both, `toolchains//conditions:<os>-<cpu>`, one per platform.
- Every platform gets a branch, empty if it needs nothing more, and there is
  no `DEFAULT`: building for a platform that isn't listed fails instead of
  silently missing deps.
- Sync reads this form back and owns all of it. The `turnkey:auto` and
  `turnkey:preserve` markers apply to the plain list only. A change to one
  branch rewrites only that branch.
- A target whose deps don't depend on the platform keeps a plain list.

A `select()` sync can't read (its keys aren't `config//os:*`,
`config//cpu:*`, `toolchains//conditions:*` or `DEFAULT`, or its values
aren't lists of labels) makes the target unreadable, like any other
expression.

## Where Deps Come From

- **Rust**: the crate's `Cargo.toml`, not its sources. `[dependencies]` go to
  library and binary targets; test targets get `[dependencies]` plus
  `[dev-dependencies]`. `workspace = true` entries are resolved against the
  root `Cargo.toml`, a workspace member maps to its own target, and any other
  crate maps to `rustdeps//vendor/<package>:<package>` (a renamed dependency
  maps to its `package`). An existing label that pins a version
  (`rustdeps//vendor/tokio@1.50.0:tokio`) satisfies the unversioned one.
  A target-specific table (`[target.'cfg(...)'.dependencies]`, or a target
  triple) applies on the platforms its spec holds on, so its deps are
  [platform-conditional](#platform-conditional-deps): a dep every platform
  gets is a plain dep, one no platform gets is dropped. `cfg()` supports
  `target_os`, `target_family` (`unix`), `target_arch`,
  `target_pointer_width`, `target_env`, `target_vendor`, `target_endian`
  and `all`/`any`/`not`, as the rustdeps cell evaluates it for vendored
  crates. Build dependencies are not synced: sync reports them and leaves
  any existing dep on them alone. See [Rust Features](#rust-features) for
  features, optional dependencies and dependencies on a member's variant.
- **Python**: the imports found in the sources. An import of a package that a
  uv workspace member provides (`turnkey.cfg`, `from turnkey import cfg`)
  maps to that member's target. The packages come from the members listed in
  the root `pyproject.toml`'s `[tool.uv.workspace]` and their source layout,
  so a downstream namespace such as `acme.*` works the same way. Any other
  import maps to `pydeps`.
- **TypeScript**: the npm packages imported by the sources, written to the
  target's `npm_deps` as the jsdeps cell's `jsdeps//:<package>` aliases, plus
  each one's `@types/...` package when `js-deps.toml` has it. The target's
  `deps` (other TypeScript targets) are not synced.
- **Other languages**: the imports found in the sources, mapped to targets.

Sync never removes a dep it can't account for:

- If a target has imports sync can't map to a target (reported as `unmapped
  import`), its deps are incomplete, so sync adds what it resolved but removes
  nothing, and prints a `KEPT:` line naming the deps it kept and why.
- A target whose `deps` is an expression (`_DEPS`, `_COMMON_DEPS + [...]`)
  rather than a list of labels is not synced. Unless `# turnkey:no-sync` opts
  it out, `tk rules sync` and `tk rules check` report it as `UNREADABLE:`,
  and it makes `tk rules check` and strict mode fail: write its deps as a
  list of labels, or opt it out. (The sync before a build doesn't report
  it.)
- Deps are only added or removed, never reordered.

## Rust Features

A Rust target builds what Cargo would, and sync keeps it that way: it
writes the target's `features` (the literal list the prelude passes to
rustc, one `--cfg feature="..."` each) as well as its `deps`.

- A **primary** target, one that sets neither of the attributes below,
  builds what `cargo build -p <crate>` builds: the crate's `default`
  features, expanded. An optional dependency is a dep only when an enabled
  feature activates it (`dep:x`, an implicit feature, `x/feat`).
- A **variant** asks for features in Cargo's terms, on two attributes of
  turnkey's prelude Rust rules that rustc never sees:

  ```python
  rust_library(
      name = "composition-full",
      crate = "composition",
      cargo_features = ["watcher"] + select({
          "config//os:linux": ["fuse"],
          "config//os:macos": ["fuse-t"],
      }),
      # default_features = False,  # as Cargo's default-features
  )
  ```

  Sync expands the request as Cargo would for a dependency asking for those
  features (`dep:x`, `x/feat`, weak `x?/feat`, feature-to-feature, and
  `default` unless `default_features = False`), and writes the `features`
  and `deps` it gives, per platform when the request is a `select()`.
- `features` is literal: `default` is written only when listed, and Buck2
  adds nothing implicit.
- A **dependency on a workspace member that asks for features** (its own
  `features = [...]` with the `[workspace.dependencies]` entry's, those its
  crate's features forward to it, and the member's defaults unless
  `default-features = false`) maps to the member's `rust_library` whose
  request enables exactly the same features, per platform. If none or
  several do, the dependency is reported as an unmapped import naming the
  features it needs, and nothing is removed.

A crate that doesn't build under Buck2 is a bug to fix in the rustdeps
cell, not a reason to make a target a variant.

rust-analyzer sees a `select()`'d `features` through the host's branch.

## Troubleshooting

### Sync not running

1. Ensure `deps-extract` is in your PATH (built with `cargo install --path src/rust/deps-extract`)
2. Check that `[rules] enabled = true` in `.turnkey/sync.toml`
3. Verify the file type is supported (Go, Rust, Python, TypeScript, Solidity)

### Sync too slow

1. Use the default staleness detection (don't use `--force` in on-save hooks)
2. Target a specific directory: `tk rules sync src/cmd/myapp`

### Wrong dependencies detected

1. Check your `*-deps.toml` files are up to date (run `tk sync`)
2. Verify internal prefix configuration in sync.toml
3. Run `tk rules sync --verbose` to see what's being detected
