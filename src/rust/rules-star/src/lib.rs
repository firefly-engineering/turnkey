//! rules-star: `rules.star` files read, edited and written back, rewriting
//! only what changed
//!
//! A [`File`] is a `rules.star` parsed into its load statements and its
//! targets (the rule calls with a `name`), each target into its
//! attributes, and each attribute's value into a [`Value`]: a string, a
//! list of labels (with turnkey's `# turnkey:auto-start` /
//! `# turnkey:preserve-start` sections, [`DepsValue`]), a
//! `[...] + select({...})` ([`SelectValue`]), or any other expression,
//! kept as written. Editing a target (`set_labels`, `set_select`, ...)
//! records what changed, and [`File::write`] copies the source, rewriting
//! only the values that changed and inserting new attributes, so comments
//! and layout elsewhere are kept byte for byte. [`conditional`] reads and
//! writes an attribute whose value depends on the build configuration.
//!
//! Ported from Go's starlark package and conditional's Read, ReadLabels
//! and SetLabels, for rules sync (#215). What it reads
//! from a file is what go.starlark.net's syntax tree holds (the `syntax`
//! module rebuilds it from starlark_syntax's), and what it writes is
//! byte-identical to the Go version's: values are written with
//! deps-gen-kit's Starlark writer, which quotes as Go's `strconv.Quote`.

pub mod conditional;
mod edit;
mod syntax;
mod write;

pub use deps_gen_kit::starlark::{
    DepsValue, SelectBranch, SelectValue, Value, render, render_indented,
};

use std::collections::{HashMap, HashSet};
use std::fmt;
use std::path::Path;
use syntax::{Kind, NodeId, Op, Tree};

/// A byte range of a file's source
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Span {
    /// Where it starts
    pub start: usize,
    /// Where it ends, exclusive
    pub end: usize,
}

/// Why a `rules.star` couldn't be read
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Error(String);

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for Error {}

/// A parsed `rules.star`, with what was changed in it
#[derive(Debug, Clone)]
pub struct File {
    /// The file's path
    pub path: String,
    /// The source as parsed
    pub source: String,
    /// The load statements, in order
    pub loads: Vec<Load>,
    /// The targets: rule calls with a `name`, in order
    pub targets: Vec<Target>,
    /// Whether a target was added or removed
    modified: bool,
}

/// A load statement
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Load {
    /// The module loaded, e.g. `@prelude//:rules.bzl`
    pub module: String,
    /// The symbols it loads
    pub symbols: Vec<LoadSymbol>,
    span: Span,
}

/// A symbol a load statement loads
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoadSymbol {
    /// The name it is bound to
    pub name: String,
    /// Its name in the module
    pub original: String,
}

/// A target: a call of a rule, e.g. `go_library(name = "x", ...)`
#[derive(Debug, Clone)]
pub struct Target {
    /// The rule, e.g. `go_library`
    pub rule: String,
    /// The target's name, its `name` attribute
    pub name: String,
    /// Set when a `# turnkey:no-sync` comment precedes the call: rules
    /// sync leaves the target alone
    pub no_sync: bool,
    attributes: HashMap<String, Attribute>,
    /// The attributes' names, in order
    attribute_order: Vec<String>,
    /// The call as parsed; `None` for a new target
    call: Option<CallSyntax>,
    span: Span,
    modified: bool,
    modified_attrs: HashSet<String>,
}

/// Where a parsed call's parts are
#[derive(Debug, Clone)]
struct CallSyntax {
    /// Where its `)` is
    rparen: usize,
    /// Its arguments
    args: Vec<Span>,
}

/// A target's attribute, e.g. `deps = [...]`
#[derive(Debug, Clone)]
pub struct Attribute {
    /// The attribute's name
    pub name: String,
    /// Its value
    pub value: Value,
    /// Where its value is, as parsed; `None` for a new attribute
    syntax: Option<ValueSyntax>,
    /// Its value as parsed, compared with `value` to rewrite only what
    /// changed
    original: Option<Value>,
    modified: bool,
}

