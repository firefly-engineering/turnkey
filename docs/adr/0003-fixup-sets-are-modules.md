---
status: accepted
---

# Fixup sets are modules

A repository brings **fixup sets**, and a set is a module of class `turnkeyFixups`. Sets can be turnkey's built-ins, an organization's shared registry, a third party's, or the repository's own. A set is published as `modules.turnkeyFixups.<name>` (the flake-parts `flake.modules` convention) and brought in with `imports` under one option, `perSystem.turnkey.toolchains.buck2.fixups`. That option also takes the repository's inline fixups. All sets merge in a single module evaluation: list fields concatenate, and two sets that disagree on a scalar field fail evaluation, naming both files. Before, the only way in was two attrset options merged as `builtin // consumer`, which silently threw away turnkey's built-in entry for a crate the consumer touched. Nothing could be shared between repositories. Decided in [How is a fixup set published, imported, selected and composed?](https://github.com/firefly-engineering/turnkey/issues/124), on top of the fixup record from [What is a fixup?](https://github.com/firefly-engineering/turnkey/issues/123).

## Considered options

- **A flake-parts module that sets the consumer's turnkey options.** Rejected. It ties every publisher to flake-parts and to turnkey's option paths. A `turnkeyFixups` module only needs `pkgs` and `lib`, and its `_class` makes the module system reject it if it's imported into the wrong kind of evaluation.
- **A bare attrset or function under a custom flake output.** Rejected. There's no merging, so it brings back the `//` problem, no class check, and `nix flake check` warns about an unknown output.
- **An "authoritative" set that rejects other definitions for the crates it fixes.** Rejected. Without it, a repository still gets exactly the registry's fixup unless it *visibly* diverges, through `lib.mkForce`, `disabledModules`, `enable = false`, or extra list entries. Enforcing more than that is organization policy, better done as a lint than in turnkey's types.
- **A dedicated `sets` list next to `imports`.** Rejected in favour of Nix convention. `imports` is enough to tell sets from inline fixups (see Consequences).

## Consequences

- **Opting out.** To drop a whole set, don't import it. To drop one crate's fixup, set `<lang>.<name>.enable = false`, because the module system can't remove a definition an imported module made. A disabled crate with a build.rs then fails the cell until the repository supplies its own fixup.
- **Where a fixup came from is read from definition locations.** A fixup defined only in the module that sets `fixups` counts as inline, and warns when it matches no locked dependency. Anything reached through `imports` counts as a set and is never reported, which lets one organization registry serve many repositories that each lock only part of it. A repository that splits its *own* fixups across imported files gives up that warning for them.
- **turnkey applies no fixups a repository didn't import.** turnkey's own fixups exist because turnkey's repository locks those crates. They are turnkey's own fixup set: its `flake.nix` imports them like any consumer would, and they are published per family (`modules.turnkeyFixups.{serde,ring,…}`, plus `default` for all of them) for others to import. Before, they were merged into every managed repository's cell implicitly. When a crate with a build.rs has no fixup and turnkey publishes one for it, the error names the module to import. Decided in [Do turnkey's built-in fixups ship inside turnkey or as their own flake?](https://github.com/firefly-engineering/turnkey/issues/125).
- **The option exists only on the flake-parts side.** Cells are built per system in `perSystem`. The devenv module only receives the built cells.
