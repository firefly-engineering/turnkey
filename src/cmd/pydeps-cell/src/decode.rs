//! pydeps-cell.json and python-deps.toml, decoded as the Go version of
//! pydeps-cell (#212) decoded them, with Go's encoding/json and go-toml
//!
//! encoding/json and go-toml match a key to a struct field ignoring case
//! (go-toml compares `strings.ToLower` of the key, encoding/json folds it
//! as `bytes.EqualFold` does), take the keys in document order (the last
//! of two that match the same field wins), and ignore unknown keys. A JSON
//! `null` leaves a string as it is, and empties a list. go-toml decodes a
//! key into what the struct already holds: a table it meets again merges
//! into the same map, and an array replaces the list.

use crate::cell::{Config, DepsFile, Edge, Package};
use conditions::Platform;
use serde::Deserialize;
use serde::de::{DeserializeSeed, Deserializer, IgnoredAny, MapAccess, Visitor};
use std::collections::BTreeMap;
use std::fmt;

/// A key folded as encoding/json folds field names: ASCII letters
/// uppercased, and the two non-ASCII letters that fold to an ASCII one
/// (U+017F `ſ` and U+212A, the Kelvin sign) folded to it. The fields' names
/// are ASCII, so nothing else can match them.
fn json_fold(key: &str) -> String {
    key.chars()
        .map(|c| match c {
            '\u{17f}' => 'S',
            '\u{212a}' => 'K',
            c => c.to_ascii_uppercase(),
        })
        .collect()
}

/// Whether a JSON key names the field `field`
fn json_key(key: &str, field: &str) -> bool {
    json_fold(key) == json_fold(field)
}

/// Whether a TOML key names the field `field` (whose name is lowercase)
fn toml_key(key: &str, field: &str) -> bool {
    key == field || gostd::strings::to_lower(key) == field
}

impl<'de> Deserialize<'de> for Config {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        struct V;
        impl<'de> Visitor<'de> for V {
            type Value = Config;
            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                f.write_str("an object")
            }
            // A JSON null leaves the configuration empty
            fn visit_unit<E>(self) -> Result<Config, E> {
                Ok(Config::default())
            }
            fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Config, A::Error> {
                let mut cfg = Config::default();
                let mut backing = Vec::new();
                while let Some(key) = map.next_key::<String>()? {
                    if json_key(&key, "settings") {
                        if let Some(s) = map.next_value::<Option<String>>()? {
                            cfg.settings = s;
                        }
                    } else if json_key(&key, "platforms") {
                        match map.next_value::<Option<Vec<Option<JsonPlatform>>>>()? {
                            Some(array) => {
                                decode_platforms(array, &mut cfg.platforms, &mut backing)
                            }
                            // A null empties the slice, capacity and all
                            None => {
                                cfg.platforms.clear();
                                backing.clear();
                            }
                        }
                    } else if json_key(&key, "python_version") {
                        if let Some(s) = map.next_value::<Option<String>>()? {
                            cfg.python_version = s;
                        }
                    } else {
                        map.next_value::<IgnoredAny>()?;
                    }
                }
                Ok(cfg)
            }
        }
        d.deserialize_any(V)
    }
}

/// A platform in pydeps-cell.json, as the assignments to make: Go's
/// conditions.Platform has no JSON names, so its fields' (OS, CPU) match
struct JsonPlatform(Vec<(PlatformField, Option<String>)>);

#[derive(Clone, Copy)]
enum PlatformField {
    Os,
    Cpu,
}

impl JsonPlatform {
    /// Assigns the platform's values to `p`; a null leaves a value as it is
    fn apply(self, p: &mut Platform) {
        for (field, value) in self.0 {
            if let Some(value) = value {
                match field {
                    PlatformField::Os => p.os = value,
                    PlatformField::Cpu => p.cpu = value,
                }
            }
        }
    }
}

