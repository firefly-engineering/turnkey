//! tk's own flags, before its subcommand (or among sync's and check's rule
//! names)

/// tk's flags
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Flags {
    /// `--no-sync`: skip dependency sync, run buck2 directly
    pub no_sync: bool,
    /// `--no-rules-sync`: skip rules.star sync (deps sync still runs)
    pub no_rules_sync: bool,
    /// `--strict-rules`: fail if rules.star files would change
    pub strict_rules: bool,
    /// `--no-local`: skip the local target overrides
    pub no_local: bool,
    /// `--rerun`: run every test instead of reusing recorded results
    pub rerun: bool,
    /// `--verbose`, `-v`: show what tk is doing
    pub verbose: bool,
    /// `--dry-run`, `-n`: show what would be synced without doing it
    pub dry_run: bool,
    /// `--quiet`, `-q`: suppress non-error output
    pub quiet: bool,
}

/// What [`Flags::parse`] found
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Parsed<'a, S> {
    /// The arguments after the flags
    Rest(&'a [S]),
    /// `--help` or `-h`: tk prints its help and exits 0
    Help,
}

/// Why [`Flags::rule_args`] stopped
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RuleArgsError {
    /// `--help` or `-h`: tk prints its help and exits 0
    Help,
    /// A flag that isn't tk's: tk says so and exits 2
    UnknownFlag(String),
}

impl Flags {
    /// Sets the flags at the start of `args`, up to the first argument
    /// that isn't one of them, and returns the arguments from there
    pub fn parse<'a, S: AsRef<str>>(&mut self, args: &'a [S]) -> Parsed<'a, S> {
        let mut i = 0;
        while i < args.len() {
            match args[i].as_ref() {
                "--no-sync" => self.no_sync = true,
                "--no-rules-sync" => self.no_rules_sync = true,
                "--strict-rules" => self.strict_rules = true,
                "--no-local" => self.no_local = true,
                "--rerun" => self.rerun = true,
                "--verbose" | "-v" => self.verbose = true,
                "--dry-run" | "-n" => self.dry_run = true,
                "--quiet" | "-q" => self.quiet = true,
                "--help" | "-h" => return Parsed::Help,
                // Not a tk flag: done
                _ => break,
            }
            i += 1;
        }
        Parsed::Rest(&args[i..])
    }

    /// The deps rule names given to sync or check, setting any tk flag
    /// among them, so that `tk sync --verbose go` works as
    /// `tk --verbose sync go` does
    pub fn rule_args<S: AsRef<str>>(&mut self, args: &[S]) -> Result<Vec<String>, RuleArgsError> {
        let mut rules = Vec::new();
        for arg in args {
            let arg = arg.as_ref();
            if !arg.starts_with('-') {
                rules.push(arg.to_string());
                continue;
            }
            match self.parse(&[arg]) {
                Parsed::Help => return Err(RuleArgsError::Help),
                Parsed::Rest([]) => {}
                Parsed::Rest(_) => return Err(RuleArgsError::UnknownFlag(arg.to_string())),
            }
        }
        Ok(rules)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_the_flags_before_the_subcommand() {
        let mut flags = Flags::default();
        let args = [
            "--no-sync",
            "-v",
            "--rerun",
            "-q",
            "test",
            "--no-local",
            "//...",
        ];
        assert_eq!(flags.parse(&args), Parsed::Rest(&args[4..]));
        assert_eq!(
            flags,
            Flags {
                no_sync: true,
                verbose: true,
                rerun: true,
                quiet: true,
                ..Default::default()
            }
        );

        let mut flags = Flags::default();
        let args = ["--no-rules-sync", "--strict-rules", "--no-local", "-n"];
        assert_eq!(flags.parse(&args), Parsed::Rest(&args[4..]));
        assert!(flags.no_rules_sync && flags.strict_rules && flags.no_local && flags.dry_run);

        for help in ["--help", "-h"] {
            assert_eq!(Flags::default().parse(&["-v", help, "build"]), Parsed::Help);
        }
        assert_eq!(Flags::default().parse::<&str>(&[]), Parsed::Rest(&[][..]));
    }

    #[test]
    fn rule_args_applies_flags_among_rule_names() {
        let mut flags = Flags::default();
        let rules = flags
            .rule_args(&["--verbose", "go", "-n", "python"])
            .unwrap();
        assert_eq!(rules, ["go", "python"]);
        assert!(flags.verbose && flags.dry_run);
    }

    #[test]
    fn rule_args_without_rule_names() {
        assert_eq!(Flags::default().rule_args::<&str>(&[]), Ok(vec![]));
    }

    #[test]
    fn rule_args_stops_at_help_or_an_unknown_flag() {
        assert_eq!(
            Flags::default().rule_args(&["go", "--frobnicate", "-h"]),
            Err(RuleArgsError::UnknownFlag("--frobnicate".into()))
        );
        assert_eq!(
            Flags::default().rule_args(&["go", "-h", "--frobnicate"]),
            Err(RuleArgsError::Help)
        );
    }
}
