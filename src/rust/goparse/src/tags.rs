//! Go builds as configurations of the conditions crate: a build tag's
//! on/off dimension, and the Go build of a configuration

use crate::BuildContext;
use conditions::{CPU, Configuration, OS, OnOff, SET};

/// Starts the name of a Go build tag's configuration dimension, e.g.
/// `go_tag:integration`
const TAG_DIMENSION_PREFIX: &str = "go_tag:";

/// The configuration dimension of a Go build tag: on when the tag is set,
/// backed by the prelude's `prelude//go/tags/constraints:<tag>` constraint
pub fn tag_dimension(tag: &str) -> OnOff {
    OnOff {
        name: format!("{TAG_DIMENSION_PREFIX}{tag}"),
        constraint: format!("prelude//go/tags/constraints:{tag}"),
        token: tag.to_string(),
    }
}

/// The build tags a configuration sets, sorted: those whose dimension
/// ([`tag_dimension`]) it sets on
pub fn config_tags(config: &Configuration) -> Vec<String> {
    let mut tags: Vec<String> = config
        .iter()
        .filter_map(|(dim, value)| {
            dim.strip_prefix(TAG_DIMENSION_PREFIX)
                .filter(|_| value == SET)
                .map(str::to_string)
        })
        .collect();
    tags.sort();
    tags
}

/// Go's name for a Buck2 os constraint value
fn go_os(os: &str) -> Option<&'static str> {
    match os {
        "linux" => Some("linux"),
        "macos" => Some("darwin"),
        _ => None,
    }
}

/// Go's name for a Buck2 cpu constraint value
fn go_arch(cpu: &str) -> Option<&'static str> {
    match cpu {
        "x86_64" => Some("amd64"),
        "arm64" => Some("arm64"),
        _ => None,
    }
}

/// The Go build of a configuration: its platform's GOOS and GOARCH, cgo,
/// and the tags it sets ([`config_tags`]). The Go version is left to the
/// caller, whose toolchain it is. The flag is false, with GOOS and GOARCH
/// empty, for a configuration without a platform Go has names for.
pub fn config_context(config: &Configuration) -> (BuildContext, bool) {
    let mut ctx = BuildContext {
        cgo_enabled: true,
        tags: config_tags(config),
        ..BuildContext::default()
    };
    match (go_os(config.get(OS)), go_arch(config.get(CPU))) {
        (Some(goos), Some(goarch)) => {
            ctx.goos = goos.to_string();
            ctx.goarch = goarch.to_string();
            (ctx, true)
        }
        _ => (ctx, false),
    }
}

impl BuildContext {
    /// The environment the go command needs to build for this context's
    /// platform, whatever the host: GOOS, GOARCH and cgo. Empty for a
    /// context without a platform, which the go command takes as the
    /// host's.
    pub fn environ(&self) -> Vec<String> {
        if self.goos.is_empty() || self.goarch.is_empty() {
            return Vec::new();
        }
        let cgo = if self.cgo_enabled { "1" } else { "0" };
        vec![
            format!("GOOS={}", self.goos),
            format!("GOARCH={}", self.goarch),
            format!("CGO_ENABLED={cgo}"),
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use conditions::{Branch, Space, UNSET};

    /// A configuration sets the tags whose dimension it sets on, and no
    /// other
    #[test]
    fn config_tags_are_the_dimensions_set_on() {
        let config = Configuration::new([
            (OS, "linux"),
            (tag_dimension("b").name.as_str(), SET),
            (tag_dimension("a").name.as_str(), SET),
            (tag_dimension("off").name.as_str(), UNSET),
        ]);
        assert_eq!(config_tags(&config), ["a", "b"]);
    }

    /// A tag's dimension is keyed on the prelude's constraint for it
    #[test]
    fn tag_dimension_keys() {
        let space = Space::new(&[], "").with_dimensions(&[tag_dimension("integration")]);
        let split = space.split(config_tags);
        assert_eq!(
            split.branches,
            [
                Branch {
                    key: "prelude//go/tags/constraints:integration[set]".into(),
                    labels: vec!["integration".into()],
                },
                Branch {
                    key: "prelude//go/tags/constraints:integration[unset]".into(),
                    labels: vec![],
                },
            ]
        );
    }

    /// A configuration's platform, in Buck2's names, is its Go build's GOOS
    /// and GOARCH, with cgo on and the tags it sets
    #[test]
    fn config_context_of_a_platform() {
        for (os, cpu, goos, goarch) in [
            ("linux", "x86_64", "linux", "amd64"),
            ("linux", "arm64", "linux", "arm64"),
            ("macos", "x86_64", "darwin", "amd64"),
            ("macos", "arm64", "darwin", "arm64"),
        ] {
            let integration = tag_dimension("integration");
            let config =
                Configuration::new([(OS, os), (CPU, cpu), (integration.name.as_str(), SET)]);
            let (ctx, ok) = config_context(&config);
            let want = BuildContext {
                goos: goos.into(),
                goarch: goarch.into(),
                cgo_enabled: true,
                tags: vec!["integration".into()],
                ..BuildContext::default()
            };
            assert!(ok, "{config}");
            assert_eq!(ctx, want, "{config}");
            assert_eq!(
                ctx.environ(),
                [
                    format!("GOOS={goos}"),
                    format!("GOARCH={goarch}"),
                    "CGO_ENABLED=1".to_string()
                ]
            );
        }
    }

    /// Without a platform Go has names for, a configuration's Go build has
    /// no GOOS or GOARCH, and the go command gets the host's
    #[test]
    fn config_context_without_platform() {
        for config in [
            Configuration::default(),
            Configuration::new([(OS, "windows"), (CPU, "x86_64")]),
            Configuration::new([(OS, "linux"), (CPU, "riscv64")]),
        ] {
            let (ctx, ok) = config_context(&config);
            assert!(
                !ok && ctx.goos.is_empty() && ctx.goarch.is_empty(),
                "{config}"
            );
            assert!(ctx.environ().is_empty(), "{config}");
        }
    }
}