/// Decodes a platforms array into `platforms`, as encoding/json decodes
/// into a slice: each element into the one already at its index, if any
/// (so a field it doesn't set, or a null element, keeps its value),
/// `platforms` ending as long as the array. `backing` holds the elements
/// past the end the slice's capacity still holds, which a later array
/// reuses too.
fn decode_platforms(
    array: Vec<Option<JsonPlatform>>,
    platforms: &mut Vec<Platform>,
    backing: &mut Vec<Platform>,
) {
    let mut all = std::mem::take(platforms);
    all.append(backing);
    let n = array.len();
    for (i, element) in array.into_iter().enumerate() {
        if i >= all.len() {
            all.push(Platform::default());
        }
        if let Some(element) = element {
            element.apply(&mut all[i]);
        }
    }
    *backing = all.split_off(n);
    *platforms = all;
}

impl<'de> Deserialize<'de> for JsonPlatform {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        struct V;
        impl<'de> Visitor<'de> for V {
            type Value = JsonPlatform;
            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                f.write_str("an object")
            }
            fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<JsonPlatform, A::Error> {
                let mut assignments = Vec::new();
                while let Some(key) = map.next_key::<String>()? {
                    let field = if json_key(&key, "os") {
                        PlatformField::Os
                    } else if json_key(&key, "cpu") {
                        PlatformField::Cpu
                    } else {
                        map.next_value::<IgnoredAny>()?;
                        continue;
                    };
                    assignments.push((field, map.next_value::<Option<String>>()?));
                }
                Ok(JsonPlatform(assignments))
            }
        }
        d.deserialize_map(V)
    }
}

impl<'de> Deserialize<'de> for DepsFile {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        struct V;
        impl<'de> Visitor<'de> for V {
            type Value = DepsFile;
            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                f.write_str("a table")
            }
            fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<DepsFile, A::Error> {
                let mut file = DepsFile::default();
                while let Some(key) = map.next_key::<String>()? {
                    if toml_key(&key, "deps") {
                        map.next_value_seed(Into(&mut file.deps))?;
                    } else {
                        map.next_value::<IgnoredAny>()?;
                    }
                }
                Ok(file)
            }
        }
        d.deserialize_map(V)
    }
}

/// Decodes a value into what is already there, as go-toml does
struct Into<'a, T>(&'a mut T);

/// A table of packages merges into the packages there are
impl<'de> DeserializeSeed<'de> for Into<'_, BTreeMap<String, Package>> {
    type Value = ();
    fn deserialize<D: Deserializer<'de>>(self, d: D) -> Result<(), D::Error> {
        struct V<'a>(&'a mut BTreeMap<String, Package>);
        impl<'de> Visitor<'de> for V<'_> {
            type Value = ();
            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                f.write_str("a table of packages")
            }
            fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<(), A::Error> {
                while let Some(name) = map.next_key::<String>()? {
                    let pkg = self.0.entry(name).or_default();
                    map.next_value_seed(Into(pkg))?;
                }
                Ok(())
            }
        }
        d.deserialize_map(V(self.0))
    }
}

/// A package's table merges into its entry
impl<'de> DeserializeSeed<'de> for Into<'_, Package> {
    type Value = ();
    fn deserialize<D: Deserializer<'de>>(self, d: D) -> Result<(), D::Error> {
        struct V<'a>(&'a mut Package);
        impl<'de> Visitor<'de> for V<'_> {
            type Value = ();
            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                f.write_str("a package table")
            }
            fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<(), A::Error> {
                while let Some(key) = map.next_key::<String>()? {
                    if toml_key(&key, "dependencies") {
                        self.0.dependencies = map.next_value()?;
                    } else if toml_key(&key, "extras") {
                        map.next_value_seed(Into(&mut self.0.extras))?;
                    } else if toml_key(&key, "requested_extras") {
                        self.0.requested_extras = map.next_value()?;
                    } else {
                        map.next_value::<IgnoredAny>()?;
                    }
                }
                Ok(())
            }
        }
        d.deserialize_map(V(self.0))
    }
}

