//! src/go/pkg/starlark's tests, ported

use super::*;

fn strings(list: &[&str]) -> Vec<String> {
    list.iter().map(|s| s.to_string()).collect()
}

fn parse_str(src: &str) -> File {
    parse("rules.star", src.to_string()).unwrap()
}

const SAMPLE_RULES_STAR: &str = r#"# Sample rules.star file
load("@prelude//:rules.bzl", "go_library", "go_test")

# Main library
go_library(
    name = "mylib",
    srcs = ["foo.go", "bar.go"],
    deps = [
        "//pkg/foo:foo",
        "godeps//vendor/github.com/bar:bar",
    ],
    visibility = ["PUBLIC"],
)

go_test(
    name = "mylib_test",
    srcs = ["foo_test.go"],
    target_under_test = ":mylib",
    visibility = ["PUBLIC"],
)
"#;

#[test]
fn parses() {
    let f = parse_str(SAMPLE_RULES_STAR);

    assert_eq!(f.loads.len(), 1);
    assert_eq!(f.loads[0].module, "@prelude//:rules.bzl");
    assert_eq!(f.loads[0].symbols.len(), 2);

    assert_eq!(f.targets.len(), 2);
    let lib = f.get_target("mylib").expect("mylib");
    assert_eq!(lib.rule, "go_library");
    assert_eq!(
        lib.get_attribute("srcs").unwrap().value,
        Value::StringList(strings(&["foo.go", "bar.go"]))
    );
    assert_eq!(
        lib.get_deps(),
        ["//pkg/foo:foo", "godeps//vendor/github.com/bar:bar"]
    );

    let test = f.get_target("mylib_test").expect("mylib_test");
    assert_eq!(test.rule, "go_test");
    assert_eq!(test.get_string_attr("target_under_test"), ":mylib");
}

