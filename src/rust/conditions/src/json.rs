//! A list of platforms in a JSON configuration, decoded as Go's
//! `encoding/json` decoded it into a `[]conditions.Platform` field
//!
//! Go's `conditions.Platform` has no JSON names, so a key matches its
//! fields (`OS`, `CPU`) ignoring case ([`deps_gen_kit::gojson`]).
//! `encoding/json` decodes an array into the slice that is there
//! ([`deps_gen_kit::gojson::Slice`]): each element into the one already at
//! its index, if any, so a field it doesn't set, or a `null` element, keeps
//! its value.

use crate::Platform;
use deps_gen_kit::gojson::{Slice, key_is};
use serde::de::{Deserialize, Deserializer, IgnoredAny, MapAccess, Visitor};
use std::fmt;

/// The platforms decoded so far into a field
#[derive(Debug, Default)]
pub struct Platforms {
    slice: Slice<Platform>,
}

impl Platforms {
    /// Decodes the map's next value, an array of platforms or `null`, into
    /// the field
    pub fn next_value<'de, A: MapAccess<'de>>(&mut self, map: &mut A) -> Result<(), A::Error> {
        let array = map.next_value::<Option<Vec<Option<JsonPlatform>>>>()?;
        self.slice.decode(array, |element, p| {
            if let Some(element) = element {
                element.apply(p);
            }
        });
        Ok(())
    }

    /// The field's value
    pub fn platforms(&self) -> &[Platform] {
        &self.slice.items
    }

    /// The field's value, taken
    pub fn into_platforms(self) -> Vec<Platform> {
        self.slice.items
    }
}

/// A platform in the JSON, as the assignments to make
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
                    let field = if key_is(&key, "os") {
                        PlatformField::Os
                    } else if key_is(&key, "cpu") {
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

#[cfg(test)]
mod tests {
    use super::*;

    /// `{"platforms": ...}` objects, each decoded in turn into one field
    struct Field(Vec<Platform>);

    impl<'de> Deserialize<'de> for Field {
        fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
            struct V;
            impl<'de> Visitor<'de> for V {
                type Value = Field;
                fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                    f.write_str("an object")
                }
                fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Field, A::Error> {
                    let mut platforms = Platforms::default();
                    while let Some(key) = map.next_key::<String>()? {
                        if key_is(&key, "platforms") {
                            platforms.next_value(&mut map)?;
                        } else {
                            map.next_value::<IgnoredAny>()?;
                        }
                    }
                    Ok(Field(platforms.into_platforms()))
                }
            }
            d.deserialize_map(V)
        }
    }

    fn decode(json: &str) -> Vec<Platform> {
        serde_json::from_str::<Field>(json).unwrap().0
    }

    fn platform(os: &str, cpu: &str) -> Platform {
        Platform {
            os: os.into(),
            cpu: cpu.into(),
        }
    }

    #[test]
    fn keys_fold_and_unknown_keys_are_ignored() {
        assert_eq!(
            decode(r#"{"PLATFORMS": [{"Os": "linux", "CPU": "x86_64", "system": "x"}, null]}"#),
            [platform("linux", "x86_64"), Platform::default()]
        );
    }

    #[test]
    fn a_later_array_decodes_into_the_elements_there_are() {
        assert!(decode(r#"{"platforms": [{"os": "linux"}], "platforms": null}"#).is_empty());
        // A second array decodes into the first one's elements, even past
        // the end of a shorter one in between
        assert_eq!(
            decode(
                r#"{"platforms": [{"os": "linux", "cpu": "x86_64"}, {"os": "macos", "cpu": "arm64"}],
                    "platforms": [{"os": "linux"}], "platforms": [{"cpu": "arm64"}, null, {}]}"#
            ),
            [
                platform("linux", "arm64"),
                platform("macos", "arm64"),
                Platform::default()
            ]
        );
        // An empty array drops them
        assert_eq!(
            decode(r#"{"platforms": [{"os": "linux"}], "platforms": [], "platforms": [null]}"#),
            [Platform::default()]
        );
        assert!(serde_json::from_str::<Field>(r#"{"platforms": {}}"#).is_err());
        assert!(serde_json::from_str::<Field>(r#"{"platforms": [{"os": 1}]}"#).is_err());
    }
}
