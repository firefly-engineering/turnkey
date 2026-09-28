//! A crate's fixup, as nix/lib/fixups/resolve.nix writes it for one locked
//! version (its `gen` part): what stands in for the build script in the
//! crate's rules.star.
//!
//! Rustc flags and env are layered: what every platform gets (common), and
//! what a platform's OS, CPU, or OS and CPU pair (`<os>-<cpu>`) adds.
//! Native libraries exist only for the platform the crate was built on.

use crate::platforms::{Platform, Platforms};
use serde::Deserialize;
use std::collections::BTreeMap;

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Fixup {
    #[serde(default)]
    pub out_dir: bool,
    #[serde(default)]
    pub rustc_flags: Layers<Vec<String>>,
    #[serde(default)]
    pub env: Layers<BTreeMap<String, String>>,
    #[serde(default)]
    pub native_libraries: Vec<NativeLibrary>,
}

/// Values every platform gets, and those an OS, a CPU or a pair adds
#[derive(Debug, Default, Deserialize)]
pub struct Layers<T> {
    #[serde(default)]
    pub common: T,
    #[serde(default)]
    pub os: BTreeMap<String, T>,
    #[serde(default)]
    pub cpu: BTreeMap<String, T>,
    #[serde(default)]
    pub platform: BTreeMap<String, T>,
}

impl<T> Layers<T> {
    /// What the overlays give a platform, most general first: its OS's, its
    /// CPU's, then its OS and CPU pair's
    fn overlays_on(&self, p: &Platform) -> impl Iterator<Item = &T> {
        [
            self.os.get(&p.os),
            self.cpu.get(&p.cpu),
            self.platform.get(&p.name()),
        ]
        .into_iter()
        .flatten()
    }
}

/// A library a fixup built natively, which the crate links
#[derive(Debug, Clone, Deserialize)]
pub struct NativeLibrary {
    pub lib_name: String,
    pub static_lib_path: String,
    #[serde(default = "out_dir")]
    pub link_search_path: String,
}

fn out_dir() -> String {
    "out_dir".to_string()
}

/// Values every platform has, and select() branches when platforms differ
pub struct Split<T> {
    pub common: T,
    pub by_platform: BTreeMap<String, T>,
}

impl Fixup {
    /// The rustc flags, as common flags and select() branches. Flags come
    /// in pairs (--cfg foo), so each platform's list is kept whole and in
    /// order.
    pub fn rustc_flags(&self, platforms: &Platforms) -> Split<Vec<String>> {
        let layers = &self.rustc_flags;
        let on =
            |p: &Platform| -> Vec<String> { layers.overlays_on(p).flatten().cloned().collect() };
        let mut common = layers.common.clone();
        match platforms.branches(on) {
            None => {
                common.extend(on(&platforms.platforms[0]));
                Split {
                    common,
                    by_platform: BTreeMap::new(),
                }
            }
            Some(by_platform) => Split {
                common,
                by_platform,
            },
        }
    }

    /// The env, as common entries and select() branches. Overlays never
    /// disagree on a key: resolving the fixup fails first.
    pub fn env(&self, platforms: &Platforms) -> Split<BTreeMap<String, String>> {
        let layers = &self.env;
        let on = |p: &Platform| -> BTreeMap<String, String> {
            layers
                .overlays_on(p)
                .flat_map(|m| m.iter().map(|(k, v)| (k.clone(), v.clone())))
                .collect()
        };
        let mut common = layers.common.clone();
        match platforms.branches(on) {
            None => {
                common.extend(on(&platforms.platforms[0]));
                Split {
                    common,
                    by_platform: BTreeMap::new(),
                }
            }
            Some(by_platform) => Split {
                common,
                by_platform,
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn platforms() -> Platforms {
        Platforms::from_json(
            r#"{"settings": "toolchains//conditions", "platforms": [
                {"os": "linux", "cpu": "x86_64"}, {"os": "linux", "cpu": "arm64"},
                {"os": "macos", "cpu": "x86_64"}, {"os": "macos", "cpu": "arm64"}]}"#,
        )
        .unwrap()
    }

    #[test]
    fn layers_flags_per_platform_in_order() {
        let fixup: Fixup = serde_json::from_str(
            r#"{"rustcFlags": {"common": ["--cfg", "a"], "os": {"linux": ["--cfg", "l"]},
                "platform": {"linux-arm64": ["--cfg", "la"]}}}"#,
        )
        .unwrap();
        let flags = fixup.rustc_flags(&platforms());
        assert_eq!(flags.common, ["--cfg", "a"]);
        assert_eq!(
            flags.by_platform.into_iter().collect::<Vec<_>>(),
            vec![
                (
                    "toolchains//conditions:linux-arm64".to_string(),
                    vec!["--cfg".into(), "l".into(), "--cfg".into(), "la".into()]
                ),
                (
                    "toolchains//conditions:linux-x86_64".to_string(),
                    vec!["--cfg".into(), "l".into()]
                ),
                ("toolchains//conditions:macos-arm64".to_string(), vec![]),
                ("toolchains//conditions:macos-x86_64".to_string(), vec![]),
            ]
        );
    }

    #[test]
    fn uniform_overlays_are_common() {
        let fixup: Fixup = serde_json::from_str(
            r#"{"outDir": true, "env": {"common": {"A": "1"}, "os": {"linux": {"B": "2"}, "macos": {"B": "2"}}},
                "nativeLibraries": [{"lib_name": "ring_core", "static_lib_path": "out_dir/libring.a"}]}"#,
        )
        .unwrap();
        let env = fixup.env(&platforms());
        assert_eq!(
            env.common,
            BTreeMap::from([("A".into(), "1".into()), ("B".into(), "2".into())])
        );
        assert!(env.by_platform.is_empty());
        assert!(fixup.out_dir);
        assert_eq!(fixup.native_libraries[0].link_search_path, "out_dir");
    }
}
