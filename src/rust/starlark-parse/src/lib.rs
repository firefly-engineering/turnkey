//! Parse Starlark/Buck2 build files using tree-sitter.
//!
//! This library provides utilities for extracting information from rules.star
//! files, such as target names, source patterns, and other build attributes.

use anyhow::{Context, Result};
use std::collections::HashMap;
use tree_sitter::{Parser, Query, QueryCursor, StreamingIterator};

/// A parsed Starlark target (e.g., rust_library, python_binary).
#[derive(Debug, Clone)]
pub struct Target {
    /// The rule type (e.g., "rust_library", "python_binary")
    pub rule: String,
    /// The target name from `name = "..."`
    pub name: String,
    /// All keyword arguments as key-value pairs
    pub args: HashMap<String, Value>,
    /// Line number where this target starts (1-indexed)
    pub line: usize,
}

/// A value in a Starlark expression.
#[derive(Debug, Clone)]
pub enum Value {
    /// A string literal
    String(String),
    /// A list of values
    List(Vec<Value>),
    /// A function call (e.g., glob(["**/*.rs"]))
    Call { func: String, args: Vec<Value> },
    /// An identifier reference
    Identifier(String),
    /// Other/unparsed expression
    Other(String),
}

impl Value {
    /// Try to get this value as a string.
    pub fn as_str(&self) -> Option<&str> {
        match self {
            Value::String(s) => Some(s),
            _ => None,
        }
    }

    /// Try to get this value as a list of strings.
    pub fn as_string_list(&self) -> Option<Vec<&str>> {
        match self {
            Value::List(items) => {
                let strings: Vec<_> = items.iter().filter_map(|v| v.as_str()).collect();
                if strings.len() == items.len() {
                    Some(strings)
                } else {
                    None
                }
            }
            _ => None,
        }
    }

    /// Collect all string patterns from this value, recursively.
    /// Handles glob() calls, lists, and direct strings.
    pub fn collect_patterns(&self) -> Vec<String> {
        match self {
            Value::String(s) => vec![s.clone()],
            Value::List(items) => items.iter().flat_map(|v| v.collect_patterns()).collect(),
            Value::Call { args, .. } => {
                // For glob() and similar, extract patterns from args
                args.iter().flat_map(|v| v.collect_patterns()).collect()
            }
            _ => vec![],
        }
    }
}

/// Parse a Starlark file and extract all targets.
pub fn parse_targets(source: &str) -> Result<Vec<Target>> {
    let mut parser = Parser::new();
    let language = tree_sitter_starlark::LANGUAGE;
    parser
        .set_language(&language.into())
        .context("Failed to set Starlark language")?;

    let tree = parser
        .parse(source, None)
        .context("Failed to parse Starlark source")?;

    // Query for function calls (potential targets)
    let query = Query::new(
        &language.into(),
        r#"
        (call
          function: (identifier) @func
          arguments: (argument_list) @args) @call
        "#,
    )
    .context("Failed to compile query")?;

    let mut cursor = QueryCursor::new();
    let mut targets = Vec::new();

    let func_idx = query.capture_index_for_name("func").unwrap();
    let args_idx = query.capture_index_for_name("args").unwrap();
    let call_idx = query.capture_index_for_name("call").unwrap();

    let mut matches = cursor.matches(&query, tree.root_node(), source.as_bytes());
    while let Some(match_) = matches.next() {
        let mut func_name = None;
        let mut args_node = None;
        let mut call_node = None;

        for capture in match_.captures {
            if capture.index == func_idx {
                func_name = Some(capture.node.utf8_text(source.as_bytes())?);
            } else if capture.index == args_idx {
                args_node = Some(capture.node);
            } else if capture.index == call_idx {
                call_node = Some(capture.node);
            }
        }

        if let (Some(func), Some(args), Some(call)) = (func_name, args_node, call_node) {
            // Parse the keyword arguments
            let parsed_args = parse_argument_list(args, source)?;

            // Only include if it has a "name" argument (it's a target definition)
            if let Some(Value::String(name)) = parsed_args.get("name") {
                targets.push(Target {
                    rule: func.to_string(),
                    name: name.clone(),
                    args: parsed_args,
                    line: call.start_position().row + 1,
                });
            }
        }
    }

    Ok(targets)
}

