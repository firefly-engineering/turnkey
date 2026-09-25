---
status: accepted
---

# Turnkey owns the buck2 version

Consumers used to choose buck2 themselves, by declaring `buck2-toolchain` (or `buck2`) in their `toolchain.toml`. Turnkey had to support whichever release they picked. But turnkey mirrors buck2's wire formats: its test runner speaks buck2's test-runner protocol, generated from buck2's source at the revision the binary was built from ([ADR 0001](0001-runner-recorded-native-test-caching.md)). Its patched prelude is also written against one prelude revision. Each of these is correct only against one exact release. So each turnkey revision now ships exactly one **pinned buck2 release**: the binary, the prelude built with it, and the buck2 source revision they come from. They are named together in one record and bumped together. buck2 is no longer declarable in `toolchain.toml`, and declaring it is an evaluation error. A consumer moves to another buck2 by moving to another turnkey revision. Decided in the wayfinder map `turnkey-luk` ("Bundle buck2 inside turnkey").

## Considered options

- **Consumer declares buck2, turnkey supports a table of releases** (the previous model). Rejected. Every release a consumer could pick needed its own source revision, protocol code, prelude pairing and patch set. A release missing from the table turned test result caching off. The protocol mirror and the binary that speaks it were also pinned by two different parties.
- **Turnkey supports a set of releases, and the consumer picks one through a turnkey option.** Rejected for the same reasons, with a nicer interface. It keeps turnkey paying for several releases at once, and keeps the unsupported-release path alive.
- **An explicit override option for the buck2 package.** Rejected. It would let a consumer run a binary whose protocol no longer matches the test runner, which is the failure this decision exists to remove. Anyone who really needs a different buck2 can override turnkey's `toolbox` input with `follows`, knowingly off the supported path.

## Consequences

- **buck2 is resolved from turnkey's own toolbox**, never from the consumer's registry. A consumer's `registry` or `registryExtensions` cannot move it.
- **The prelude is pinned by revision as well as by date.** The record carries the prelude commit the buck2 release was built with, so a toolbox update that points the date at another commit fails loudly. A wrong or missing prelude entry is fixed in toolbox, not overridden in turnkey.
- **Every bump is gated by the test-runner parity suite.** The gate is run by hand, and the commit that bumps the pin carries the suite's summary line. It isn't a CI job, because CI can't build the dev shell the suite needs.
- **Only buck2 is bundled.** reindeer, which toolbox's `buck2-toolchain` meta-package also carries, is not used by turnkey. Consumers who want it declare it like any other tool.
- **Which shells get buck2 is turnkey configuration**, not a `toolchain.toml` entry. `buck2` stays on the shell's PATH.
- **The prelude isn't a consumer choice either.** It used to be one of four strategies (`bundled`, `git`, `nix`, `path`), and the default template chose `bundled`. That gave new projects buck2's upstream prelude, whose test rules never opt into test result caching, so caching was silently off. There is now only turnkey's prelude. Setting `prelude.strategy` is an evaluation error. The one escape hatch is `prelude.path`, which is off the supported path and turns test result caching off, since turnkey can't know which of that prelude's test rules are cache-safe. Decided in `turnkey-efx.5`.
