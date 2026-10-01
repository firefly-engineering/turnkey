//! The report of a rules sync, which tk prints: what rules sync did to each
//! `rules.star` file, or the error that stopped it
//!
//! Ported from Go's rulesreport, once the wire format between tk and a
//! separate rules-sync binary (#215). It still writes itself as JSON, as
//! Go's `json.Encoder` writes it.

use serde::Serialize;

/// The outcome of one rules-sync run
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct Report {
    /// What sync did to each rules.star file it looked at; `None` (null)
    /// when an error stopped it
    pub results: Option<Vec<Result>>,
    /// The error that stopped sync, if any
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<Error>,
}

/// Where sync stopped on an error
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Stage {
    /// Reading the sync configuration and loading the languages' plug-ins,
    /// before any rules.star is read
    Setup,
    /// Walking the directory and syncing its rules.star files
    Sync,
}

/// The error that stopped sync
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Error {
    /// Where sync stopped
    pub stage: Stage,
    /// The error's text
    pub message: String,
}

/// What sync did to one rules.star file
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct Result {
    /// The path to the rules.star file
    pub path: String,
    /// Whether the file was modified, or would be in a dry run
    pub updated: bool,
    /// Whether the file was skipped as up-to-date
    pub skipped: bool,
    /// Per target whose deps changed, what was added and removed, in the
    /// order the targets appear in the file
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub changes: Vec<TargetChange>,
    /// The targets a "# turnkey:no-sync" comment opts out
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub opted_out: Vec<String>,
    /// The targets sync skipped because their deps aren't a list of labels
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub unreadable: Vec<UnreadableTarget>,
    /// The errors sync met in the file
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub errors: Vec<String>,
}

/// A target whose deps attribute sync can't read
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct UnreadableTarget {
    /// The target's name
    pub target: String,
    /// The deps attribute that isn't a list of labels
    pub attribute: String,
}

/// How sync changed one target's deps, or another attribute it owns
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct TargetChange {
    /// The target's name
    pub target: String,
    /// The attribute that changed, when it isn't the deps
    #[serde(skip_serializing_if = "String::is_empty")]
    pub attribute: String,
    /// Dependencies added
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub added: Vec<String>,
    /// Dependencies removed
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub removed: Vec<String>,
    /// Dependencies sync would have removed but kept because the target
    /// has unmapped imports (in `unmapped`)
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub kept: Vec<String>,
    /// The imports that made sync keep the deps in `kept`
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub unmapped: Vec<String>,
}

impl Report {
    /// The report as Go's `json.Encoder` encodes it: compact, on one line
    /// ending with a newline, with `<`, `>`, `&`, U+2028 and U+2029 in
    /// strings escaped.
    pub fn to_json(&self) -> String {
        let mut out = Vec::new();
        let mut ser = serde_json::Serializer::with_formatter(&mut out, GoFormatter);
        self.serialize(&mut ser)
            .expect("a report always serializes");
        let mut json = String::from_utf8(out).expect("JSON is UTF-8");
        json.push('\n');
        json
    }
}

/// Writes JSON as Go's encoding/json does with HTML escaping on, its
/// default
struct GoFormatter;

impl serde_json::ser::Formatter for GoFormatter {
    fn write_string_fragment<W: ?Sized + std::io::Write>(
        &mut self,
        writer: &mut W,
        fragment: &str,
    ) -> std::io::Result<()> {
        let mut start = 0;
        for (i, c) in fragment.char_indices() {
            let escape = match c {
                '<' => "\\u003c",
                '>' => "\\u003e",
                '&' => "\\u0026",
                '\u{2028}' => "\\u2028",
                '\u{2029}' => "\\u2029",
                _ => continue,
            };
            writer.write_all(&fragment.as_bytes()[start..i])?;
            writer.write_all(escape.as_bytes())?;
            start = i + c.len_utf8();
        }
        writer.write_all(&fragment.as_bytes()[start..])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Fields left out when empty are, and an error leaves the results null
    #[test]
    fn json_as_go_writes_it() {
        let report = Report {
            results: Some(vec![
                Result {
                    path: "a/rules.star".into(),
                    updated: true,
                    changes: vec![TargetChange {
                        target: "lib".into(),
                        added: vec!["//x:x".into()],
                        ..TargetChange::default()
                    }],
                    ..Result::default()
                },
                Result {
                    path: "b/rules.star".into(),
                    skipped: true,
                    errors: vec!["needs <x> & \u{2028}\"y\"\n".into()],
                    ..Result::default()
                },
            ]),
            error: None,
        };
        assert_eq!(
            report.to_json(),
            concat!(
                r#"{"results":[{"path":"a/rules.star","updated":true,"skipped":false,"changes":[{"target":"lib","added":["//x:x"]}]},"#,
                r#"{"path":"b/rules.star","updated":false,"skipped":true,"errors":["needs \u003cx\u003e \u0026 \u2028\"y\"\n"]}]}"#,
                "\n"
            )
        );
        let failed = Report {
            results: None,
            error: Some(Error {
                stage: Stage::Setup,
                message: "no languages".into(),
            }),
        };
        assert_eq!(
            failed.to_json(),
            "{\"results\":null,\"error\":{\"stage\":\"setup\",\"message\":\"no languages\"}}\n"
        );
    }
}
