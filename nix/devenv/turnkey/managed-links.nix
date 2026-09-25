# The symlinks turnkey keeps in a project (.buckconfig, .turnkey/sync.toml,
# the cells under .turnkey/), and the one shell routine that maintains them.
#
# Both the shell's enterShell and direnv's use_turnkey run `ensure`, so a
# link is created, updated or left alone the same way from either: a path
# that exists and is not a symlink is the user's, and is never replaced.
{ lib }:

{
  # Shell code that points each link at its target:
  # [ { path, target, label } ], path relative to the project root.
  ensure =
    links:
    ''
      _turnkey_link() {
        local target="$1" link="$2" label="$3"
        if [ -L "$link" ]; then
          if [ "$(readlink "$link")" != "$target" ]; then
            ln -sfn "$target" "$link"
            echo "turnkey: Updated $label symlink"
          fi
        elif [ -e "$link" ]; then
          echo "turnkey: Warning: $link exists and is not a symlink"
          echo "         Remove it to let turnkey manage it"
        else
          mkdir -p "$(dirname "$link")"
          ln -s "$target" "$link"
          echo "turnkey: Created $label symlink"
        fi
      }
    ''
    + lib.concatMapStringsSep "\n" (
      link:
      "_turnkey_link ${lib.escapeShellArg "${link.target}"} ${lib.escapeShellArg link.path} ${lib.escapeShellArg link.label}"
    ) links;
}
