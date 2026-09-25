# Prelude patches

Patches turnkey applies to the upstream Buck2 prelude of the pinned buck2
release (`nix/buck2/buck2-source.nix`). There is one set, written against
that release's prelude, applied in name order; use numbered prefixes
(`0001-...`). Older sets are in the history.

turnkey's test result caching depends on these patches. `patch` fails the
prelude build if one doesn't apply, so the prelude never ships unpatched.

Each patch starts with a plain-text header saying what it changes and why;
`patch` ignores it.

## Porting to a new prelude version

1. Fetch the new upstream prelude, and dry-run each patch against it
   (`patch -p1 --dry-run`).
2. For each patch that doesn't apply, redo the same change on the new files,
   keep the header, and regenerate the diff in place.
3. Build turnkey's prelude on top of the new version (`nix build
   .#turnkey-prelude --no-link`), and follow the rest of the bump checklist
   in `docs/developer-manual/src/contributing/bumping-buck2.md`.
