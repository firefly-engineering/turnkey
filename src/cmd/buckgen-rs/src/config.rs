//! buckgen.json, decoded as the Go version of buckgen (#213) decoded it,
//! with Go's encoding/json ([`deps_gen_kit::gojson`]): keys matched to
//! fields ignoring case, in document order (the last wins), unknown keys
//! ignored, a `null` leaving a string or an object's fields as they are, an
//! object met again merging into the fields there are, and an array decoded
//! into the slice there is.

use conditions::Platform;
use conditions::json::Platforms;
use deps_gen_kit::gojson::{Slice, key_is, set_string};
use serde::de::{DeserializeSeed, Deserializer, IgnoredAny, MapAccess, Visitor};
use std::fmt;

/// buckgen's configuration
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Config {
    /// How the rules.star files are written
    pub buck: BuckConfig,
    /// The configurations a package's deps are resolved for: the platforms
    /// turnkey builds for (buck2.platforms) and the allowed Go build tags
    /// (buck2.go.allowedBuildTags)
    pub conditions: ConditionsConfig,
    /// The Go toolchain's version (e.g. "1.24"): files constrained to a
    /// later release are left out
    pub go_version: String,
}

/// How the rules.star files are written
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BuckConfig {
    /// Written first, followed by a blank line, unless empty (the spelling
    /// is the configuration's)
    pub preambule: String,
    /// The rule of a Go package's target
    pub go_library_rule: String,
    /// Prefixes a dep's import path in its label
    pub deps_target_label_prefix: String,
    /// The attribute the deps are written to
    pub deps_attr: String,
    /// The name of the files written
    pub buildfile_name: String,
}

/// The configurations, named as turnkey's Nix names them
/// (nix/buck2/platforms.nix)
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ConditionsConfig {
    /// The package of the combined config_settings
    pub settings: String,
    /// The platforms, in Buck2's names
    pub platforms: Vec<Platform>,
    /// The build tags that may vary per configuration
    pub go_tags: Vec<String>,
}

/// Reads buckgen.json
pub fn load(data: &[u8]) -> Result<Config, serde_json::Error> {
    let mut dec = ConfigDecoder::default();
    let text = deps_gen_kit::gojson::text(data);
    let mut de = serde_json::Deserializer::from_str(&text);
    Into(&mut dec).deserialize(&mut de)?;
    de.end()?;
    Ok(Config {
        buck: dec.buck,
        conditions: ConditionsConfig {
            settings: dec.conditions.settings,
            platforms: dec.conditions.platforms.into_platforms(),
            go_tags: dec.conditions.go_tags.items,
        },
        go_version: dec.go_version,
    })
}

/// The configuration being decoded, with what its slices' capacity holds
#[derive(Default)]
struct ConfigDecoder {
    buck: BuckConfig,
    conditions: ConditionsDecoder,
    go_version: String,
}

/// Decodes a JSON value into what is already there, as encoding/json does:
/// `null` changes nothing, an object sets the fields it names
struct Into<'a, T>(&'a mut T);

/// Implements an object's decoding into `$ty`, each key matched (with
/// [`key_is`]) by the `$decode` closure, which reports whether it took the
/// value
macro_rules! decode_object {
    ($ty:ty, $what:literal, |$target:ident, $key:ident, $map:ident| $body:block) => {
        impl<'de> DeserializeSeed<'de> for Into<'_, $ty> {
            type Value = ();
            fn deserialize<D: Deserializer<'de>>(self, d: D) -> Result<(), D::Error> {
                struct V<'a>(&'a mut $ty);
                impl<'de> Visitor<'de> for V<'_> {
                    type Value = ();
                    fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                        f.write_str($what)
                    }
                    fn visit_unit<E>(self) -> Result<(), E> {
                        Ok(())
                    }
                    fn visit_map<A: MapAccess<'de>>(self, mut $map: A) -> Result<(), A::Error> {
                        let $target = self.0;
                        while let Some($key) = $map.next_key::<String>()? {
                            let $key = $key.as_str();
                            let taken: bool = $body;
                            if !taken {
                                $map.next_value::<IgnoredAny>()?;
                            }
                        }
                        Ok(())
                    }
                }
                d.deserialize_any(V(self.0))
            }
        }
    };
}

/// The map's next value, a string or `null`, into `slot`
fn string<'de, A: MapAccess<'de>>(map: &mut A, slot: &mut String) -> Result<bool, A::Error> {
    set_string(map.next_value()?, slot);
    Ok(true)
}