/// Where an attribute's parsed value is
#[derive(Debug, Clone)]
struct ValueSyntax {
    span: Span,
    /// For a `select()`: where its parts are
    select: Option<SelectSyntax>,
}

/// Where a parsed `[<common>] + select({...})`'s parts are
#[derive(Debug, Clone)]
struct SelectSyntax {
    /// The plain part, if any
    common: Option<Span>,
    /// Each entry's value
    branches: Vec<Span>,
}

/// A label-list value's labels: a list's, or all of a list with markers'.
/// No value has none. `None` for any other value.
pub fn labels(value: Option<&Value>) -> Option<Vec<String>> {
    match value {
        None => Some(Vec::new()),
        Some(Value::StringList(values)) => Some(values.clone()),
        Some(Value::Deps(deps)) => Some(deps.all_deps()),
        Some(_) => None,
    }
}

impl File {
    /// Whether anything was changed
    pub fn is_modified(&self) -> bool {
        self.modified || self.targets.iter().any(|t| t.modified)
    }

    /// The target named `name`
    pub fn get_target(&self, name: &str) -> Option<&Target> {
        self.targets.iter().find(|t| t.name == name)
    }

    /// The target named `name`, to edit
    pub fn get_target_mut(&mut self, name: &str) -> Option<&mut Target> {
        self.targets.iter_mut().find(|t| t.name == name)
    }
}

impl Target {
    /// Whether the target was changed
    pub fn is_modified(&self) -> bool {
        self.modified
    }

    /// The attribute named `name`
    pub fn get_attribute(&self, name: &str) -> Option<&Attribute> {
        self.attributes.get(name)
    }

    /// The attributes' names, in order
    pub fn attribute_names(&self) -> &[String] {
        &self.attribute_order
    }

    /// The deps attribute's labels (see [`Target::get_labels`])
    pub fn get_deps(&self) -> Vec<String> {
        self.get_labels("deps")
    }

    /// A label-list attribute's labels (deps, npm_deps, ...), all of them
    /// with markers; none if it is absent or not a list of labels.
    pub fn get_labels(&self, name: &str) -> Vec<String> {
        match self.get_attribute(name).map(|a| &a.value) {
            Some(Value::StringList(values)) => values.clone(),
            Some(Value::Deps(deps)) => deps.all_deps(),
            _ => Vec::new(),
        }
    }

    /// The deps' auto-managed labels: all of them without markers
    pub fn get_auto_deps(&self) -> Vec<String> {
        match self.get_attribute("deps").map(|a| &a.value) {
            Some(Value::Deps(deps)) if deps.has_markers => deps.auto_deps.clone(),
            Some(Value::Deps(deps)) => deps.raw_deps.clone(),
            Some(Value::StringList(values)) => values.clone(),
            _ => Vec::new(),
        }
    }

    /// The deps' preserved labels (see [`Target::get_preserved_labels`])
    pub fn get_preserved_deps(&self) -> Vec<String> {
        self.get_preserved_labels("deps")
    }

    /// A label-list attribute's preserved labels, those outside its
    /// auto-managed section when it has markers, which may be the plain
    /// part of a `select()`
    pub fn get_preserved_labels(&self, name: &str) -> Vec<String> {
        let mut value = self.get_attribute(name).map(|a| &a.value);
        if let Some(Value::Select(sel)) = value {
            value = sel.common.as_deref();
        }
        match value {
            Some(Value::Deps(deps)) => deps.preserved_deps.clone(),
            _ => Vec::new(),
        }
    }

    /// An attribute written as `[<common>] + select({...})`
    pub fn get_select(&self, name: &str) -> Option<&SelectValue> {
        match self.get_attribute(name).map(|a| &a.value) {
            Some(Value::Select(sel)) => Some(sel),
            _ => None,
        }
    }

    /// A string attribute's value, `""` if it is absent or not a string
    pub fn get_string_attr(&self, name: &str) -> &str {
        match self.get_attribute(name).map(|a| &a.value) {
            Some(Value::String(s)) => s,
            _ => "",
        }
    }
}

