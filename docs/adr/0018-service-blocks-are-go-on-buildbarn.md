---
status: accepted
---

# The build service's blocks are Go, built on Buildbarn's libraries

No REAPI server fits a request-driven, scale-to-zero executor as shipped, but Buildbarn's Go libraries already give us hash-checked uploads, refusal of client `UpdateActionResult`, refusal of cache entries with missing blobs, and a runner without a scheduler ([#62](https://github.com/firefly-engineering/turnkey/issues/62)). So the service blocks (cache front, executor, realiser, gatekeeper, App, collector) are written in Go in the repo's root module, on those libraries, with our own `Execute` handler and GCS storage layer, even though turnkey's own tools are Rust. The client kit stays in Rust inside `tk`; the two meet only at wire protocols (REAPI, the Nix binary-cache protocol, the realise API, token exchange).

Decided in [What are the building blocks, and where are their seams?](https://github.com/firefly-engineering/turnkey/issues/72); the blocks are in the "Building blocks" section of [`docs/specs/build-service.md`](../specs/build-service.md).

## Considered options

- **Rust throughout, with our own REAPI implementation.** Rejected: it rewrites what Buildbarn already gets right (digest verification, action-cache completeness checks), for one language.
- **Run Buildbarn's binaries as they are.** Rejected: its scheduler and workers assume long-lived processes, and it dropped its GCS/S3 backend ([#62](https://github.com/firefly-engineering/turnkey/issues/62)).

## Consequences

- **Buildbarn is pinned like any Go dependency** in the root `go.mod`, and its REAPI proto versions travel with it.
- **Two languages in the product**, joined only by wire protocols, so neither side links the other.
