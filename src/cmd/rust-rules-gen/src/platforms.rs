//! The platforms turnkey builds for, and the select() keys that tell them
//! apart.
//!
//! The same rules as src/go/pkg/conditions and turnkey.cfg (src/python/cfg):
//! a platform is an (os, cpu) pair in Buck2's constraint names, and values
//! that differ between platforms are keyed on the smallest exact key:
//! `config//os:<os>` when they differ only by OS (`config//cpu:<cpu>` by CPU
//! alone), otherwise the combined config_setting `<settings>:<os>-<cpu>`.
//! Every platform gets a branch, and there is never a DEFAULT. The tests run
//! the conditions module's shared test cases (split-vectors.json).

use anyhow::{Result, bail};
use serde::Deserialize;
use std::collections::BTreeMap;

/// A platform, named by the values of its Buck2 os and cpu constraints
#[derive(Debug, Clone, PartialEq, Eq, Hash, Deserialize)]
pub struct Platform {
    pub os: String,
    pub cpu: String,
}

impl Platform {
    /// "<os>-<cpu>", as rust-deps.toml and the combined config_settings
    /// name it
    pub fn name(&self) -> String {
        format!("{}-{}", self.os, self.cpu)
    }
}

/// The dimensions a key can match, in the order keys are tried
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Dims {
    Os,
    Cpu,
    OsCpu,
}

const DIMS: [Dims; 3] = [Dims::Os, Dims::Cpu, Dims::OsCpu];

/// The platforms, and the package of their combined config_settings, as
/// turnkey's platforms record writes them (nix/buck2/platforms.nix's
/// conditions)
#[derive(Debug, Clone, Deserialize)]
pub struct Platforms {
    pub settings: String,
    pub platforms: Vec<Platform>,
}

impl Platforms {
    pub fn from_json(text: &str) -> Result<Platforms> {
        let mut platforms: Platforms = serde_json::from_str(text)?;
        let mut seen = Vec::new();
        platforms.platforms.retain(|p| {
            let new = !seen.contains(p);
            seen.push(p.clone());
            new
        });
        if platforms.platforms.is_empty() {
            bail!("no platforms");
        }
        Ok(platforms)
    }

    /// The combined config_setting of a platform
    pub fn combined_key(&self, platform: &Platform) -> String {
        self.key(Dims::OsCpu, platform)
    }

    fn key(&self, dims: Dims, platform: &Platform) -> String {
        match dims {
            Dims::Os => format!("config//os:{}", platform.os),
            Dims::Cpu => format!("config//cpu:{}", platform.cpu),
            Dims::OsCpu => format!("{}:{}-{}", self.settings, platform.os, platform.cpu),
        }
    }

    /// Key each platform's value on the smallest exact key, or None when
    /// every platform has the same value
    pub fn branches<T: Clone + PartialEq>(
        &self,
        values: impl Fn(&Platform) -> T,
    ) -> Option<BTreeMap<String, T>> {
        self.choose(values).map(|(_, keyed)| keyed)
    }

    /// Split each platform's items into those every platform has (in the
    /// first platform's order) and, when platforms differ, the rest per
    /// key (each in the order of the first platform the key matches)
    pub fn split<T: Clone + PartialEq + Ord>(
        &self,
        items: impl Fn(&Platform) -> Vec<T>,
    ) -> (Vec<T>, BTreeMap<String, Vec<T>>) {
        let per: Vec<Vec<T>> = self
            .platforms
            .iter()
            .map(|p| {
                let mut unique: Vec<T> = Vec::new();
                for item in items(p) {
                    if !unique.contains(&item) {
                        unique.push(item);
                    }
                }
                unique
            })
            .collect();
        let common: Vec<T> = per[0]
            .iter()
            .filter(|i| per.iter().all(|p| p.contains(i)))
            .cloned()
            .collect();
        let extra: Vec<Vec<T>> = per
            .iter()
            .map(|p| p.iter().filter(|i| !common.contains(i)).cloned().collect())
            .collect();
        let index = |p: &Platform| self.platforms.iter().position(|q| q == p).unwrap();
        let as_set = |items: &Vec<T>| {
            let mut sorted = items.clone();
            sorted.sort();
            sorted
        };
        let Some((dims, _)) = self.choose(|p| as_set(&extra[index(p)])) else {
            return (common, BTreeMap::new());
        };
        let mut result = BTreeMap::new();
        for (i, p) in self.platforms.iter().enumerate() {
            result
                .entry(self.key(dims, p))
                .or_insert_with(|| extra[i].clone());
        }
        (common, result)
    }

    /// The fewest dimensions whose values determine each platform's value
    fn choose<T: Clone + PartialEq>(
        &self,
        values: impl Fn(&Platform) -> T,
    ) -> Option<(Dims, BTreeMap<String, T>)> {
        let all: Vec<T> = self.platforms.iter().map(&values).collect();
        if all.iter().all(|v| *v == all[0]) {
            return None;
        }
        for dims in DIMS {
            let mut keyed: BTreeMap<String, T> = BTreeMap::new();
            let consistent = self
                .platforms
                .iter()
                .zip(&all)
                .all(|(p, v)| keyed.entry(self.key(dims, p)).or_insert_with(|| v.clone()) == v);
            if consistent {
                return Some((dims, keyed));
            }
        }
        unreachable!("os and cpu together tell every platform apart")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

    /// The shared test cases of src/go/pkg/conditions. testdata/ links to
    /// the file, and Buck2 maps it to the same path.
    fn vectors() -> Value {
        serde_json::from_str(include_str!("../testdata/split-vectors.json")).unwrap()
    }

    #[test]
    fn splits_as_the_conditions_module_does() {
        let cases = vectors()["cases"].as_array().unwrap().clone();
        let platform_only: Vec<&Value> = cases
            .iter()
            .filter(|c| c["dimensions"].as_array().unwrap().is_empty())
            .collect();
        assert!(!platform_only.is_empty());
        for case in platform_only {
            let platforms = Platforms::from_json(
                &serde_json::json!({"settings": case["settings"], "platforms": case["platforms"]})
                    .to_string(),
            )
            .unwrap();
            let labels = |p: &Platform| -> Vec<String> {
                case["labels"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .filter(|rule| {
                        rule["when"]
                            .as_object()
                            .unwrap()
                            .iter()
                            .all(|(dim, value)| {
                                let actual = if dim == "os" { &p.os } else { &p.cpu };
                                value.as_str() == Some(actual.as_str())
                            })
                    })
                    .flat_map(|rule| {
                        rule["labels"]
                            .as_array()
                            .unwrap()
                            .iter()
                            .map(|l| l.as_str().unwrap().to_string())
                    })
                    .collect()
            };
            let (common, branches) = platforms.split(labels);
            let expected_common: Vec<String> =
                serde_json::from_value(case["common"].clone()).unwrap();
            let expected_branches: Vec<(String, Vec<String>)> = case["branches"]
                .as_array()
                .unwrap()
                .iter()
                .map(|b| {
                    (
                        b["key"].as_str().unwrap().to_string(),
                        serde_json::from_value(b["labels"].clone()).unwrap(),
                    )
                })
                .collect();
            let name = case["name"].as_str().unwrap();
            assert_eq!(common, expected_common, "{name}");
            assert_eq!(
                branches.into_iter().collect::<Vec<_>>(),
                expected_branches,
                "{name}"
            );
        }
    }
}
