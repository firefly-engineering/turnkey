//! Writing a file back: its source with only what changed rewritten

use crate::{Attribute, File, Load, SelectValue, Span, Target, Value, render, render_indented};
use gostd::strconv::quote;

/// A replacement of the source bytes `[start, end)` with `text`
struct Edit {
    start: usize,
    end: usize,
    text: String,
}

impl File {
    /// The file's source with what changed rewritten: a changed target's
    /// changed values (only the changed parts of a `select()` whose keys are
    /// unchanged) and its new attributes, before its closing parenthesis.
    /// Where that isn't possible, the whole target is written again. New
    /// targets go first. Everything else is copied as it was.
    pub fn write(&self) -> String {
        if !self.is_modified() {
            return self.source.clone();
        }

        enum Item<'a> {
            Load(&'a Load),
            Target(&'a Target),
        }
        let mut items: Vec<(Span, bool, Item)> = self
            .loads
            .iter()
            .map(|load| (load.span, false, Item::Load(load)))
            .chain(
                self.targets
                    .iter()
                    .map(|t| (t.span, t.modified, Item::Target(t))),
            )
            .collect();
        items.sort_by_key(|item| item.0.start);

        let mut out = String::new();
        let mut pos = 0;
        let write = |out: &mut String, item: &Item| match item {
            Item::Load(load) => write_load(out, load),
            Item::Target(target) => {
                // Rewrite only the attributes that changed, if possible
                if target.span.end > 0
                    && let Some(text) = splice_target(&self.source, target)
                {
                    out.push_str(&text);
                    return;
                }
                write_target(out, target);
            }
        };
        for (span, modified, item) in &items {
            if span.start == 0 && span.end == 0 && *modified {
                // A new target
                write(&mut out, item);
                out.push('\n');
                continue;
            }
            if span.start > pos {
                out.push_str(&self.source[pos..span.start]);
            }
            if *modified {
                write(&mut out, item);
            } else {
                out.push_str(&self.source[span.start..span.end]);
            }
            pos = span.end;
        }
        if pos < self.source.len() {
            out.push_str(&self.source[pos..]);
        }
        out
    }

    /// The file written again from its model: loads, then targets, each
    /// formatted the same way, comments and all else dropped.
    pub fn write_formatted(&self) -> String {
        let mut out = String::new();
        for load in &self.loads {
            write_load(&mut out, load);
            out.push('\n');
        }
        if !self.loads.is_empty() && !self.targets.is_empty() {
            out.push('\n');
        }
        for (i, target) in self.targets.iter().enumerate() {
            if i > 0 {
                out.push('\n');
            }
            write_target(&mut out, target);
            out.push('\n');
        }
        out
    }
}

/// A load statement, on one line
fn write_load(out: &mut String, load: &Load) {
    out.push_str("load(");
    out.push_str(&quote(&load.module));
    for sym in &load.symbols {
        out.push_str(", ");
        if sym.name == sym.original {
            out.push_str(&quote(&sym.original));
        } else {
            out.push_str(&sym.name);
            out.push_str(" = ");
            out.push_str(&quote(&sym.original));
        }
    }
    out.push(')');
}

/// A target, one attribute per line
fn write_target(out: &mut String, target: &Target) {
    out.push_str(&target.rule);
    out.push_str("(\n");
    let mut order = target.attribute_order.clone();
    if order.is_empty() {
        order = target.attributes.keys().cloned().collect();
        order.sort();
    }
    for name in &order {
        let Some(attr) = target.attributes.get(name) else {
            continue;
        };
        out.push_str("    ");
        out.push_str(name);
        out.push_str(" = ");
        out.push_str(&render_indented(&attr.value, "    "));
        out.push_str(",\n");
    }
    out.push(')');
}

/// A changed target's source with only what changed rewritten: each
/// changed attribute's value, and new attributes inserted before the
/// closing parenthesis. `None` if that isn't possible (an attribute was
/// removed, or the call's layout leaves no clean place for a new one).
fn splice_target(src: &str, t: &Target) -> Option<String> {
    if t.modified_attrs
        .iter()
        .any(|name| !t.attributes.contains_key(name))
    {
        return None;
    }

    let mut edits = Vec::new();
    for name in &t.attribute_order {
        if !t.modified_attrs.contains(name) {
            continue;
        }
        let attr = &t.attributes[name];
        if attr.syntax.is_none() {
            edits.push(insert_attribute(src, t, attr)?);
            continue;
        }
        edits.extend(value_edits(src, attr));
    }
    edits.sort_by_key(|e| e.start);

    let mut out = String::new();
    let mut pos = t.span.start;
    for e in edits {
        out.push_str(&src[pos..e.start]);
        out.push_str(&e.text);
        pos = e.end;
    }
    out.push_str(&src[pos..t.span.end]);
    Some(out)
}

/// The edits that rewrite an existing attribute's value
fn value_edits(src: &str, attr: &Attribute) -> Vec<Edit> {
    let syntax = attr.syntax.as_ref().expect("a parsed attribute");
    if let (Value::Select(sel), Some(Value::Select(orig)), Some(parts)) =
        (&attr.value, &attr.original, &syntax.select)
        && let Some(edits) = select_edits(src, parts, orig, sel)
    {
        return edits;
    }
    let span = syntax.span;
    vec![Edit {
        start: span.start,
        end: span.end,
        text: render_indented(&attr.value, &line_indent(src, span.start)),
    }]
}

/// The edits that turn the `select()` at `parts`, parsed as `orig`, into
/// `sel`: one per changed part. `None` if the two have different shapes
/// (a plain part added or removed, different keys), which needs the whole
/// value rewritten.
fn select_edits(
    src: &str,
    parts: &crate::SelectSyntax,
    orig: &SelectValue,
    sel: &SelectValue,
) -> Option<Vec<Edit>> {
    if orig.common.is_none() != sel.common.is_none()
        || orig.branches.len() != sel.branches.len()
        || orig
            .branches
            .iter()
            .zip(&sel.branches)
            .any(|(a, b)| a.key != b.key)
    {
        return None;
    }

    let mut edits = Vec::new();
    let mut replace = |span: Span, before: Option<&Value>, after: Option<&Value>| {
        let render_opt = |v: Option<&Value>| v.map(render).unwrap_or_default();
        if render_opt(before) == render_opt(after) {
            return;
        }
        let indent = line_indent(src, span.start);
        edits.push(Edit {
            start: span.start,
            end: span.end,
            text: after
                .map(|v| render_indented(v, &indent))
                .unwrap_or_default(),
        });
    };
    if let Some(common) = parts.common {
        replace(common, orig.common.as_deref(), sel.common.as_deref());
    }
    for (i, &span) in parts.branches.iter().enumerate() {
        replace(
            span,
            Some(&orig.branches[i].value),
            Some(&sel.branches[i].value),
        );
    }
    Some(edits)
}

/// The edit that adds a new attribute on its own line before the call's
/// closing parenthesis, indented like the last argument. `None` unless the
/// parenthesis is on its own line and the last argument ends with a comma.
fn insert_attribute(src: &str, t: &Target, attr: &Attribute) -> Option<Edit> {
    let call = t.call.as_ref()?;
    let last = *call.args.last()?;
    let bytes = src.as_bytes();
    let rparen = call.rparen;
    let mut line_start = rparen;
    while line_start > 0 && bytes[line_start - 1] != b'\n' {
        line_start -= 1;
    }
    if !trim_space(&src[line_start..rparen]).is_empty() {
        return None;
    }
    if last.end > line_start || !trim_space(&src[last.end..line_start]).starts_with(',') {
        return None;
    }
    let indent = line_indent(src, last.start);
    let text = format!(
        "{indent}{} = {},\n",
        attr.name,
        render_indented(&attr.value, &indent)
    );
    Some(Edit {
        start: line_start,
        end: line_start,
        text,
    })
}

/// `s` without leading and trailing white space, as Go's
/// `strings.TrimSpace` trims it
fn trim_space(s: &str) -> &str {
    s.trim_matches(gostd::unicode::is_space)
}

/// The leading spaces and tabs of the line holding `offset`
fn line_indent(src: &str, offset: usize) -> String {
    let bytes = src.as_bytes();
    let mut start = offset;
    while start > 0 && bytes[start - 1] != b'\n' {
        start -= 1;
    }
    let mut end = start;
    while end < bytes.len() && (bytes[end] == b' ' || bytes[end] == b'\t') {
        end += 1;
    }
    src[start..end].to_string()
}
