# Prelude patches

Patches turnkey applies to the upstream Buck2 prelude of the pinned buck2
release (`nix/buck2/buck2-source.nix`): `<version>/*.patch`, where
`<version>` is the toolbox `buck2-prelude` version (a release date, e.g.
`2026-09-15`). The set is applied in name order; use numbered prefixes
(`0001-...`).

`nix/buck2/prelude.nix` picks the set matching the upstream prelude it
builds, and fails if there is none: turnkey's test result caching depends on
these patches. Only the pinned release's set is kept; older ones are in the
history.

Each patch starts with a plain-text header saying what it changes and why;
`patch` ignores it.

## Porting to a new prelude version

1. Fetch the new upstream prelude, and dry-run each patch of the current set
   against it (`patch -p1 --dry-run`).
2. Create the new version's directory. Copy the patches that apply. For those
   that don't, redo the same change on the new files, keep the header, and
   regenerate the diff against the new version's own files.
3. Remove the old version's directory in the same change that bumps the pin.
4. Build turnkey's prelude on top of the new version, and follow the rest of
   the bump checklist in
   `docs/developer-manual/src/contributing/bumping-buck2.md`.
