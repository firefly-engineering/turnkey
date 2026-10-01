//! Starlark attribute values, written as turnkey writes them in `rules.star`
//!
//! The value model and writer of src/go/pkg/starlark, whose output the
//! cell generators (pydeps-cell, buckgen) and rules sync write: strings
//! quoted by Go's `strconv.Quote` ([`gostd::strconv::quote`]), a list of
//! one label on one line, longer lists one label per line, and
//! `[<common>] + select({<key>: <value>, ...})` for a value that depends on
//! the configuration.
//!
//! Only the values are here; rules sync's parser and span-preserving file
//! writer join them when rules sync is ported (#215).

use gostd::strconv::quote;

/// An attribute's value
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Value {
    /// A string
    String(String),
    /// A list of strings
    StringList(Vec<String>),
    /// A label list with turnkey's `# turnkey:auto-start` / `preserve-start`
    /// sections
    Deps(DepsValue),
    /// `True` or `False`
    Bool(bool),
    /// An integer
    Int(i64),
    /// An identifier, e.g. a variable
    Ident(String),
    /// `[<common>] + select({...})`, or `select({...})` alone
    Select(SelectValue),
    /// Any other expression, kept as written
    Expr(String),
}

/// A label list with turnkey's markers: the auto-managed labels (between
/// `# turnkey:auto-start` and `# turnkey:auto-end`), regenerated, and the
/// preserved ones (between `# turnkey:preserve-start` and
/// `# turnkey:preserve-end`), left alone.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DepsValue {
    /// The auto-managed labels
    pub auto_deps: Vec<String>,
    /// The preserved labels
    pub preserved_deps: Vec<String>,
    /// Whether the list has markers; without them, it is `raw_deps`
    pub has_markers: bool,
    /// The whole list, when it has no markers
    pub raw_deps: Vec<String>,
}

impl DepsValue {
    /// All the labels, auto-managed then preserved
    pub fn all_deps(&self) -> Vec<String> {
        if !self.has_markers {
            return self.raw_deps.clone();
        }
        let mut all = self.auto_deps.clone();
        all.extend(self.preserved_deps.iter().cloned());
        all
    }
}

/// A value that depends on the build configuration, written
/// `[<common>] + select({<key>: <value>, ...})`, or `select({...})` alone
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SelectValue {
    /// The part every configuration has: a [`Value::StringList`], or a
    /// [`Value::Deps`] when it carries turnkey's markers. `None` for a
    /// `select()` alone.
    pub common: Option<Box<Value>>,
    /// The `select()` entries, in order
    pub branches: Vec<SelectBranch>,
}

/// One `select()` entry
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SelectBranch {
    /// The entry's key, e.g. `config//os:linux` or `DEFAULT`
    pub key: String,
    /// The entry's value
    pub value: Value,
}

/// A value as written, on one line where it fits. Two values that render
/// the same are the same.
pub fn render(value: &Value) -> String {
    render_indented(value, "")
}

/// A value as written on a line indented by `indent`, e.g. an attribute of
/// a rule call (`"    "`)
pub fn render_indented(value: &Value, indent: &str) -> String {
    let mut out = String::new();
    write_value(&mut out, value, indent);
    out
}

fn write_value(out: &mut String, value: &Value, indent: &str) {
    match value {
        Value::String(s) => out.push_str(&quote(s)),
        Value::StringList(values) => write_string_list(out, values, indent),
        Value::Deps(deps) => write_deps(out, deps, indent),
        Value::Bool(true) => out.push_str("True"),
        Value::Bool(false) => out.push_str("False"),
        Value::Int(n) => out.push_str(&n.to_string()),
        Value::Ident(name) => out.push_str(name),
        Value::Select(sel) => write_select(out, sel, indent),
        Value::Expr(text) => out.push_str(text),
    }
}

/// `[<common>] + select({...})`, or `select({...})` alone
fn write_select(out: &mut String, sel: &SelectValue, indent: &str) {
    if let Some(common) = &sel.common {
        write_value(out, common, indent);
        out.push_str(" + ");
    }
    out.push_str("select({\n");
    let inner = format!("{indent}    ");
    for branch in &sel.branches {
        out.push_str(&inner);
        out.push_str(&quote(&branch.key));
        out.push_str(": ");
        write_value(out, &branch.value, &inner);
        out.push_str(",\n");
    }
    out.push_str(indent);
    out.push_str("})");
}

/// A label list with markers; without them, a plain list
fn write_deps(out: &mut String, deps: &DepsValue, indent: &str) {
    if !deps.has_markers {
        write_string_list(out, &deps.raw_deps, indent);
        return;
    }
    if deps.auto_deps.is_empty() && deps.preserved_deps.is_empty() {
        out.push_str("[]");
        return;
    }
    out.push_str("[\n");
    // An empty auto-managed section is still written, for the labels sync
    // adds later
    write_section(out, "auto", &deps.auto_deps, indent);
    if !deps.preserved_deps.is_empty() {
        write_section(out, "preserve", &deps.preserved_deps, indent);
    }
    out.push_str(indent);
    out.push(']');
}