decode_object!(ConfigDecoder, "a buckgen configuration", |cfg, key, map| {
    if key_is(key, "buck") {
        map.next_value_seed(Into(&mut cfg.buck))?;
        true
    } else if key_is(key, "conditions") {
        map.next_value_seed(Into(&mut cfg.conditions))?;
        true
    } else if key_is(key, "go_version") {
        string(&mut map, &mut cfg.go_version)?
    } else {
        false
    }
});

decode_object!(BuckConfig, "the buck settings", |buck, key, map| {
    if key_is(key, "preambule") {
        string(&mut map, &mut buck.preambule)?
    } else if key_is(key, "go_library_rule") {
        string(&mut map, &mut buck.go_library_rule)?
    } else if key_is(key, "deps_target_label_prefix") {
        string(&mut map, &mut buck.deps_target_label_prefix)?
    } else if key_is(key, "deps_attr") {
        string(&mut map, &mut buck.deps_attr)?
    } else if key_is(key, "buildfile_name") {
        string(&mut map, &mut buck.buildfile_name)?
    } else {
        false
    }
});

/// The conditions, with the slices' capacity kept between their arrays
#[derive(Default)]
struct ConditionsDecoder {
    settings: String,
    platforms: Platforms,
    go_tags: Slice<String>,
}

decode_object!(ConditionsDecoder, "the conditions", |c, key, map| {
    if key_is(key, "settings") {
        string(&mut map, &mut c.settings)?
    } else if key_is(key, "platforms") {
        c.platforms.next_value(&mut map)?;
        true
    } else if key_is(key, "go_tags") {
        c.go_tags
            .decode(map.next_value::<Option<Vec<Option<String>>>>()?, set_string);
        true
    } else {
        false
    }
});

#[cfg(test)]
mod tests {
    use super::*;

    fn platform(os: &str, cpu: &str) -> Platform {
        Platform {
            os: os.into(),
            cpu: cpu.into(),
        }
    }

    /// The configuration nix/lib/deps-cell/adapters/go.nix writes
    #[test]
    fn loads_what_nix_writes() {
        let cfg = load(br##"{"buck":{"buildfile_name":"rules.star","deps_attr":"deps","deps_target_label_prefix":"godeps//vendor/","go_library_rule":"go_library","preambule":"# Auto-generated by turnkey buckgen\n"},"conditions":{"go_tags":["integration"],"platforms":[{"cpu":"x86_64","os":"linux"},{"cpu":"arm64","os":"macos"}],"settings":"toolchains//conditions"},"go_version":"1.26"}"##).unwrap();
        assert_eq!(
            cfg,
            Config {
                buck: BuckConfig {
                    preambule: "# Auto-generated by turnkey buckgen\n".into(),
                    go_library_rule: "go_library".into(),
                    deps_target_label_prefix: "godeps//vendor/".into(),
                    deps_attr: "deps".into(),
                    buildfile_name: "rules.star".into(),
                },
                conditions: ConditionsConfig {
                    settings: "toolchains//conditions".into(),
                    platforms: vec![platform("linux", "x86_64"), platform("macos", "arm64")],
                    go_tags: vec!["integration".into()],
                },
                go_version: "1.26".into(),
            }
        );
    }

    /// Keys fold, the last wins, null leaves values, objects merge, and
    /// arrays decode into the slice there is
    #[test]
    fn decodes_as_encoding_json() {
        let cfg = load(
            br#"{"BUCK": {"Deps_Attr": "a", "go_library_rule": "r"}, "buck": {"deps_attr": "b", "preambule": null},
                "buck": null, "unknown": {"x": [1]}, "go_version": "1.20", "GO_VERSION": null,
                "conditions": {"go_tags": ["x", "y"], "settings": "s"},
                "Conditions": {"go_tags": [null], "platforms": [{"os": "linux"}]},
                "conditions": {"go_tags": [null, null], "platforms": [{"cpu": "arm64"}]}}"#,
        )
        .unwrap();
        assert_eq!(cfg.buck.deps_attr, "b");
        assert_eq!(cfg.buck.go_library_rule, "r");
        assert_eq!(cfg.go_version, "1.20");
        assert_eq!(cfg.conditions.settings, "s");
        assert_eq!(cfg.conditions.go_tags, ["x", "y"]);
        assert_eq!(cfg.conditions.platforms, [platform("linux", "arm64")]);
        assert_eq!(load(b"null").unwrap(), Config::default());
        assert_eq!(load(b" {} ").unwrap(), Config::default());
        for bad in [
            &b"[]"[..],
            b"\"x\"",
            b"{\"go_version\": 1}",
            b"{\"buck\": []}",
            b"{\"conditions\": {\"go_tags\": \"x\"}}",
            b"{} {}",
            b"",
        ] {
            assert!(load(bad).is_err(), "{}", String::from_utf8_lossy(bad));
        }
    }
}
