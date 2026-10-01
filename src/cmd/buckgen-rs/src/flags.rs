//! buckgen's command line, as Go's flag package parsed it: `-name value`,
//! `-name=value`, with one or two dashes, the last of repeated flags
//! winning, until the first argument that isn't a flag (or `--`)

/// The flags and the arguments after them
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Flags {
    pub config: String,
    pub module_path: String,
    pub targets_out: String,
    pub imports_out: String,
    pub args: Vec<String>,
}

/// Why the command line was not parsed
#[derive(Debug, PartialEq, Eq)]
pub enum Error {
    /// -h or -help: the usage, and exit 0
    Help,
    /// A flag error: the message and the usage, and exit 2
    Invalid(String),
}

/// The defaults Go's flag package prints after an error or -h
pub fn usage() -> String {
    [
        ("config", "buckgen configuration (JSON)"),
        (
            "imports-out",
            "where to list the import paths their deps reference",
        ),
        (
            "module-path",
            "the module's path (its go.mod's module line)",
        ),
        ("targets-out", "where to list the rendered packages"),
    ]
    .iter()
    .fold("Usage of buckgen:\n".to_string(), |out, (name, usage)| {
        format!("{out}  -{name} string\n    \t{usage}\n")
    })
    .trim_end()
    .to_string()
}

/// Parses the arguments after the program name
pub fn parse(args: &[String]) -> Result<Flags, Error> {
    let mut flags = Flags::default();
    let mut i = 0;
    while i < args.len() {
        let s = &args[i];
        if s.len() < 2 || !s.starts_with('-') {
            break;
        }
        let mut minuses = 1;
        if s.as_bytes()[1] == b'-' {
            minuses = 2;
            if s.len() == 2 {
                i += 1;
                break;
            }
        }
        let name = &s[minuses..];
        if name.is_empty() || name.starts_with('-') || name.starts_with('=') {
            return Err(Error::Invalid(format!("bad flag syntax: {s}")));
        }
        i += 1;
        let (name, value) = match name.split_once('=') {
            Some((n, v)) => (n, Some(v.to_string())),
            None => (name, None),
        };
        let slot = match name {
            "config" => &mut flags.config,
            "module-path" => &mut flags.module_path,
            "targets-out" => &mut flags.targets_out,
            "imports-out" => &mut flags.imports_out,
            "help" | "h" => return Err(Error::Help),
            _ => {
                return Err(Error::Invalid(format!(
                    "flag provided but not defined: -{name}"
                )));
            }
        };
        *slot = match value {
            Some(v) => v,
            None if i < args.len() => {
                i += 1;
                args[i - 1].clone()
            }
            None => return Err(Error::Invalid(format!("flag needs an argument: -{name}"))),
        };
    }
    flags.args = args[i..].to_vec();
    Ok(flags)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_strs(args: &[&str]) -> Result<Flags, Error> {
        parse(&args.iter().map(|a| a.to_string()).collect::<Vec<_>>())
    }

    #[test]
    fn flags_as_go_parses_them() {
        let f = parse_strs(&[
            "--config",
            "c.json",
            "-module-path=example.com/m",
            "--targets-out=t",
            "-imports-out",
            "i",
            "--config",
            "d.json",
            "dir",
            "--config",
            "x",
        ])
        .unwrap();
        assert_eq!(
            f,
            Flags {
                config: "d.json".into(),
                module_path: "example.com/m".into(),
                targets_out: "t".into(),
                imports_out: "i".into(),
                args: vec!["dir".into(), "--config".into(), "x".into()],
            }
        );
        // -- ends the flags, and - is an argument
        assert_eq!(parse_strs(&["--", "-x"]).unwrap().args, ["-x"]);
        assert_eq!(parse_strs(&["-", "x"]).unwrap().args, ["-", "x"]);
        // A value can start with a dash, or be empty
        assert_eq!(parse_strs(&["-config", "-x"]).unwrap().config, "-x");
        assert_eq!(parse_strs(&["-config="]).unwrap().config, "");
    }

    #[test]
    fn flag_errors() {
        assert_eq!(parse_strs(&["-h"]), Err(Error::Help));
        assert_eq!(parse_strs(&["--help=x"]), Err(Error::Help));
        for bad in [&["-bogus"][..], &["-config"], &["---config", "x"], &["-=x"]] {
            assert!(matches!(parse_strs(bad), Err(Error::Invalid(_))), "{bad:?}");
        }
    }
}
