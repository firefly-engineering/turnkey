//! The report of a rules sync, which tk prints: what rules sync did to each
//! `rules.star` file, or the error that stopped it
//!
//! Ported from Go's rulesreport, once the wire format between tk and a
//! separate rules-sync binary (#215). Now that tk calls rules sync as a
//! library, the report is never encoded.

/// The outcome of one rules-sync run
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Report {
    /// What sync did to each rules.star file it looked at; `None` when an
    /// error stopped it
    pub results: Option<Vec<Result>>,
    /// The error that stopped sync, if any
    pub error: Option<Error>,
}

/// Where sync stopped on an error
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stage {
    /// Reading the sync configuration and loading the languages' plug-ins,
    /// before any rules.star is read
    Setup,
    /// Walking the directory and syncing its rules.star files
    Sync,
}

/// The error that stopped sync
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Error {
    /// Where sync stopped
    pub stage: Stage,
    /// The error's text
    pub message: String,
}

/// What sync did to one rules.star file
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Result {
    /// The path to the rules.star file
    pub path: String,
    /// Whether the file was modified, or would be in a dry run
    pub updated: bool,
    /// Whether the file was skipped as up-to-date
    pub skipped: bool,
    /// Per target whose deps changed, what was added and removed, in the
    /// order the targets appear in the file
    pub changes: Vec<TargetChange>,
    /// The targets a "# turnkey:no-sync" comment opts out
    pub opted_out: Vec<String>,
    /// The targets sync skipped because their deps aren't a list of labels
    pub unreadable: Vec<UnreadableTarget>,
    /// The errors sync met in the file
    pub errors: Vec<String>,
}

/// A target whose deps attribute sync can't read
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct UnreadableTarget {
    /// The target's name
    pub target: String,
    /// The deps attribute that isn't a list of labels
    pub attribute: String,
}

/// How sync changed one target's deps, or another attribute it owns
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TargetChange {
    /// The target's name
    pub target: String,
    /// The attribute that changed, when it isn't the deps
    pub attribute: String,
    /// Dependencies added
    pub added: Vec<String>,
    /// Dependencies removed
    pub removed: Vec<String>,
    /// Dependencies sync would have removed but kept because the target
    /// has unmapped imports (in `unmapped`)
    pub kept: Vec<String>,
    /// The imports that made sync keep the deps in `kept`
    pub unmapped: Vec<String>,
}