#[test]
fn parses_glob_as_an_expression() {
    let f = parse_str(
        r#"load("@prelude//:rules.bzl", "go_library")

go_library(
    name = "mylib",
    srcs = glob(["*.go"]),
    visibility = ["PUBLIC"],
)
"#,
    );
    let srcs = &f
        .get_target("mylib")
        .unwrap()
        .get_attribute("srcs")
        .unwrap()
        .value;
    assert_eq!(srcs, &Value::Expr(r#"glob(["*.go"])"#.into()));
}

#[test]
fn parses_booleans() {
    let f = parse_str(
        r#"load("@prelude//:rules.bzl", "go_library")

go_library(
    name = "mylib",
    srcs = ["main.go"],
    optimizer = True,
    debug = False,
)
"#,
    );
    let lib = f.get_target("mylib").unwrap();
    assert_eq!(
        lib.get_attribute("optimizer").unwrap().value,
        Value::Bool(true)
    );
    assert_eq!(
        lib.get_attribute("debug").unwrap().value,
        Value::Bool(false)
    );
}

/// Integers read as Go's strconv.ParseInt(raw, 0, 64) reads them
#[test]
fn parses_integers() {
    let f = parse_str(
        "r(name = \"x\", a = 42, b = 0x1F, c = 0o17, d = 0b101, e = 99999999999999999999)\n",
    );
    let t = &f.targets[0];
    for (attr, want) in [("a", 42), ("b", 31), ("c", 15), ("d", 5), ("e", i64::MAX)] {
        assert_eq!(
            t.get_attribute(attr).unwrap().value,
            Value::Int(want),
            "{attr}"
        );
    }
}

#[test]
fn round_trip() {
    let f = parse_str(SAMPLE_RULES_STAR);
    assert_eq!(f.write(), SAMPLE_RULES_STAR);
}

#[test]
fn mutation_tracking() {
    let mut f = parse_str(SAMPLE_RULES_STAR);
    assert!(!f.is_modified(), "file modified after parse");
    let lib = f.get_target_mut("mylib").unwrap();
    assert!(!lib.is_modified(), "target modified after parse");

    lib.add_dep("//new:dep");
    assert!(lib.is_modified(), "target not modified after add_dep");
    assert!(
        f.is_modified(),
        "file not modified after a target's modification"
    );
}

#[test]
fn modify_deps() {
    let mut f = parse_str(SAMPLE_RULES_STAR);
    let lib = f.get_target_mut("mylib").unwrap();

    lib.add_dep("//new:dep");
    assert_eq!(lib.get_deps().len(), 3);

    lib.remove_dep("//pkg/foo:foo");
    assert_eq!(lib.get_deps().len(), 2);

    // Adding an existing dep doesn't duplicate it
    lib.add_dep("//new:dep");
    assert_eq!(lib.get_deps().len(), 2);

    lib.set_deps(strings(&["//only:one"]));
    assert_eq!(lib.get_deps(), ["//only:one"]);
}

#[test]
fn add_target() {
    let mut f = parse_str(SAMPLE_RULES_STAR);
    let t = f.add_target("go_binary", "mybin");
    t.set_string("main", "main.go");
    t.set_deps(strings(&["//pkg:lib"]));

    assert_eq!(f.targets.len(), 3);
    assert_eq!(f.get_target("mybin").unwrap().rule, "go_binary");
}

/// A new target is written first, then the file as it was
#[test]
fn new_target_is_written_first() {
    let mut f = parse_str("x = 1\n\ngo_library(name = \"a\")\n");
    f.add_target("go_binary", "b").set_deps(strings(&["//a:a"]));
    assert_eq!(
        f.write(),
        "go_binary(\n    name = \"b\",\n    deps = [\"//a:a\"],\n)\nx = 1\n\ngo_library(name = \"a\")\n"
    );
}

#[test]
fn remove_target() {
    let mut f = parse_str(SAMPLE_RULES_STAR);
    assert!(f.remove_target("mylib_test"));
    assert_eq!(f.targets.len(), 1);
    assert!(f.get_target("mylib_test").is_none());
    assert!(!f.remove_target("nonexistent"));
}

#[test]
fn write_modified() {
    let mut f = parse_str(SAMPLE_RULES_STAR);
    f.get_target_mut("mylib")
        .unwrap()
        .set_deps(strings(&["//new:dep"]));
    let out = f.write();
    assert!(out.contains(r#""//new:dep""#), "{out}");
    assert!(!out.contains(r#""//pkg/foo:foo""#), "{out}");
    assert!(out.contains("go_library"), "{out}");
    assert!(out.contains("mylib_test"), "{out}");
}

const SAMPLE_WITH_MARKERS: &str = r#"load("@prelude//:rules.bzl", "go_library")

go_library(
    name = "mylib",
    srcs = ["foo.go"],
    deps = [
        # turnkey:auto-start
        "//auto:dep1",
        "//auto:dep2",
        # turnkey:auto-end
        # turnkey:preserve-start
        "//manual:special",
        # turnkey:preserve-end
    ],
    visibility = ["PUBLIC"],
)
"#;

#[test]
fn parses_markers() {
    let f = parse_str(SAMPLE_WITH_MARKERS);
    let lib = f.get_target("mylib").unwrap();
    let Value::Deps(deps) = &lib.get_attribute("deps").unwrap().value else {
        panic!("deps is not a list with markers");
    };
    assert!(deps.has_markers);
    assert_eq!(deps.auto_deps, ["//auto:dep1", "//auto:dep2"]);
    assert_eq!(deps.preserved_deps, ["//manual:special"]);
    assert_eq!(lib.get_deps().len(), 3);
    assert_eq!(lib.get_auto_deps().len(), 2);
    assert_eq!(lib.get_preserved_deps().len(), 1);
}

#[test]
fn set_deps_preserves_markers() {
    let mut f = parse_str(SAMPLE_WITH_MARKERS);
    let lib = f.get_target_mut("mylib").unwrap();
    lib.set_deps(strings(&["//new:dep1", "//new:dep2", "//new:dep3"]));
    assert_eq!(lib.get_preserved_deps(), ["//manual:special"]);
    assert_eq!(lib.get_auto_deps().len(), 3);

    let out = f.write();
    for want in [
        r#""//new:dep1""#,
        r#""//manual:special""#,
        "# turnkey:auto-start",
        "# turnkey:preserve-start",
    ] {
        assert!(out.contains(want), "output lacks {want}:\n{out}");
    }
}

#[test]
fn round_trip_with_markers() {
    let f = parse_str(SAMPLE_WITH_MARKERS);
    let f2 = parse("rules.star", f.write()).unwrap();
    let lib = f2.get_target("mylib").unwrap();
    assert_eq!(lib.get_auto_deps().len(), 2);
    assert_eq!(lib.get_preserved_deps().len(), 1);
}

#[test]
fn set_deps_keeps_deps_outside_the_markers() {
    let mut f = parse_str(
        r#"go_library(
    name = "lib",
    deps = [
        "//hand/written:dep",
        # turnkey:auto-start
        "//old:auto",
        # turnkey:auto-end
    ],
)
"#,
    );
    f.get_target_mut("lib")
        .unwrap()
        .set_deps(strings(&["//new:auto"]));
    let out = f.write();
    assert!(
        out.contains("//hand/written:dep"),
        "a dep outside the markers was dropped:\n{out}"
    );
}

#[test]
fn markers_from_the_syntax_tree() {
    let f = parse_str(
        r#"go_library(
    name = "lib",
    deps = [
        # turnkey:preserve-start
        "//kept:one", "//kept:two",
        # turnkey:preserve-end
        # turnkey:auto-start
        "//auto:one", "//auto:two",  # two on a line
        # turnkey:auto-end
    ],
)
"#,
    );
    let lib = f.get_target("lib").unwrap();
    assert_eq!(lib.get_auto_deps(), ["//auto:one", "//auto:two"]);
    assert_eq!(lib.get_preserved_deps(), ["//kept:one", "//kept:two"]);
}

#[test]
fn no_sync_marker() {
    let f = parse_str(
        r#"rust_library(
    name = "synced",
)

# Built with platform-specific features
# turnkey:no-sync
rust_library(
    name = "opted-out",
)

# turnkey:no-sync is only a marker on its own line
rust_library(
    name = "also-synced",
)
"#,
    );
    let got: Vec<(&str, bool)> = f
        .targets
        .iter()
        .map(|t| (t.name.as_str(), t.no_sync))
        .collect();
    assert_eq!(
        got,
        [
            ("synced", false),
            ("opted-out", true),
            ("also-synced", false)
        ]
    );
}

#[test]
fn parses_single_quoted_strings() {
    let f = parse_str(
        r#"go_library(
    name = 'single',
    srcs = ['a.go', "b.go"],
    importpath = 'example.com/x',
)
"#,
    );
    let lib = f.get_target("single").expect("target single");
    assert_eq!(
        lib.get_attribute("srcs").unwrap().value,
        Value::StringList(strings(&["a.go", "b.go"]))
    );
    assert_eq!(
        lib.get_attribute("importpath").unwrap().value,
        Value::String("example.com/x".into())
    );
}

#[test]
fn set_deps_after_non_ascii_on_the_same_line() {
    let mut f = parse_str("go_library(name = \"x\", srcs = [\"café\"], deps = [])\n");
    f.get_target_mut("x").unwrap().set_deps(strings(&["//a:b"]));
    let out = f.write();
    parse("out.star", out.clone()).expect("the rewritten file parses");
    assert!(
        out.contains(r#"srcs = ["café"]"#) && out.contains(r#""//a:b""#),
        "{out}"
    );
}

/// go.starlark.net ends an index's, a slice's and an empty tuple's span at
/// their closing bracket: a value that is or ends with one is still kept
/// whole when its target is written again.
#[test]
fn values_ending_with_a_bracket_are_kept_whole() {
    // One line: the new attribute can't be inserted, so the whole target
    // is written again from its values
    let mut f = parse_str(
        "rust_library(name = \"lib\", srcs = SRCS[1:], env = ENV[\"x\"], t = (), n = -X[0], deps = [])\n",
    );
    f.targets[0].set_select("features", strings(&["default"]), vec![]);
    assert_eq!(
        f.write(),
        r#"rust_library(
    name = "lib",
    srcs = SRCS[1:],
    env = ENV["x"],
    t = (),
    n = -X[0],
    deps = [],
    features = ["default"],
)
"#
    );

    // A last argument ending with a bracket still ends before its comma:
    // the new attribute is inserted after it
    let mut f = parse_str("rust_library(\n    name = \"lib\",\n    srcs = SRCS[1:],\n)\n");
    f.targets[0].set_select("features", strings(&["default"]), vec![]);
    assert_eq!(
        f.write(),
        "rust_library(\n    name = \"lib\",\n    srcs = SRCS[1:],\n    features = [\"default\"],\n)\n"
    );
}

const SELECT_RULES_STAR: &str = r#"load("@prelude//:rules.bzl", "rust_library")

rust_library(
    name = "lib",
    srcs = glob(["src/**/*.rs"]),
    deps = [
        # turnkey:auto-start
        "rustdeps//vendor/libc:libc",
        # turnkey:auto-end
        # turnkey:preserve-start
        "//native:lib",  # hand-written
        # turnkey:preserve-end
    ] + select({
        "config//os:linux": ["rustdeps//vendor/fuser:fuser"],
        # macOS mounts through FUSE-T
        "config//os:macos": [
            "rustdeps//vendor/fuse-t:fuse-t",
            "rustdeps//vendor/objc:objc",
        ],
    }),
    visibility = ["PUBLIC"],
)
"#;

fn branch(key: &str, labels: &[&str]) -> SelectBranch {
    SelectBranch {
        key: key.into(),
        value: Value::StringList(strings(labels)),
    }
}

#[test]
fn parses_select() {
    let f = parse_str(SELECT_RULES_STAR);
    let lib = f.get_target("lib").unwrap();
    let sel = lib.get_select("deps").expect("deps is a select()");
    let Some(Value::Deps(common)) = sel.common.as_deref() else {
        panic!("common = {:?}, want the marked list", sel.common);
    };
    assert_eq!(common.auto_deps, ["rustdeps//vendor/libc:libc"]);
    assert_eq!(common.preserved_deps, ["//native:lib"]);
    assert_eq!(
        sel.branches,
        [
            branch("config//os:linux", &["rustdeps//vendor/fuser:fuser"]),
            branch(
                "config//os:macos",
                &[
                    "rustdeps//vendor/fuse-t:fuse-t",
                    "rustdeps//vendor/objc:objc"
                ]
            ),
        ]
    );
    assert_eq!(lib.get_preserved_labels("deps"), ["//native:lib"]);
}

#[test]
fn select_round_trip_is_byte_identical() {
    let mut f = parse_str(SELECT_RULES_STAR);
    assert_eq!(f.write(), SELECT_RULES_STAR);

    // Setting the same value is no change either
    let lib = f.get_target_mut("lib").unwrap();
    let branches = lib.get_select("deps").unwrap().branches.clone();
    lib.set_select("deps", strings(&["rustdeps//vendor/libc:libc"]), branches);
    assert!(
        !f.is_modified(),
        "setting the same select() modified the target"
    );
    assert_eq!(f.write(), SELECT_RULES_STAR);
}

/// Changing one branch rewrites only that branch: the other branch, its
/// comment and the common part are untouched.
#[test]
fn select_change_rewrites_only_that_branch() {
    let mut f = parse_str(SELECT_RULES_STAR);
    let lib = f.get_target_mut("lib").unwrap();
    let mut branches = lib.get_select("deps").unwrap().branches.clone();
    branches[0] = branch(
        "config//os:linux",
        &["rustdeps//vendor/fuser:fuser", "rustdeps//vendor/nix:nix"],
    );
    lib.set_select("deps", strings(&["rustdeps//vendor/libc:libc"]), branches);

    let want = SELECT_RULES_STAR.replacen(
        r#""config//os:linux": ["rustdeps//vendor/fuser:fuser"],"#,
        r#""config//os:linux": [
            "rustdeps//vendor/fuser:fuser",
            "rustdeps//vendor/nix:nix",
        ],"#,
        1,
    );
    assert_eq!(f.write(), want);
}

/// A plain list becomes the canonical [<common>] + select({...}), keeping
/// its markers on the common part.
#[test]
fn set_select_on_a_plain_list() {
    let src = r#"rust_library(
    name = "lib",
    deps = [
        # turnkey:auto-start
        "//a:a",
        # turnkey:auto-end
    ],
)
"#;
    let mut f = parse_str(src);
    f.get_target_mut("lib").unwrap().set_select(
        "deps",
        strings(&["//a:a"]),
        vec![
            branch("config//os:linux", &["//l:l"]),
            branch("config//os:macos", &[]),
        ],
    );
    let want = r#"rust_library(
    name = "lib",
    deps = [
        # turnkey:auto-start
        "//a:a",
        # turnkey:auto-end
    ] + select({
        "config//os:linux": ["//l:l"],
        "config//os:macos": [],
    }),
)
"#;
    let got = f.write();
    assert_eq!(got, want);

    // and back to a plain list
    let mut f = parse("rules.star", got).unwrap();
    f.get_target_mut("lib")
        .unwrap()
        .set_select("deps", strings(&["//a:a"]), vec![]);
    assert_eq!(f.write(), src);
}