/// Parses the `rules.star` at `path`.
pub fn parse_file(path: &Path) -> Result<File, Error> {
    let data = std::fs::read(path).map_err(|err| {
        Error(format!(
            "reading file: {}",
            io_error_text("open", &path.to_string_lossy(), &err)
        ))
    })?;
    let source = String::from_utf8(data)
        .map_err(|err| Error(format!("parsing starlark: {}: {err}", path.display())))?;
    parse(&path.to_string_lossy(), source)
}

/// An I/O error as Go's `*fs.PathError` prints it: `<op> <path>: <error>`,
/// with the error as Go names it ("no such file or directory").
fn io_error_text(op: &str, path: &str, err: &std::io::Error) -> String {
    let text = err.to_string();
    let text = match text.find(" (os error ") {
        Some(i) => &text[..i],
        None => &text,
    };
    let mut chars = text.chars();
    let text = match chars.next() {
        Some(first) => first.to_lowercase().chain(chars).collect(),
        None => String::new(),
    };
    format!("{op} {path}: {text}")
}

/// Parses `rules.star` source; `path` names it in errors.
pub fn parse(path: &str, source: String) -> Result<File, Error> {
    let tree =
        syntax::parse(path, &source).map_err(|err| Error(format!("parsing starlark: {err}")))?;
    let mut file = File {
        path: path.to_string(),
        source,
        loads: Vec::new(),
        targets: Vec::new(),
        modified: false,
    };
    let reader = Reader {
        tree: &tree,
        source: &file.source,
    };
    for &stmt in tree.stmts() {
        let node = tree.node(stmt);
        match &node.kind {
            Kind::Load { module, symbols } => file.loads.push(Load {
                module: module.clone(),
                symbols: symbols
                    .iter()
                    .map(|(name, original)| LoadSymbol {
                        name: name.clone(),
                        original: original.clone(),
                    })
                    .collect(),
                span: reader.span(stmt),
            }),
            Kind::ExprStmt => {
                let call = node.children[0];
                if let Some(mut target) = reader.target(call) {
                    target.no_sync = has_no_sync_marker(&tree, stmt);
                    file.targets.push(target);
                }
            }
            _ => {}
        }
    }
    Ok(file)
}

/// The comment that opts the rule call it precedes out of rules sync
const NO_SYNC_MARKER: &str = "turnkey:no-sync";

/// What a comment says: its text after the `#`, trimmed as Go's
/// `strings.TrimSpace` trims
fn comment_body(text: &str) -> &str {
    text.strip_prefix('#')
        .unwrap_or(text)
        .trim_matches(gostd::unicode::is_space)
}

/// Whether a `# turnkey:no-sync` comment is attached before `stmt`
fn has_no_sync_marker(tree: &Tree, stmt: NodeId) -> bool {
    tree.node(stmt)
        .comments
        .before
        .iter()
        .any(|c| comment_body(&c.text) == NO_SYNC_MARKER)
}

/// Reads the model from a syntax tree
struct Reader<'a> {
    tree: &'a Tree,
    source: &'a str,
}

/// A turnkey marker inside a list
struct Marker {
    start: usize,
    /// `turnkey:auto-start`; every other marker ends the auto section
    opens_auto: bool,
}

