---
status: accepted
---

# Languages are proven by fixtures, not by dogfooding

Turnkey's support for a language is proven by tests and toy examples (`src/examples/`, the e2e fixtures), never by turnkey's own code being written in that language. Turnkey will support languages that will never be a significant part of its own codebase. Blockchain languages such as Solidity are the clearest case. So the fixtures for each language have to cover enough ground on their own. A language that turnkey also happens to use gets no exemption: its fixtures must still stand without turnkey's code.

Decided in [Should turnkey rewrite its Go tools in Rust, and what does it buy?](https://github.com/firefly-engineering/turnkey/issues/191), part of [Rewrite turnkey's Go tools in Rust](https://github.com/firefly-engineering/turnkey/issues/190).

## Consequences

- **Moving turnkey's own code out of a language opens a coverage gap first.** Whatever turnkey's code exercised in that language's dependency path has to be covered by fixtures before that code goes away. For Go, that means `go.work`, rules sync and `//go:build`.
- **The language turnkey is written in is an implementation choice**, decided on what it costs to maintain. It is not a test-coverage decision.