fn write_section(out: &mut String, marker: &str, labels: &[String], indent: &str) {
    out.push_str(&format!("{indent}    # turnkey:{marker}-start\n"));
    for label in labels {
        out.push_str(&format!("{indent}    {},\n", quote(label)));
    }
    out.push_str(&format!("{indent}    # turnkey:{marker}-end\n"));
}

/// `[]`, `["<one>"]`, or one string per line
fn write_string_list(out: &mut String, values: &[String], indent: &str) {
    match values {
        [] => out.push_str("[]"),
        [one] => {
            out.push('[');
            out.push_str(&quote(one));
            out.push(']');
        }
        _ => {
            out.push_str("[\n");
            for v in values {
                out.push_str(&format!("{indent}    {},\n", quote(v)));
            }
            out.push_str(indent);
            out.push(']');
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn list(labels: &[&str]) -> Value {
        Value::StringList(labels.iter().map(|s| s.to_string()).collect())
    }

    #[test]
    fn lists() {
        assert_eq!(render(&list(&[])), "[]");
        assert_eq!(render(&list(&["//a:a"])), r#"["//a:a"]"#);
        assert_eq!(
            render_indented(&list(&["//a:a", "//b:b"]), "    "),
            "[\n        \"//a:a\",\n        \"//b:b\",\n    ]"
        );
    }

    /// Strings are quoted as Go's strconv.Quote quotes them
    #[test]
    fn strings_are_quoted_as_go_does() {
        assert_eq!(
            render(&Value::String("a\"b\\c\u{1}é\u{200b}".into())),
            "\"a\\\"b\\\\c\\x01é\\u200b\""
        );
    }

    #[test]
    fn scalars() {
        assert_eq!(render(&Value::Bool(true)), "True");
        assert_eq!(render(&Value::Bool(false)), "False");
        assert_eq!(render(&Value::Int(-42)), "-42");
        assert_eq!(render(&Value::Ident("_DEPS".into())), "_DEPS");
        assert_eq!(
            render(&Value::Expr("glob([\"*.go\"])".into())),
            "glob([\"*.go\"])"
        );
    }

    /// The pydeps cell's deps: common labels, then a select() whose
    /// branches are indented one level further
    #[test]
    fn select_with_common() {
        let sel = Value::Select(SelectValue {
            common: Some(Box::new(list(&["//vendor/a:a", "//vendor/b:b"]))),
            branches: vec![
                SelectBranch {
                    key: "config//os:linux".into(),
                    value: list(&["//vendor/c:c"]),
                },
                SelectBranch {
                    key: "config//os:macos".into(),
                    value: list(&[]),
                },
            ],
        });
        assert_eq!(
            render_indented(&sel, "    "),
            r#"[
        "//vendor/a:a",
        "//vendor/b:b",
    ] + select({
        "config//os:linux": ["//vendor/c:c"],
        "config//os:macos": [],
    })"#
        );
    }

    #[test]
    fn select_alone_with_lists() {
        let sel = Value::Select(SelectValue {
            common: None,
            branches: vec![SelectBranch {
                key: "toolchains//conditions:linux-arm64".into(),
                value: list(&["//x:x", "//y:y"]),
            }],
        });
        assert_eq!(
            render(&sel),
            "select({\n    \"toolchains//conditions:linux-arm64\": [\n        \"//x:x\",\n        \"//y:y\",\n    ],\n})"
        );
    }

    #[test]
    fn deps_with_markers() {
        let deps = |auto: &[&str], preserved: &[&str]| {
            Value::Deps(DepsValue {
                auto_deps: auto.iter().map(|s| s.to_string()).collect(),
                preserved_deps: preserved.iter().map(|s| s.to_string()).collect(),
                has_markers: true,
                raw_deps: vec![],
            })
        };
        assert_eq!(render(&deps(&[], &[])), "[]");
        assert_eq!(
            render_indented(&deps(&["//a:a"], &["//p:p"]), "    "),
            "[
        # turnkey:auto-start
        \"//a:a\",
        # turnkey:auto-end
        # turnkey:preserve-start
        \"//p:p\",
        # turnkey:preserve-end
    ]"
        );
        assert_eq!(
            render(&deps(&[], &["//p:p"])),
            "[
    # turnkey:auto-start
    # turnkey:auto-end
    # turnkey:preserve-start
    \"//p:p\",
    # turnkey:preserve-end
]"
        );
        let raw = Value::Deps(DepsValue {
            raw_deps: vec!["//r:r".into()],
            ..DepsValue::default()
        });
        assert_eq!(render(&raw), r#"["//r:r"]"#);
    }

    #[test]
    fn all_deps() {
        let deps = DepsValue {
            auto_deps: vec!["//a:a".into()],
            preserved_deps: vec!["//p:p".into()],
            has_markers: true,
            raw_deps: vec![],
        };
        assert_eq!(deps.all_deps(), ["//a:a", "//p:p"]);
    }
}
