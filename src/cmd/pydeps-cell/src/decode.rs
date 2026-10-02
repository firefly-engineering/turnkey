//! The platforms' conditions (`--platforms`), decoded as the Go version of
//! pydeps-cell (#212) decoded pydeps-cell.json, with Go's encoding/json
//!
//! encoding/json matches a key to a struct field ignoring case (it folds
//! it as `bytes.EqualFold` does), takes the keys in document order (the
//! last of two that match the same field wins), and ignores unknown keys. A
//! JSON `null` leaves a string as it is, and empties a list.

use crate::cell::Config;
use conditions::json::Platforms;
use deps_gen_kit::gojson::key_is as json_key;
use serde::Deserialize;
use serde::de::{Deserializer, IgnoredAny, MapAccess, Visitor};
use std::fmt;

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
                let mut platforms = Platforms::default();
                while let Some(key) = map.next_key::<String>()? {
                    if json_key(&key, "settings") {
                        if let Some(s) = map.next_value::<Option<String>>()? {
                            cfg.settings = s;
                        }
                    } else if json_key(&key, "platforms") {
                        platforms.next_value(&mut map)?;
                    } else if json_key(&key, "python_version") {
                        if let Some(s) = map.next_value::<Option<String>>()? {
                            cfg.python_version = s;
                        }
                    } else {
                        map.next_value::<IgnoredAny>()?;
                    }
                }
                cfg.platforms = platforms.into_platforms();
                Ok(cfg)
            }
        }
        d.deserialize_any(V)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use conditions::Platform;

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
}