/// Parse an argument_list node into a map of keyword arguments.
fn parse_argument_list(
    node: tree_sitter::Node,
    source: &str,
) -> Result<HashMap<String, Value>> {
    let mut args = HashMap::new();

    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if child.kind() == "keyword_argument" {
            if let (Some(name_node), Some(value_node)) =
                (child.child_by_field_name("name"), child.child_by_field_name("value"))
            {
                let name = name_node.utf8_text(source.as_bytes())?;
                let value = parse_value(value_node, source)?;
                args.insert(name.to_string(), value);
            }
        }
    }

    Ok(args)
}

/// Parse a value node into a Value enum.
fn parse_value(node: tree_sitter::Node, source: &str) -> Result<Value> {
    match node.kind() {
        "string" => {
            // Extract string content without quotes
            let text = node.utf8_text(source.as_bytes())?;
            // Remove surrounding quotes (handles both ' and ")
            let content = text
                .trim_start_matches('"')
                .trim_start_matches('\'')
                .trim_end_matches('"')
                .trim_end_matches('\'');
            Ok(Value::String(content.to_string()))
        }
        "list" => {
            let mut items = Vec::new();
            let mut cursor = node.walk();
            for child in node.children(&mut cursor) {
                // Skip brackets and commas
                if child.kind() != "[" && child.kind() != "]" && child.kind() != "," {
                    items.push(parse_value(child, source)?);
                }
            }
            Ok(Value::List(items))
        }
        "call" => {
            let func_node = node.child_by_field_name("function");
            let args_node = node.child_by_field_name("arguments");

            let func = func_node
                .map(|n| n.utf8_text(source.as_bytes()))
                .transpose()?
                .unwrap_or("unknown")
                .to_string();

            let mut args = Vec::new();
            if let Some(args_list) = args_node {
                let mut cursor = args_list.walk();
                for child in args_list.children(&mut cursor) {
                    // Skip parentheses and commas
                    if child.kind() != "(" && child.kind() != ")" && child.kind() != "," {
                        // For positional args in glob(), etc.
                        if child.kind() != "keyword_argument" {
                            args.push(parse_value(child, source)?);
                        }
                    }
                }
            }

            Ok(Value::Call { func, args })
        }
        "identifier" => {
            let text = node.utf8_text(source.as_bytes())?;
            Ok(Value::Identifier(text.to_string()))
        }
        "true" => Ok(Value::Identifier("True".to_string())),
        "false" => Ok(Value::Identifier("False".to_string())),
        "none" => Ok(Value::Identifier("None".to_string())),
        "integer" | "float" => {
            let text = node.utf8_text(source.as_bytes())?;
            Ok(Value::Other(text.to_string()))
        }
        _ => {
            // For other types, capture the raw text
            let text = node.utf8_text(source.as_bytes())?;
            Ok(Value::Other(text.to_string()))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_simple_target() {
        let source = r#"
rust_library(
    name = "foo",
    srcs = glob(["src/**/*.rs"]),
    edition = "2024",
)
"#;
        let targets = parse_targets(source).unwrap();
        assert_eq!(targets.len(), 1);
        assert_eq!(targets[0].rule, "rust_library");
        assert_eq!(targets[0].name, "foo");
        assert_eq!(
            targets[0].args.get("edition").and_then(|v| v.as_str()),
            Some("2024")
        );
    }

    #[test]
    fn test_parse_multiple_targets() {
        let source = r#"
rust_library(
    name = "lib",
    srcs = ["lib.rs"],
)

rust_binary(
    name = "bin",
    srcs = ["main.rs"],
    deps = [":lib"],
)
"#;
        let targets = parse_targets(source).unwrap();
        assert_eq!(targets.len(), 2);
        assert_eq!(targets[0].name, "lib");
        assert_eq!(targets[1].name, "bin");
    }

    #[test]
    fn test_collect_glob_patterns() {
        let source = r#"
rust_library(
    name = "foo",
    srcs = glob(["src/**/*.rs", "lib/**/*.rs"]),
)
"#;
        let targets = parse_targets(source).unwrap();
        let srcs = targets[0].args.get("srcs").unwrap();
        let patterns = srcs.collect_patterns();
        assert_eq!(patterns, vec!["src/**/*.rs", "lib/**/*.rs"]);
    }

    #[test]
    fn test_explicit_file_list() {
        let source = r#"
python_binary(
    name = "main",
    main = "main.py",
    srcs = ["main.py", "utils.py"],
)
"#;
        let targets = parse_targets(source).unwrap();
        assert_eq!(targets[0].args.get("main").and_then(|v| v.as_str()), Some("main.py"));
        let srcs = targets[0].args.get("srcs").unwrap();
        let patterns = srcs.collect_patterns();
        assert_eq!(patterns, vec!["main.py", "utils.py"]);
    }
}
