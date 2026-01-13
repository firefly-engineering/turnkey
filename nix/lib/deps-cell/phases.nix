# Phase definitions and execution engine for dependency cells
#
# This module defines the standard phases that all dependency packages go through:
#   fetch → patch → process → buildInfra → merge
#
# Each phase has pre and post hooks that language adapters can register into.

{ lib }:

{
  # Standard phases executed in order for per-dependency packages
  depPhases = [
    "fetch"       # Fetch source from network (git, crates.io, PyPI)
    "patch"       # Apply patches to source (fixups, compatibility patches)
    "process"     # Language-specific processing (feature computation, cfg flags)
    "buildInfra"  # Generate Buck2 BUCK files for the dependency
  ];

  # Phases executed during cell merging
  cellPhases = [
    "merge"       # Combine all dependencies into a single cell
  ];

  # Phase descriptions for documentation
  phaseDescriptions = {
    fetch = "Fetch dependency source from network (git, crates.io, PyPI)";
    patch = "Apply patches to source (fixups, compatibility patches)";
    process = "Language-specific processing (feature computation, cfg flags)";
    buildInfra = "Generate Buck2 BUCK files for the dependency";
    merge = "Combine all dependencies into a single cell";
  };

  # Capitalize first letter for hook name generation
  capitalize = str:
    let
      first = lib.substring 0 1 str;
      rest = lib.substring 1 (lib.stringLength str) str;
    in
    lib.toUpper first + rest;

  # Generate hook names for a phase
  # e.g., "fetch" -> { pre = "preFetch"; post = "postFetch"; }
  hookNames = phase: {
    pre = "pre${lib.deps-cell.phases.capitalize phase}";
    post = "post${lib.deps-cell.phases.capitalize phase}";
  };

  # Run a single phase with its hooks
  # Returns updated context with accumulated shell commands
  runPhase = {
    phase,      # Phase name (e.g., "fetch", "patch")
    hooks,      # Attribute set of hooks
    phaseImpl,  # Function: context -> string (shell commands)
    context,    # Build context passed through phases
  }:
  let
    names = lib.deps-cell.phases.hookNames phase;
    preHook = hooks.${names.pre} or null;
    postHook = hooks.${names.post} or null;

    # Pre-hook shell commands
    preCmds = if preHook != null then preHook context else "";

    # Phase implementation shell commands
    phaseCmds = phaseImpl context;

    # Post-hook shell commands
    postCmds = if postHook != null then postHook context else "";

    # Accumulate commands under phase name
    phaseKey = "${phase}Cmds";
    existingCmds = context.${phaseKey} or "";
  in
  context // {
    ${phaseKey} = lib.concatStringsSep "\n" (
      lib.filter (s: s != "") [ existingCmds preCmds phaseCmds postCmds ]
    );
  };

  # Run all dependency phases in sequence
  runDepPhases = {
    hooks,        # Attribute set of hooks
    phaseImpls,   # Attribute set of phase implementations
    context,      # Initial build context
  }:
  lib.foldl' (ctx: phase:
    lib.deps-cell.phases.runPhase {
      inherit phase hooks context;
      phaseImpl = phaseImpls.${phase} or (_: "");
      context = ctx;
    }
  ) context lib.deps-cell.phases.depPhases;
}
