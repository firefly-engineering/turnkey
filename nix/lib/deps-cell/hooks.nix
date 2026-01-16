# Hook registration and execution for dependency cells
#
# Hooks are functions that take a context and return shell commands (strings).
# They can be combined from multiple sources:
#   1. Built-in adapter hooks (language-specific defaults)
#   2. User-provided hooks (overrides/extensions)

{ lib }:

{
  # All available hook points
  hookPoints = [
    "preFetch"
    "postFetch"
    "prePatch"
    "postPatch"
    "preProcess"
    "postProcess"
    "preBuildInfra"
    "postBuildInfra"
    "preMerge"
    "postMerge"
  ];

  # Merge multiple hook sets
  # Later hooks in the list override earlier ones for the same hook point
  # If both hooks exist, commands are concatenated
  mergeHooks = hookSets:
    lib.foldl' (acc: hooks:
      lib.mapAttrs (name: hook:
        if acc ? ${name}
        then ctx: lib.concatStringsSep "\n" [
          (acc.${name} ctx)
          (hook ctx)
        ]
        else hook
      ) (acc // hooks)
    ) {} hookSets;

  # Create a hook that generates shell commands
  mkShellHook = cmdFn: cmdFn;

  # Create a hook that conditionally runs based on context
  mkConditionalHook = {
    condition,  # Function: context -> bool
    hook,       # Function: context -> string
  }: ctx:
    if condition ctx then hook ctx else "";

  # Create a hook from a static string
  mkStaticHook = cmds: _: cmds;

  # No-op hook
  noopHook = _: "";
}
