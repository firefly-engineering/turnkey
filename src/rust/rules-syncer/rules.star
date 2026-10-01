# rules-syncer - rules.star files' deps kept in step with their sources
load("@prelude//:rules.bzl", "rust_library", "rust_test")

rust_library(
    name = "rules-syncer",
    srcs = glob(["src/**/*.rs"]),
    edition = "2024",
    deps = [
        "//src/rust/conditions:conditions",
        "//src/rust/deps-extract:extraction",
        "//src/rust/gomod:gomod",
        "//src/rust/goparse:goparse",
        "//src/rust/gostd:gostd",
        "//src/rust/pep508:pep508",
        "//src/rust/project-sync:project-sync",
        "//src/rust/rules-star:rules-star",
        "rustdeps//vendor/anyhow:anyhow",
        "rustdeps//vendor/cfg-expr:cfg-expr",
        "rustdeps//vendor/serde:serde",
        "rustdeps//vendor/serde_json:serde_json",
        "rustdeps//vendor/toml:toml",
    ],
    visibility = ["PUBLIC"],
)

rust_test(
    name = "rules-syncer-test",
    srcs = glob(["src/**/*.rs"]),
    edition = "2024",
    deps = [
        "//src/rust/conditions:conditions",
        "//src/rust/deps-extract:extraction",
        "//src/rust/gomod:gomod",
        "//src/rust/goparse:goparse",
        "//src/rust/gostd:gostd",
        "//src/rust/pep508:pep508",
        "//src/rust/project-sync:project-sync",
        "//src/rust/rules-star:rules-star",
        "rustdeps//vendor/anyhow:anyhow",
        "rustdeps//vendor/cfg-expr:cfg-expr",
        "rustdeps//vendor/serde:serde",
        "rustdeps//vendor/serde_json:serde_json",
        "rustdeps//vendor/tempfile:tempfile",
        "rustdeps//vendor/toml:toml",
    ],
    # The cfg() and feature activation cases src/go/pkg/cargocfg and
    # src/go/pkg/cargofeatures run too, and the sync.toml turnkey writes,
    # embedded from where testdata/ links to them
    mapped_srcs = {
        "//src/go/pkg/cargocfg:cfg-vectors": "testdata/cfg-vectors.json",
        "//src/go/pkg/cargofeatures:activation-vectors": "testdata/activation-vectors.json",
        "//src/go/pkg/rulessync:sync-contract": "testdata/sync.toml",
    },
)
