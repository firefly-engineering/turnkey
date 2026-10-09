---
status: accepted
---

# The build environment stays out of action keys

The build service instantiates a repo's **build environment** (a Nix output of its flake) before running any action, so buck2 never has to know Nix exists. The obvious way to keep keys honest would be to put the environment's id in every action key, as a platform property. We don't: the id travels as a request header, and an action's key covers its tools only through the store paths it names (in argv, a declared `PATH`, or a store link among its inputs). buck2 hashes only declared inputs and the `Command`, never a tool reached outside them, and a store path stands for its binary and its whole closure. Keying by environment would re-key every action whenever anything in the environment changes (a cargo dependency bump would invalidate every Go action), so nothing would be reused across a change to `flake.lock` or a deps file.

Decided in [What does the executor need from an action to run it?](https://github.com/firefly-engineering/turnkey/issues/70); the full contract is the "Executor contract" section of [`docs/specs/build-service.md`](../specs/build-service.md).

## Considered options

- **Environment id as a platform property.** Rejected for the re-keying above, even though it makes keys honest by construction.
- **Hashing tool contents as declared inputs.** Rejected: every action's input tree would carry its toolchain closure (hundreds of MB) through the content store, and libraries loaded through RPATH would still escape unless declared too.

## Consequences

- **Rules must reach tools by store path.** An undeclared `PATH` lookup gives an action a key that two environments share. This is the same discipline local execution already needs (G1, G2, and the daemon-environment rules), and [Verify action keys match across machines (G10)](https://github.com/firefly-engineering/turnkey/issues/55) is where a violation shows.
- **A shared result is only as honest as the rule that keyed it.** A rule that escapes the discipline can serve one environment's output to another.