impl Reader<'_> {
    /// A node's span: the bytes of its whole text
    fn span(&self, id: NodeId) -> Span {
        let node = self.tree.node(id);
        Span {
            start: node.start,
            end: node.end,
        }
    }

    /// A node's text, as written
    fn text(&self, id: NodeId) -> String {
        let span = self.span(id);
        if span.start < span.end && span.end <= self.source.len() {
            self.source[span.start..span.end].to_string()
        } else {
            String::new()
        }
    }

    /// A target, from a rule call: `None` if the call isn't of a name, or
    /// has no `name` attribute
    fn target(&self, call: NodeId) -> Option<Target> {
        let node = self.tree.node(call);
        let Kind::Call { rparen } = node.kind else {
            return None;
        };
        let Kind::Ident(rule) = &self.tree.node(node.children[0]).kind else {
            return None;
        };
        let args = &node.children[1..];
        let mut target = Target {
            rule: rule.clone(),
            name: String::new(),
            no_sync: false,
            attributes: HashMap::new(),
            attribute_order: Vec::new(),
            call: Some(CallSyntax {
                rparen,
                args: args.iter().map(|&a| self.span(a)).collect(),
            }),
            span: self.span(call),
            modified: false,
            modified_attrs: HashSet::new(),
        };
        for &arg in args {
            let arg_node = self.tree.node(arg);
            // Only named arguments are attributes
            let Kind::Binary(Op::Eq) = arg_node.kind else {
                continue;
            };
            let Kind::Ident(name) = &self.tree.node(arg_node.children[0]).kind else {
                continue;
            };
            let value_node = arg_node.children[1];
            let value = self.attribute_value(value_node);
            let select = match value {
                Value::Select(_) => Some(self.select_syntax(value_node)),
                _ => None,
            };
            if name == "name"
                && let Value::String(s) = &value
            {
                target.name = s.clone();
            }
            target.attributes.insert(
                name.clone(),
                Attribute {
                    name: name.clone(),
                    value: value.clone(),
                    syntax: Some(ValueSyntax {
                        span: self.span(value_node),
                        select,
                    }),
                    original: Some(value),
                    modified: false,
                },
            );
            target.attribute_order.push(name.clone());
        }
        (!target.name.is_empty()).then_some(target)
    }

    /// An attribute's value: a list may carry turnkey markers, and
    /// `[...] + select({...})` or `select({...})` is a select value.
    fn attribute_value(&self, id: NodeId) -> Value {
        if let Kind::List = self.tree.node(id).kind {
            return self.deps_value(id);
        }
        if let Some(sel) = self.select(id) {
            return Value::Select(sel);
        }
        self.value(id)
    }

    /// `[<list>] + select({...})` or `select({...})`, whose keys are
    /// strings. Any other shape is not a select value.
    fn select(&self, id: NodeId) -> Option<SelectValue> {
        let node = self.tree.node(id);
        let mut common = None;
        let mut call = id;
        if let Kind::Binary(Op::Plus) = node.kind {
            let list = node.children[0];
            if !matches!(self.tree.node(list).kind, Kind::List) {
                return None;
            }
            let value = self.deps_value(list);
            labels(Some(&value))?;
            common = Some(Box::new(value));
            call = node.children[1];
        }
        let dict = self.select_dict(call)?;
        let mut sel = SelectValue {
            common,
            branches: Vec::new(),
        };
        for &entry in &self.tree.node(dict).children {
            let entry = self.tree.node(entry);
            let Kind::String(key) = &self.tree.node(entry.children[0]).kind else {
                return None;
            };
            sel.branches.push(SelectBranch {
                key: key.clone(),
                value: self.attribute_value(entry.children[1]),
            });
        }
        Some(sel)
    }

    /// The dict of `select({...})`, a call of `select` with one argument
    fn select_dict(&self, call: NodeId) -> Option<NodeId> {
        let node = self.tree.node(call);
        if !matches!(node.kind, Kind::Call { .. }) || node.children.len() != 2 {
            return None;
        }
        match &self.tree.node(node.children[0]).kind {
            Kind::Ident(name) if name == "select" => {}
            _ => return None,
        }
        let dict = node.children[1];
        matches!(self.tree.node(dict).kind, Kind::Dict).then_some(dict)
    }

    /// Where a select value's parts are
    fn select_syntax(&self, id: NodeId) -> SelectSyntax {
        let node = self.tree.node(id);
        let (common, call) = match node.kind {
            Kind::Binary(Op::Plus) => (Some(self.span(node.children[0])), node.children[1]),
            _ => (None, id),
        };
        let dict = self.select_dict(call).expect("a select value's dict");
        let branches = self
            .tree
            .node(dict)
            .children
            .iter()
            .map(|&entry| self.span(self.tree.node(entry).children[1]))
            .collect();
        SelectSyntax { common, branches }
    }

    /// Any other value
    fn value(&self, id: NodeId) -> Value {
        let node = self.tree.node(id);
        match &node.kind {
            // The parser decoded the literal, whichever quotes it was
            // written with
            Kind::String(s) => Value::String(s.clone()),
            Kind::Int(raw) => Value::Int(parse_int(raw)),
            Kind::Ident(name) if name == "True" => Value::Bool(true),
            Kind::Ident(name) if name == "False" => Value::Bool(false),
            Kind::Ident(name) => Value::Ident(name.clone()),
            Kind::List => self.list_value(id),
            _ => Value::Expr(self.text(id)),
        }
    }

    /// A list: of strings, or any other expression
    fn list_value(&self, id: NodeId) -> Value {
        let mut strings = Vec::new();
        for &elem in &self.tree.node(id).children {
            match &self.tree.node(elem).kind {
                Kind::String(s) => strings.push(s.clone()),
                _ => return Value::Expr(self.text(id)),
            }
        }
        Value::StringList(strings)
    }

    /// A label list with marker support: the labels between
    /// `turnkey:auto-start` and `turnkey:auto-end` are auto-managed; every
    /// other label (between `turnkey:preserve-start` and
    /// `turnkey:preserve-end`, or outside any markers) is preserved, since
    /// a person wrote it. The markers are read from the syntax tree's
    /// comments, so any layout works, including several labels on a line.
    fn deps_value(&self, list: NodeId) -> Value {
        let markers = self.list_markers(list);
        if markers.is_empty() {
            return self.list_value(list);
        }
        let mut deps = DepsValue {
            has_markers: true,
            ..DepsValue::default()
        };
        let mut in_auto = false;
        let mut next = 0;
        for &elem in &self.tree.node(list).children {
            let elem = self.tree.node(elem);
            while next < markers.len() && markers[next].start < elem.start {
                in_auto = markers[next].opens_auto;
                next += 1;
            }
            let Kind::String(dep) = &elem.kind else {
                // Rewriting the labels would drop it: not a label list
                return Value::Expr(self.text(list));
            };
            if in_auto {
                deps.auto_deps.push(dep.clone());
            } else {
                deps.preserved_deps.push(dep.clone());
            }
        }
        Value::Deps(deps)
    }

    /// The turnkey markers among the comments attached in `list`, in
    /// source order
    fn list_markers(&self, list: NodeId) -> Vec<Marker> {
        let mut markers = Vec::new();
        for id in self.tree.walk(list) {
            let comments = &self.tree.node(id).comments;
            for c in comments.before.iter().chain(&comments.suffix) {
                let opens_auto = match comment_body(&c.text) {
                    "turnkey:auto-start" => true,
                    "turnkey:auto-end" | "turnkey:preserve-start" | "turnkey:preserve-end" => false,
                    _ => continue,
                };
                markers.push(Marker {
                    start: c.start,
                    opens_auto,
                });
            }
        }
        markers.sort_by_key(|m| m.start);
        markers
    }
}

/// An integer literal's value, as Go's `strconv.ParseInt(raw, 0, 64)`
/// reads it: decimal, or `0x`, `0o`, `0b` (and a leading `0`, octal); out
/// of range, the bound it passes; malformed, 0.
fn parse_int(raw: &str) -> i64 {
    let lower = raw.to_ascii_lowercase();
    let (digits, radix) = if let Some(d) = lower.strip_prefix("0x") {
        (d, 16)
    } else if let Some(d) = lower.strip_prefix("0o") {
        (d, 8)
    } else if let Some(d) = lower.strip_prefix("0b") {
        (d, 2)
    } else if lower.len() > 1 && lower.starts_with('0') {
        (&lower[1..], 8)
    } else {
        (lower.as_str(), 10)
    };
    if digits.is_empty() || !digits.chars().all(|c| c.is_digit(radix)) {
        return 0;
    }
    i64::from_str_radix(digits, radix).unwrap_or(i64::MAX)
}

#[cfg(test)]
mod tests;
