//! A target's variant attributes, read per configuration

use conditions::{Configuration, Space};
use rules_star::Value;
use rules_star::conditional::{self, Reading};
use std::collections::BTreeMap;

/// The variant attributes a target sets in one configuration, by name
pub type Variant = BTreeMap<String, Value>;

/// A target's variant attributes, read in each configuration of a space
#[derive(Debug, Clone)]
pub struct VariantReading(Vec<(String, Reading)>);

impl VariantReading {
    /// The variant in `config`: each attribute with a value there (an
    /// unset one, or a `select()` alone with no branch for it, is absent)
    pub fn get(&self, config: &Configuration) -> Variant {
        self.0
            .iter()
            .filter_map(|(name, read)| read.value(config).map(|v| (name.clone(), v)))
            .collect()
    }
}

/// Reads a target's variant attributes (`attrs`) as their values in each
/// configuration of `space`, as [`conditional::read`] reads them. It is an
/// error, naming the attribute, if one can't be read.
pub fn read_variant(
    target: &rules_star::Target,
    attrs: &[&str],
    space: &Space,
) -> std::result::Result<VariantReading, String> {
    let mut readings = Vec::with_capacity(attrs.len());
    for &name in attrs {
        match conditional::read(target, name, space) {
            Ok(read) => readings.push((name.to_string(), read)),
            Err(_) => return Err(name.to_string()),
        }
    }
    Ok(VariantReading(readings))
}

#[cfg(test)]
mod tests {
    use super::*;
    use conditions::Platform;

    /// A variant attribute is read as a label list is: in a configuration
    /// no branch applies to, a list followed by a select() is the list
    /// alone, and a concatenation holds each label once.
    #[test]
    fn plain_part_when_no_branch_applies() {
        let f = rules_star::parse(
            "rules.star",
            r#"rust_library(
    name = "lib",
    cargo_features = ["std", "alloc"] + select({"config//os:linux": ["alloc", "linux"]}),
    default_features = select({"config//os:linux": False}),
)
"#
            .into(),
        )
        .unwrap();
        let space = Space::new(
            &[
                Platform {
                    os: "linux".into(),
                    cpu: "x86_64".into(),
                },
                Platform {
                    os: "macos".into(),
                    cpu: "arm64".into(),
                },
            ],
            "",
        );
        let variant = read_variant(
            &f.targets[0],
            &["cargo_features", "default_features"],
            &space,
        )
        .unwrap();
        let list = |l: &[&str]| Value::StringList(l.iter().map(|s| s.to_string()).collect());
        assert_eq!(
            variant.get(&space.configurations[0]),
            Variant::from([
                ("cargo_features".into(), list(&["std", "alloc", "linux"])),
                ("default_features".into(), Value::Bool(false)),
            ])
        );
        assert_eq!(
            variant.get(&space.configurations[1]),
            Variant::from([("cargo_features".into(), list(&["std", "alloc"]))])
        );
    }
}
