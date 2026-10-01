# Rust replacements for the Go tools' parsers

Research for [Which Rust crates can replace the Go tools' go.mod, Go-source,
build-constraint and Starlark parsers?](https://github.com/firefly-engineering/turnkey/issues/193),
part of map [Rewrite turnkey's Go tools in Rust](https://github.com/firefly-engineering/turnkey/issues/190).
Gathered on 2026-10-01 against turnkey `main` at `1e8ffb21`.

Sources are the crates' published sources (the `.crate` tarball, which
docs.rs shows), crates.io and GitHub metadata, the Go sources turnkey pins
(`golang.org/x/mod v0.31.0`, `go.starlark.net 3fee463870c9`, Go 1.26.2 from
`toolchain.toml`) and the [Go Modules Reference](https://go.dev/ref/mod).
Claims marked **(probe)** were checked by running code: a throwaway Rust crate
that ran the candidates against turnkey's own test vectors, and a Go program
that ran the same inputs through turnkey's packages and `x/mod`. Neither is
committed. *(inferred)* marks a conclusion drawn from the sources rather than
stated by them.

## TL;DR

| Area | Go today | Recommendation | Hand-written grammar? |
|---|---|---|---|
| `go.mod` / `go.work` | `x/mod/modfile` | No crate is faithful enough. Write a lexer and parser for the documented go.mod grammar, shared by go.mod and go.work. | **Yes**, about 300–450 lines |
| Module path escaping, semver | hand-rolled `escapeModulePath`; semver not used | Port `module.EscapePath`/`EscapeVersion` (about 20 lines). Port `x/mod/semver` only if a comparison is ever needed: the `semver` crate orders differently. | Small hand port |
| Go package clause and imports | `go/parser` (`ImportsOnly`) | **`tree-sitter-go` 0.25.0**. Check for errors only in the header, and unquote import strings yourself. | No |
| `//go:build`, `// +build` | `go/build/constraint` | No crate exists. Port the recursive-descent parser. | **Yes**, about 200 lines |
| Starlark (`rules.star`) | `go.starlark.net/syntax` | **`starlark_syntax` 0.14.2** (the parser Buck2 itself uses). `tree-sitter-starlark` is the lighter fallback. Byte-identical output depends on porting `strconv.Quote`, not on the parser. | No, but comment attachment must be rebuilt (small) |
| PEP 508 | `src/go/pkg/pep508` (hand-written) | **Reuse the Rust twin that already exists**, `src/cmd/pydeps-gen/src/pep508.rs`. `pep508_rs` gets a vector wrong, and `uv-pep508` is unstable and needs rustc 1.96. | Already written |
| Cargo `cfg()` | `src/go/pkg/cargocfg` (hand-written) | **`cfg-expr` 0.20.9**, with its built-in target table in place of `ForPlatform`. 33 of 33 vectors agree **(probe)**. | No |

The probe also found four latent bugs in the Go code, listed under
[Latent Go bugs](#latent-go-bugs-found-on-the-way). A faithful port should
**not** reproduce them, so a byte-for-byte parity harness has to allow them as
known differences.

---

## 1. `go.mod` / `go.work`

### What turnkey uses

- `modfile.Parse`: `Module.Mod.Path`, `Require[].{Mod.Path, Mod.Version, Indirect}`,
  and `Replace[].{Old.Path, New.Path}`
  ([`godeps/parser.go`](../../src/go/pkg/godeps/parser.go),
  [`godeps/workspace.go`](../../src/go/pkg/godeps/workspace.go)).
- `modfile.ParseWork`: `Use[].Path` and `Replace`.
- `modfile.IsDirectoryPath`
  ([`workspace.go:144`](../../src/go/pkg/godeps/workspace.go#L144)).
- `modfile.ModulePath`
  ([`mapper/golang.go:279`](../../src/go/pkg/mapper/golang.go#L279)).
- go.sum is split on whitespace by hand (`ParseGoSum`). That is a
  line-oriented format, not a grammar, and needs no crate.

### The format

[Lexical elements](https://go.dev/ref/mod#go-mod-file-lexical): newlines are
tokens; comments are `//` to end of line; the punctuation is `(`, `)` and `=>`;
identifiers are runs of non-whitespace; strings are interpreted (`"…"`, with
escapes) or raw (`` `…` ``); and "identifiers and strings are interchangeable".
go.work uses the same lexer with the verbs `go`, `toolchain`, `use` and
`replace` ([go.work grammar](https://go.dev/ref/mod#go-work-file)). `// indirect`
is a suffix comment: `x/mod` treats it as indirect when the comment's first
field is `indirect`, or `indirect;` followed by more
([`modfile/rule.go` `isIndirect`](https://github.com/golang/mod/blob/v0.31.0/modfile/rule.go#L198-L204)).

### Candidates

| Crate | Version / date | Licence | Maintenance | go.work | Notes |
|---|---|---|---|---|---|
| [`gomod-parser`](https://crates.io/crates/gomod-parser) | 0.5.3, 2026-09-07 | Apache-2.0 | active (repo pushed 2026-10-01, 7 stars, 1.2M downloads) | **no** | winnow combinators ([`src/parser.rs`](https://docs.rs/crate/gomod-parser/0.5.3/source/src/parser.rs)) |
| [`tree-sitter-gomod-orchard`](https://crates.io/crates/tree-sitter-gomod-orchard) | 0.5.3, 2025-11-17 | MIT | Codeberg "grammar-orchard" forks | **no** | tree-sitter grammar ([`grammar.js`](https://docs.rs/crate/tree-sitter-gomod-orchard/0.5.3/source/grammar.js)) |
| [`tree-sitter-gomod`](https://crates.io/crates/tree-sitter-gomod) | 1.0.1, 2023-12-18 | MIT | stale; the listed repo returns 404 | no | superseded by the orchard fork |
| [`gomod-rs`](https://crates.io/crates/gomod-rs) | 0.1.1, 2024-05-23 | MIT | abandoned (two releases on one day) | no | nom; keeps locations |
| [omertuc/tree-sitter-go-work](https://github.com/omertuc/tree-sitter-go-work) | not on crates.io, last push 2022-10 | MIT | stale | yes | grammar only |
| [`deps-go`](https://crates.io/crates/deps-go) | 2.0.0 | MIT | active | — | **rejected: regex-based** (`Regex::new(r"^\s*require\s+(\S+)\s+(\S+)")`, [`src/parser.rs`](https://docs.rs/crate/deps-go/2.0.0/source/src/parser.rs)) |
| [`use-go-module`](https://crates.io/crates/use-go-module) | 0.0.1 | MIT/Apache-2.0 | new | — | newtypes only, no parser; versions order as plain strings |

**Fidelity of `gomod-parser` (probe, compared with `x/mod` v0.31.0 on the same inputs):**

| Input | `x/mod/modfile` | `gomod-parser` 0.5.3 |
|---|---|---|
| `module "example.com/q"` | `example.com/q` | `"example.com/q"` (keeps the quotes) |
| `module example.com/m // comment` | `example.com/m` | `example.com/m // comment` |
| `require example.com/a v1.0.0 // indirect; keep` | `Indirect=true` | `indirect=false` |
| `require ()` | accepted | parse error |

The cause is in the source: `module` and `go` take `take_till(1.., CRLF)`,
the rest of the line, and there is no string-literal handling. `indirect` is
detected only by exact equality with `Comment("indirect")`. There is no `use`
directive at all.

`tree-sitter-gomod-orchard` does handle strings, but it keeps comments as
`extras`, so `// indirect` has to be found by position. Its blocks require
`"(" "\n"`, so `require ()` is an error node. It has no go.work grammar.

### Recommendation

Hand-write a lexer and a small recursive-descent parser for the documented
grammar, sharing one lexer between go.mod and go.work. This is a grammar from
the spec, not a regex. The fields turnkey reads are few: module, require
(with indirect), replace, and use. *(inferred)* 300–450 lines with tests.
Prove it with a differential corpus: every go.mod and go.work in the repo and
the e2e fixtures, plus edge cases (quoted and raw strings, factored `module (`,
empty blocks, `indirect;`, CRLF). Run that corpus through both `x/mod` and the
port. `IsDirectoryPath` and `ModulePath` are tiny functions to port alongside.

### Module path escaping and semver

- **Escaping.** The proxy protocol case-encodes both `$module` *and*
  `$version` ([GOPROXY protocol](https://go.dev/ref/mod#goproxy-protocol)).
  turnkey's hand-rolled
  [`escapeModulePath`](../../src/go/pkg/godeps/prefetch.go#L104) escapes only
  the path. `module.EscapeVersion("v1.0.0-RC1")` is `v1.0.0-!r!c1` **(probe)**.
  No crate provides this. Port `EscapePath` and `EscapeVersion`, about 20
  lines.
- **Semver.** Nothing calls `x/mod/semver` today. `Workspace.Requires`
  promises "the highest when several do"
  ([`workspace.go:168`](../../src/go/pkg/godeps/workspace.go#L168)) but keeps
  the first version it sees. In practice the version comes from `go list -m
  all` afterwards, so this is only a misleading comment. If a comparison is
  ever needed, the [`semver`](https://crates.io/crates/semver) crate is **not**
  Go's semver **(probe)**:
  - `1.2.3+incompatible` compares `Greater` than `1.2.3`, where Go's
    `semver.Compare` gives `0`;
  - `1.2` is rejected, where Go accepts the `v1.2` shorthand.

  Pseudo-versions do order correctly as SemVer prereleases. A faithful port of
  `x/mod/semver` (about 400 lines of Go) would be needed.

## 2. Go package clause and imports

### What turnkey uses

[`goparse/parser.go`](../../src/go/pkg/goparse/parser.go) calls
`parser.ParseFile(…, ImportsOnly|ParseComments)`. It reads:

- the package name;
- the import paths, with `"C"` meaning cgo;
- the comments before the `package` clause, for build constraints.

A file that fails to parse is skipped (`ScanDir`, `TreeConstraintTags`).
`ImportsOnly` stops after the import declarations, so **errors later in the
body do not fail the parse**.

### Candidates

| Crate | Version / date | Licence | Maintenance | Notes |
|---|---|---|---|---|
| [`tree-sitter-go`](https://crates.io/crates/tree-sitter-go) | 0.25.0, 2025-08-29 | MIT | tree-sitter org, repo pushed 2026-09-13, 13.5M downloads | Needs only `tree-sitter-language 0.1`, so it fits the workspace's `tree-sitter = "0.25"` **(probe: builds with tree-sitter 0.25.10)** |
| [`gosyn`](https://crates.io/crates/gosyn) | 0.2.15, 2026-08-29 | MIT | one maintainer, 32 stars | A full hand-written Go parser with no imports-only mode. It must accept the whole file, so it fails where `go/parser` would not, and it has to keep up with new body syntax. |
| [`go-parser`](https://crates.io/crates/go-parser) (Goscript) | 0.1.5, 2023-09-12 | BSD-3-Clause | stale | no |

### Fidelity of `tree-sitter-go` (probe)

On a file with a `//go:build` line, a doc comment, `package x`, an import
block (including `C "C"` and a raw string) and a malformed function body:

- the root reports `has_error = true`;
- `comment`, `comment`, `package_clause` and `import_declaration` each report
  `has_error = false`;
- only the `function_declaration` is in error.

So the port must check errors **only on the package clause and the import
declarations**. Checking the root would drop files that `go/parser` keeps
*(inferred)*.

Other gaps, all small and none needing a grammar:

- **Import paths** come back as `interpreted_string_literal` or
  `raw_string_literal` nodes, which have to be unquoted by Go's rules. This
  fixes a Go quirk: `strings.Trim(imp.Path.Value, "\"")`
  ([`parser.go:33`](../../src/go/pkg/goparse/parser.go#L33)) leaves a raw-string
  import with its backquotes.
- **Leading comments** are sibling `comment` nodes before `package_clause`,
  with byte ranges. Each is a single `//` or `/* */` token, as `go/ast`
  `Comment.Text` is. That is enough to feed the build-constraint parser.
- **`//go:embed`** is read today with a **regex**
  ([`parser.go:14`](../../src/go/pkg/goparse/parser.go#L14)), and its
  arguments are split with `strings.Fields`, which mishandles quoted patterns.
  With tree-sitter, the port can read `comment` nodes anywhere in the tree and
  parse the arguments with Go's quoting rules. That removes a rule violation
  the Go code has today.

## 3. `//go:build` and `// +build` constraints

### What turnkey uses

- `constraint.IsGoBuild`, `IsPlusBuild` and `Parse`;
- `Expr.Eval`;
- a walk over `TagExpr`, `NotExpr`, `AndExpr` and `OrExpr` to list tags
  ([`goparse/constraints.go`](../../src/go/pkg/goparse/constraints.go)).

`PlusBuildLines` is not used.

### Candidates

None on crates.io. Searches for "go build constraint", "go:build" and
"golang" found nothing relevant. `tree-sitter-go` treats these lines as
opaque comments.

### Recommendation

Port
[`go/build/constraint/expr.go`](https://github.com/golang/go/blob/go1.26.2/src/go/build/constraint/expr.go)
(596 lines of Go, including `PlusBuildLines`, which turnkey does not need).
The port needs:

- the `//go:build` lexer and parser: `||`, `&&`, `!`, parentheses, and tags
  made of letters, digits, `_` and `.` (`isValidTag`);
- the legacy `// +build` form: spaces mean OR, commas mean AND, `!` negates;
- the prefix rules of `splitGoBuild` and `splitPlusBuild`;
- the `maxSize = 1000` limit, which guards against stack exhaustion.

*(inferred)* About 200 lines of Rust. Test it against Go's own `expr_test.go`
cases.

## 4. Starlark (`rules.star`)

### What turnkey uses

From `go.starlark.net/syntax`
([`starlark/parser.go`](../../src/go/pkg/starlark/parser.go),
[`writer.go`](../../src/go/pkg/starlark/writer.go)):

- `syntax.Parse(…, RetainComments)`;
- the `LoadStmt`, `CallExpr`, `BinaryExpr`, `ListExpr`, `DictExpr` and
  `Literal` nodes;
- `Node.Span()`, converted to byte offsets;
- the `Before`, `Suffix` and `After` comments on nodes, which carry the
  `turnkey:no-sync`, `auto-start/end` and `preserve-start/end` markers.

The parser never produces output. turnkey's own writer does: it splices the
original source by byte span and renders new values with Go's
`strconv.Quote`. So **round-tripping exactly depends on correct spans and on
porting `strconv.Quote`, not on which parser is chosen** *(inferred from
writer.go)*.

Two details of the Go library matter:

- **Comment attachment.** Each line comment attaches to the first node, in
  pre-order, that starts after it. Suffix comments attach to the last node
  that ends before them
  ([`syntax/parse.go` `assignComments`](https://github.com/google/starlark-go/blob/3fee463870c9/syntax/parse.go#L1013-L1062)).
  A Rust port has to rebuild this from comment spans. It is a short pass over
  sorted spans.
- **Columns count runes.** `Position.Col` is "1-based column (rune) number"
  ([`syntax/scan.go`](https://github.com/google/starlark-go/blob/3fee463870c9/syntax/scan.go#L235-L238)),
  but turnkey adds it to a byte offset (see
  [Latent Go bugs](#latent-go-bugs-found-on-the-way)).

### Candidates

| Crate | Version / date | Licence | Maintenance | Spans | Comments | String values |
|---|---|---|---|---|---|---|
| [`starlark_syntax`](https://crates.io/crates/starlark_syntax) (facebook/starlark-rust) | 0.14.2, 2026-06-05 | Apache-2.0 | Meta; repo pushed 2026-10-01, 1k stars; it is the parser Buck2 itself uses | byte offsets | dropped from the AST, but `AstModule::comments()` returns every comment's byte span ([`syntax/module.rs`](https://docs.rs/crate/starlark_syntax/0.14.2/source/src/syntax/module.rs)) | decoded |
| [`tree-sitter-starlark`](https://crates.io/crates/tree-sitter-starlark) (already in the workspace) | 1.3.0, 2024-12-05 | MIT | tree-sitter-grammars org, repo pushed 2026-09-13, 24 stars | byte offsets | `comment` nodes in the tree | raw: `string_start` / `string_content` / `string_end`, and escapes must be decoded by hand |
| [`starlark-cst`](https://crates.io/crates/starlark-cst) | 0.1.2, 2026-08-26 | **GPL-3.0-only** | new, 0 stars | lossless | yes | — |

What the probe saw on a `rules.star` with markers, a suffix comment and a
non-ASCII string:

- **`starlark_syntax`** (`Dialect::Extended`) parsed it and returned all four
  comment spans, at the right byte offsets.
- **`tree-sitter-starlark`** parsed it with no error nodes, and reported
  `has_error = true` on truncated input. It recovers from errors, so the port
  must reject any tree with `has_error` to match `go.starlark.net`, which
  fails the whole parse.

### Assessment

- **`starlark_syntax` is the stronger choice for fidelity** *(inferred)*:
  - it accepts exactly the dialect Buck2 accepts, so a file parses in turnkey
    if and only if it parses in Buck2;
  - it decodes string literals, so there is no escape-decoding code to write;
  - its spans are byte offsets.

  Its costs:
  - its API is 0.x (seven releases since 2023-10);
  - it brings a heavier dependency tree (`lsp-types`, `annotate-snippets`,
    `allocative`, `logos`, …);
  - comment attachment is rebuilt from `comments()`.
- **`tree-sitter-starlark` is the lightweight fallback.** It is already
  locked, and the existing
  [`src/rust/starlark-parse`](../../src/rust/starlark-parse/src/lib.rs) uses
  it. That crate is read-only, though: it keeps no attribute spans and no
  comments, so it is not a drop-in replacement for the Go package. The
  fallback needs a hand-written decoder for Starlark string literals (quotes,
  triple quotes, `r` prefix, escapes). That is a small lexer, not a grammar.
- **`starlark-cst` is rejected** for its licence (GPL-3.0-only) and its
  immaturity.
- **Either way, write `strconv.Quote` by hand.** Rust's `{:?}` escapes
  differently, for example `\u{…}` instead of `\u…` or `\x…`. Go's rule keeps
  any rune that `strconv.IsPrint` accepts, so a port matches on non-ASCII
  labels only if its printability table matches Go's *(inferred)*.

## 5. PEP 508 markers

### What turnkey uses

[`src/go/pkg/pep508`](../../src/go/pkg/pep508/pep508.go):

- requirement name, extras and marker;
- marker evaluation, with release-only version comparison and a string
  fallback;
- `Variables()`;
- `EnvFor` and `Decides`. Together these give partial-environment semantics:
  a marker that reads a variable the configuration does not fix is **kept**
  ([`mapper/pymarkers.go:204`](../../src/go/pkg/mapper/pymarkers.go#L204)).

It is pinned by `testdata/pep508-vectors.json`. Those vectors are **already
shared with a Rust implementation**:
[`src/cmd/pydeps-gen/src/pep508.rs`](../../src/cmd/pydeps-gen/src/pep508.rs)
(355 lines, hand-written, and its tests read the same JSON).

### Candidates

| Crate | Version / date | Licence | Maintenance | Notes |
|---|---|---|---|---|
| [`pep508_rs`](https://crates.io/crates/pep508_rs) | 0.9.2, 2025-01-02 | Apache-2.0 OR BSD-2-Clause | no release since 2025-01; development moved into uv | marker parser is hand-written; `regex` is used only to expand `${VAR}` in URLs |
| [`uv-pep508`](https://crates.io/crates/uv-pep508) | 0.0.88, 2026-09-29 | Apache-2.0 OR BSD-2-Clause | Astral, but its README says "an internal component of uv… unstable and will have frequent breaking changes"; 88 releases | **MSRV 1.96**, and turnkey's toolchain is rustc 1.94, so it does not build **(probe)** |

### Fidelity (probe, `pep508_rs` 0.9.2 against turnkey's vectors)

- **55 of 57 checks agree.** All seven `invalid` requirements are rejected,
  and all six `requirements` parse with the same name.
- **The two failures are one case, `python_version ~= "3.12.0"` on
  Python 3.13.** `pep508_rs` normalizes it to `python_full_version >= '3.12'
  and python_full_version < '4'`, the same as `~= "3.12"`, so it evaluates to
  true. Python's `packaging` and turnkey's pinned uv 0.11.6 both say false
  (checked with `uv pip compile --python-version 3.13`). The bug is in
  `pep508_rs`. uv's current code has fixed it.
- **The crate rewrites marker text** (`python_version < '2.7'` becomes
  `python_full_version < '2.7'`). turnkey does not read `Marker.Text`
  downstream, so this is harmless.
- **No partial environment.** `MarkerEnvironment` needs every field, so the
  "undecided means keep" rule has to be written by walking `MarkerTree::kind()`.

### Recommendation

Do not adopt a crate. Move the existing `pydeps-gen` `pep508.rs` into a shared
workspace crate and add `EnvFor`/`Decides` to it. It is already
parity-tested, and it brings no dependencies, no MSRV bump and no unstable API.

## 6. Cargo `cfg()` expressions

### What turnkey uses

[`src/go/pkg/cargocfg`](../../src/go/pkg/cargocfg/cargocfg.go) (333 lines,
hand-written, pinned by `testdata/cfg-vectors.json`, which it shares with the
Python `turnkey.cfg`). It parses `cfg(...)` or a target triple, and evaluates
it against a hand-written `ForPlatform` table: arch, vendor, os, env, family
and pointer width for four platforms.

### Candidate

[`cfg-expr`](https://crates.io/crates/cfg-expr) 0.20.9, released 2026-08-22.

- Licence: MIT OR Apache-2.0.
- Maintenance: EmbarkStudios, 106M downloads, repo pushed 2026-08-22,
  MSRV 1.85.
- Its built-in target table matches rustc 1.98.0
  (`targets::rustc_version()`).
- Its parser is a hand-written lexer and stack-based parser
  ([`src/expr/parser.rs`](https://docs.rs/crate/cfg-expr/0.20.9/source/src/expr/parser.rs)).

### Fidelity (probe)

- **All 33 vectors agree** (26 `cfg()` specs and 7 triples). Each spec is
  evaluated as `Expression::eval`, with `Predicate::Target(tp)` mapped to
  `tp.matches(get_builtin_target_by_triple(triple))` and every other predicate
  mapped to false. Triples are compared as strings. This covers `cfg(all())`,
  `cfg(any())`, `cfg(miri)` and `cfg(getrandom_backend = "custom")`.
- **This removes `ForPlatform`'s hand-copied target facts** in favour of the
  crate's table, which follows the repo rule to import a dependency's
  constants rather than copy them.
- **Differences from Go:**
  - **Case.** Go lowercases keys and values
    ([`cargocfg.go:141-152`](../../src/go/pkg/cargocfg/cargocfg.go#L141)), so
    `cfg(Unix)` and `target_os = "Linux"` hold there. `cfg-expr` treats them
    as an unknown flag and a non-matching OS. rustc is case-sensitive, so
    `cfg-expr` is the correct one *(inferred)*.
  - **Strictness.** `cfg(feature)` and `cfg(unix = "x")` are parse errors in
    `cfg-expr`. The Go code parses them and evaluates them to false. The port
    has to decide whether such a spec becomes a hard error or a no-match.
  - `cfg(all(unix,))` (a trailing comma) is accepted by both.

---

## Latent Go bugs found on the way

Each was reproduced with a Go program that called turnkey's packages at `main`
**(probe)**. A parity harness should list them as expected differences.

1. **Single-quoted Starlark strings are dropped.** `parseValue` and
   `parseListValue` decode with Go's `strconv.Unquote(e.Raw)`
   ([`starlark/parser.go:212,258`](../../src/go/pkg/starlark/parser.go#L212)),
   which rejects `'single'`. `go_library(name = 'single')` therefore gives a
   target with no name, and the target is skipped (0 targets). `parseDepsValue`
   uses the parser's decoded `lit.Value` instead, so it is inconsistent with
   them.
2. **Non-ASCII text corrupts spliced output.** `positionToOffset` adds a rune
   column as a byte count
   ([`parser.go:392`](../../src/go/pkg/starlark/parser.go#L392)). Running
   `SetDeps` on `go_library(name = "x", srcs = ["café"], deps = [])` writes
   `… deps =["//a:b"]])`.
3. **Module versions are not case-escaped.** `escapeModulePath` escapes the
   path but not the version (see §1).
4. **`//go:embed` is read with a regex** and split with `strings.Fields` (see
   §2).
