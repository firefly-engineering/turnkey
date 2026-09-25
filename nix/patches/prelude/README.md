# Prelude patches

Patches turnkey applies to the upstream Buck2 prelude, one set per upstream
prelude version: `<version>/*.patch`, where `<version>` is the toolbox
`buck2-prelude` version (a release date, e.g. `2026-09-15`). The set is
applied in name order; use numbered prefixes (`0001-...`).

`nix/buck2/prelude.nix` picks the set matching the upstream prelude it
builds. A prelude version without a set gets no patches, so a buck2 release
whose prelude has no set must not be listed in `nix/buck2/buck2-source.nix`
(turnkey's test result caching depends on these patches).

Each patch starts with a plain-text header saying what it changes and why;
`patch` ignores it.

## Porting to a new prelude version

1. Fetch the new upstream prelude, and dry-run each patch of the newest set
   against it (`patch -p1 --dry-run`).
2. Copy the sets that apply. For those that don't, redo the same change on
   the new files, keep the header, and regenerate the diff against the new
   version's own files.
3. Build turnkey's prelude on top of the new version, and follow the upgrade
   checklist in `docs/developer-manual`.