/// Without common labels the value is the select() alone.
#[test]
fn set_select_without_common() {
    let mut f = parse_str("rust_library(\n    name = \"lib\",\n    deps = [],\n)\n");
    f.get_target_mut("lib").unwrap().set_select(
        "deps",
        vec![],
        vec![
            branch("config//os:linux", &["//l:l"]),
            branch("config//os:macos", &[]),
        ],
    );
    assert_eq!(
        f.write(),
        "rust_library(\n    name = \"lib\",\n    deps = select({\n        \"config//os:linux\": [\"//l:l\"],\n        \"config//os:macos\": [],\n    }),\n)\n"
    );
}

/// A list with markers holding something other than a label is not a
/// label list: reading it would drop the nested select() on rewrite.
#[test]
fn marked_list_with_non_label_is_unreadable() {
    let f = parse_str(
        r#"rust_library(
    name = "lib",
    deps = [
        # turnkey:auto-start
        "//a:a",
        # turnkey:auto-end
        _EXTRA,
    ],
)
"#,
    );
    let value = &f
        .get_target("lib")
        .unwrap()
        .get_attribute("deps")
        .unwrap()
        .value;
    assert!(matches!(value, Value::Expr(_)), "deps parsed as {value:?}");
}

/// Other shapes around select() stay expressions.
#[test]
fn other_select_shapes_are_expressions() {
    for value in [
        r#"select({"config//os:linux": []}) + ["//a:a"]"#,
        r#"_COMMON + select({"config//os:linux": []})"#,
        r#"select({_KEY: []})"#,
    ] {
        let f = parse_str(&format!(
            "rust_library(\n    name = \"lib\",\n    deps = {value},\n)\n"
        ));
        let got = &f.targets[0].get_attribute("deps").unwrap().value;
        assert_eq!(got, &Value::Expr(value.into()), "deps = {value}");
    }
}