/// A table of extras merges into the extras there are; each extra's list
/// replaces the one there is
impl<'de> DeserializeSeed<'de> for Into<'_, BTreeMap<String, Vec<Edge>>> {
    type Value = ();
    fn deserialize<D: Deserializer<'de>>(self, d: D) -> Result<(), D::Error> {
        struct V<'a>(&'a mut BTreeMap<String, Vec<Edge>>);
        impl<'de> Visitor<'de> for V<'_> {
            type Value = ();
            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                f.write_str("a table of extras")
            }
            fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<(), A::Error> {
                while let Some(extra) = map.next_key::<String>()? {
                    let edges = map.next_value()?;
                    self.0.insert(extra, edges);
                }
                Ok(())
            }
        }
        d.deserialize_map(V(self.0))
    }
}

impl<'de> Deserialize<'de> for Edge {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        struct V;
        impl<'de> Visitor<'de> for V {
            type Value = Edge;
            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                f.write_str("a dependency table")
            }
            fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Edge, A::Error> {
                let mut edge = Edge::default();
                while let Some(key) = map.next_key::<String>()? {
                    if toml_key(&key, "name") {
                        edge.name = map.next_value()?;
                    } else if toml_key(&key, "marker") {
                        edge.marker = map.next_value()?;
                    } else {
                        map.next_value::<IgnoredAny>()?;
                    }
                }
                Ok(edge)
            }
        }
        d.deserialize_map(V)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn json_keys_fold_and_the_last_wins() {
        let cfg: Config = serde_json::from_str(
            r#"{"Settings": "a//b", "ſettings": "c//d", "PLATFORMS": [{"Os": "linux", "CPU": "x86_64", "system": "x"}, null],
                "python_version": null, "unknown": [1, {"x": null}]}"#,
        )
        .unwrap();
        assert_eq!(cfg.settings, "c//d");
        assert_eq!(
            cfg.platforms,
            [
                Platform {
                    os: "linux".into(),
                    cpu: "x86_64".into()
                },
                Platform::default()
            ]
        );
        assert_eq!(cfg.python_version, "");

        let cfg: Config =
            serde_json::from_str(r#"{"platforms": [{"os": "linux"}], "platforms": null}"#).unwrap();
        assert!(cfg.platforms.is_empty());
        // A second array decodes into the first one's elements, even past
        // the end of a shorter one in between
        let cfg: Config = serde_json::from_str(
            r#"{"platforms": [{"os": "linux", "cpu": "x86_64"}, {"os": "macos", "cpu": "arm64"}],
                "platforms": [{"os": "linux"}], "platforms": [{"cpu": "arm64"}, null, {}]}"#,
        )
        .unwrap();
        let platform = |os: &str, cpu: &str| Platform {
            os: os.into(),
            cpu: cpu.into(),
        };
        assert_eq!(
            cfg.platforms,
            [
                platform("linux", "arm64"),
                platform("macos", "arm64"),
                Platform::default()
            ]
        );
        let cfg: Config = serde_json::from_str("null").unwrap();
        assert_eq!(cfg, Config::default());

        for bad in [r#"{"settings": 1}"#, "[]", r#"{"platforms": {}}"#, "{} x"] {
            assert!(serde_json::from_str::<Config>(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn toml_keys_lower_and_tables_merge() {
        let deps: DepsFile = toml::from_str(
            r#"
schema_version = 2

[deps.a]
version = "1"
Dependencies = [{ NAME = "b", Marker = "extra == 'x'", extra = ["y"] }]
requested_extras = ["x"]

[deps.a.extras]
x = [{ name = "c" }]

[Deps.a]
dependencies = [{ name = "d" }]

[Deps.a.EXTRAS]
y = [{ name = "e" }]

[deps.b]
"#,
        )
        .unwrap();
        let a = &deps.deps["a"];
        assert_eq!(
            a.dependencies,
            [Edge {
                name: "d".into(),
                marker: String::new()
            }]
        );
        assert_eq!(a.requested_extras, ["x"]);
        assert_eq!(
            a.extras.keys().map(String::as_str).collect::<Vec<_>>(),
            ["x", "y"]
        );
        assert!(deps.deps.contains_key("b"));

        for bad in [
            "deps = []",
            "[deps.a]\ndependencies = [1]",
            "[deps.a]\ndependencies = [{ name = 1 }]",
            "[deps.a]\nrequested_extras = \"x\"",
        ] {
            assert!(toml::from_str::<DepsFile>(bad).is_err(), "{bad}");
        }
    }
}
