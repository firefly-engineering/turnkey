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

### sync.toml Settings

Configure rules sync behavior in `.turnkey/sync.toml`:

```toml
[rules]
enabled = true       # Enable rules.star sync (default: false)
auto_sync = true     # Auto-sync before tk build (default: true)
strict = false       # Fail if rules would change - for CI (default: false)

[rules.go]
internal_prefix = "//src/go"
external_cell = "godeps"

[rules.rust]
internal_prefix = "//src/rust"
external_cell = "rustdeps"

[rules.python]
internal_prefix = "//src/python"
external_cell = "pydeps"
```

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