/// A new attribute goes on its own line before the closing parenthesis,
/// leaving the rest of the target, comments included, as written.
#[test]
fn new_attribute_is_inserted() {
    let mut f = parse_str(
        r#"rust_library(
    name = "lib",
    # the crate's sources
    srcs = glob(["src/**/*.rs"]),
)
"#,
    );
    f.get_target_mut("lib")
        .unwrap()
        .set_select("features", strings(&["default"]), vec![]);
    assert_eq!(
        f.write(),
        r#"rust_library(
    name = "lib",
    # the crate's sources
    srcs = glob(["src/**/*.rs"]),
    features = ["default"],
)
"#
    );
}

/// A target that can't be spliced (an attribute removed) is written again
/// whole, from its model
#[test]
fn removed_attribute_rewrites_the_target() {
    let mut f = parse_str(
        "go_library(\n    name = \"x\",\n    srcs = glob([\"*.go\"]),  # all\n    deps = [],\n)\n",
    );
    f.targets[0].remove_attribute("deps");
    assert_eq!(
        f.write(),
        "go_library(\n    name = \"x\",\n    srcs = glob([\"*.go\"]),\n)\n"
    );
}

#[test]
fn write_formatted() {
    let f = parse_str(SAMPLE_RULES_STAR);
    assert_eq!(
        f.write_formatted(),
        r#"load("@prelude//:rules.bzl", "go_library", "go_test")

go_library(
    name = "mylib",
    srcs = [
        "foo.go",
        "bar.go",
    ],
    deps = [
        "//pkg/foo:foo",
        "godeps//vendor/github.com/bar:bar",
    ],
    visibility = ["PUBLIC"],
)

go_test(
    name = "mylib_test",
    srcs = ["foo_test.go"],
    target_under_test = ":mylib",
    visibility = ["PUBLIC"],
)
"#
    );
}

/// A file that doesn't parse is an error
#[test]
fn syntax_error() {
    let err = parse("rules.star", "go_library(\n".into()).unwrap_err();
    assert!(
        err.to_string().starts_with("parsing starlark: rules.star:"),
        "{err}"
    );
}

/// A missing file is an error, worded as Go words it
#[test]
fn missing_file() {
    let path = Path::new("/nonexistent/rules-star/rules.star");
    let err = parse_file(path).unwrap_err();
    assert_eq!(
        err.to_string(),
        format!(
            "reading file: open {}: no such file or directory",
            path.display()
        )
    );
}
