//! A request for a crate's features expanded the way Cargo does: what a
//! set of requested features turns on in the crate
//!
//! The Rust port of src/go/pkg/cargofeatures, which mirrors
//! turnkey.cargo.features.activate; testdata/activation-vectors.json holds
//! the test cases each runs.
//!
//! Reference: https://doc.rust-lang.org/cargo/reference/features.html

use std::collections::{BTreeMap, BTreeSet, HashSet};

/// What feature activation reads from a crate's manifest
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Crate {
    /// The `[features]` table: each feature's items
    pub features: BTreeMap<String, Vec<String>>,
    /// The manifest keys of its optional dependencies, in any dependency
    /// table
    pub optional: HashSet<String>,
    /// The manifest keys of its required dependencies: a key optional in
    /// one table and required in another is in both
    pub required: HashSet<String>,
}

/// What a set of requested features turns on in a crate
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Activation {
    /// The enabled features, transitively expanded, sorted: "default" is
    /// expanded but not reported, and dep: or forwarding items aren't
    /// features
    pub features: Vec<String>,
    /// The optional dependencies (by manifest key) the features activate,
    /// sorted
    pub optional_deps: Vec<String>,
    /// The features to request on dependencies (by manifest key), sorted:
    /// from "dep/feature" items, and from "dep?/feature" items whose
    /// dependency is active
    pub dep_features: BTreeMap<String, Vec<String>>,
}

/// Expands `requested` features of `krate` as Cargo does. Features in
/// `remove` are treated as absent, so what only they would turn on stays
/// off; removing "default" drops the crate's default set.
pub fn activate(krate: &Crate, requested: &[String], remove: &[String]) -> Activation {
    let removed: HashSet<&str> = remove.iter().map(String::as_str).collect();
    // An optional dependency named with "dep:" anywhere has no implicit
    // feature of its own name.
    let named_with_dep: HashSet<&str> = krate
        .features
        .values()
        .flatten()
        .filter_map(|item| item.strip_prefix("dep:"))
        .collect();

    struct Forward {
        dep: String,
        feature: String,
        weak: bool,
    }
    let mut enabled = BTreeSet::new();
    let mut optional_deps = BTreeSet::new();
    let mut forwards = Vec::new();
    let mut to_process: Vec<String> = requested.to_vec();
    while let Some(feature) = to_process.pop() {
        if removed.contains(feature.as_str()) || enabled.contains(&feature) {
            continue;
        }
        if let Some(dep) = feature.strip_prefix("dep:") {
            optional_deps.insert(dep.to_string());
            continue;
        }
        if let Some((dep_part, dep_feature)) = feature.split_once('/') {
            let (dep, weak) = match dep_part.strip_suffix('?') {
                Some(dep) => (dep, true),
                None => (dep_part, false),
            };
            forwards.push(Forward {
                dep: dep.to_string(),
                feature: dep_feature.to_string(),
                weak,
            });
            if !weak && krate.optional.contains(dep) {
                optional_deps.insert(dep.to_string());
                // ...and turns on the feature of the dependency's name, if
                // there is one (explicit, or implicit)
                if krate.features.contains_key(dep) || !named_with_dep.contains(dep) {
                    to_process.push(dep.to_string());
                }
            }
            continue;
        }
        enabled.insert(feature.clone());
        match krate.features.get(&feature) {
            Some(items) => to_process.extend(items.iter().cloned()),
            None if krate.optional.contains(&feature)
                && !named_with_dep.contains(feature.as_str()) =>
            {
                optional_deps.insert(feature.clone());
            }
            None => {}
        }
    }

    let mut dep_features: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for f in forwards {
        if f.weak && !krate.required.contains(&f.dep) && !optional_deps.contains(&f.dep) {
            continue;
        }
        let features = dep_features.entry(f.dep).or_default();
        if !features.contains(&f.feature) {
            features.push(f.feature);
        }
    }
    for features in dep_features.values_mut() {
        features.sort();
    }
    enabled.remove("default");
    Activation {
        features: enabled.into_iter().collect(),
        optional_deps: optional_deps.into_iter().collect(),
        dep_features,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Deserialize;

    #[derive(Deserialize)]
    struct Doc {
        cases: Vec<Case>,
    }

    #[derive(Deserialize)]
    struct Case {
        name: String,
        #[serde(default)]
        features: BTreeMap<String, Vec<String>>,
        #[serde(default)]
        optional: Vec<String>,
        #[serde(default)]
        required: Vec<String>,
        #[serde(default)]
        requested: Vec<String>,
        #[serde(default)]
        remove: Vec<String>,
        want: Want,
    }

    #[derive(Deserialize)]
    struct Want {
        #[serde(default)]
        features: Option<Vec<String>>,
        #[serde(default)]
        optional_deps: Option<Vec<String>>,
        #[serde(default)]
        dep_features: BTreeMap<String, Vec<String>>,
    }

    /// The cases src/go/pkg/cargofeatures runs too. testdata/ links to the
    /// file, which Buck2 maps there.
    #[test]
    fn shared_vectors() {
        let doc: Doc =
            serde_json::from_str(include_str!("../../testdata/activation-vectors.json")).unwrap();
        assert!(!doc.cases.is_empty());
        for c in doc.cases {
            let krate = Crate {
                features: c.features,
                optional: c.optional.into_iter().collect(),
                required: c.required.into_iter().collect(),
            };
            let got = activate(&krate, &c.requested, &c.remove);
            assert_eq!(
                got.features,
                c.want.features.unwrap_or_default(),
                "{}: features",
                c.name
            );
            assert_eq!(
                got.optional_deps,
                c.want.optional_deps.unwrap_or_default(),
                "{}: optional deps",
                c.name
            );
            assert_eq!(
                got.dep_features, c.want.dep_features,
                "{}: dep features",
                c.name
            );
        }
    }
}
