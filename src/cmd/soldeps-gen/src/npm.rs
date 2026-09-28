//! Whether an npm package holds Solidity
//!
//! A package.json dependency counts as a Solidity dependency iff its tarball
//! contains a `.sol` file. [`TarballInspector`] is the seam that looks: the
//! real adapter, [`RegistryInspector`], downloads the tarball, and
//! [`MemoryInspector`] answers from a table in tests.

use anyhow::{Context, Result};
use base64::Engine;
use sha2::Digest;
use std::ffi::OsStr;
use std::io::Read;

/// The seam through which soldeps-gen looks inside an npm tarball
pub trait TarballInspector {
    /// Whether the npm tarball at `url` contains a `.sol` file. With an
    /// `integrity` (SRI, as the pnpm lock records it), the tarball must
    /// match it first
    fn has_solidity(&mut self, url: &str, integrity: Option<&str>) -> Result<bool>;
}

/// How much of a tarball inspecting it may read
#[derive(Debug, Clone, Copy)]
pub struct Limits {
    /// Compressed bytes downloaded
    pub download: u64,
    /// Bytes unpacked from the gzip stream while looking for a `.sol` file
    pub unpacked: u64,
}

/// The limits the real inspector uses. Large Solidity packages are a few
/// MiB compressed (@chainlink/contracts, the largest common one, is under
/// 10 MiB, and unpacks to well under 100 MiB), so these leave an order of
/// magnitude of headroom while keeping a bogus or hostile URL from filling
/// memory or spinning on a gzip bomb.
pub const LIMITS: Limits = Limits {
    download: 128 * MIB,
    unpacked: 1024 * MIB,
};

const MIB: u64 = 1024 * 1024;

/// TarballInspector backed by downloads from the registry
///
/// A tarball is only downloaded when no verdict is recorded for it
/// (select_solidity), so nothing is cached here.
pub struct RegistryInspector {
    agent: ureq::Agent,
}

impl RegistryInspector {
    /// How long a download may take, so a stalled connection cannot hang
    /// `tk sync`
    const TIMEOUT: std::time::Duration = std::time::Duration::from_secs(60);

    pub fn new() -> Self {
        Self {
            agent: ureq::AgentBuilder::new().timeout(Self::TIMEOUT).build(),
        }
    }
}

impl TarballInspector for RegistryInspector {
    fn has_solidity(&mut self, url: &str, integrity: Option<&str>) -> Result<bool> {
        let response = self
            .agent
            .get(url)
            .call()
            .with_context(|| format!("Failed to fetch {}", url))?;
        inspect(response.into_reader(), integrity, &LIMITS)
            .with_context(|| format!("Failed to inspect {}", url))
    }
}

/// Whether a downloaded tarball contains a `.sol` file: read at most
/// `limits.download` bytes, check them against `integrity` if there is
/// one, then look through at most `limits.unpacked` unpacked bytes
pub fn inspect(download: impl Read, integrity: Option<&str>, limits: &Limits) -> Result<bool> {
    let mut bytes = Vec::new();
    download
        .take(limits.download + 1)
        .read_to_end(&mut bytes)
        .context("download failed")?;
    if bytes.len() as u64 > limits.download {
        anyhow::bail!("the tarball is larger than {} bytes", limits.download);
    }
    if let Some(integrity) = integrity {
        verify_integrity(&bytes, integrity)?;
    }
    let unpacked = Capped {
        inner: flate2::read::GzDecoder::new(bytes.as_slice()),
        left: limits.unpacked,
    };
    tarball_has_solidity(unpacked)
}

/// Check `bytes` against an SRI integrity: every hash in it with an
/// algorithm pnpm records (sha512, and sha1 for old packages; sha256 and
/// sha384 for completeness) must match, and there must be at least one
fn verify_integrity(bytes: &[u8], integrity: &str) -> Result<()> {
    let mut checked = false;
    for entry in integrity.split_whitespace() {
        // An SRI hash may carry `?<options>`, which do not affect it
        let hash = entry.split('?').next().unwrap_or(entry);
        let Some((algorithm, expected)) = hash.split_once('-') else {
            continue;
        };
        let actual = match algorithm {
            "sha512" => sha2::Sha512::digest(bytes).to_vec(),
            "sha384" => sha2::Sha384::digest(bytes).to_vec(),
            "sha256" => sha2::Sha256::digest(bytes).to_vec(),
            "sha1" => sha1::Sha1::digest(bytes).to_vec(),
            _ => continue,
        };
        let expected = base64::engine::general_purpose::STANDARD
            .decode(expected)
            .with_context(|| format!("integrity `{}` is not base64", hash))?;
        if actual != expected {
            anyhow::bail!(
                "the tarball does not match its integrity {} (it is {}-{})",
                hash,
                algorithm,
                base64::engine::general_purpose::STANDARD.encode(actual)
            );
        }
        checked = true;
    }
    if !checked {
        anyhow::bail!(
            "integrity `{}` has no hash soldeps-gen can check",
            integrity
        );
    }
    Ok(())
}

/// A reader that fails once more than `left` bytes were read from it
struct Capped<R> {
    inner: R,
    left: u64,
}

impl<R: Read> Read for Capped<R> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        // Ask for one byte more than allowed, to tell "exactly at the cap"
        // from "over it"
        let want = buf
            .len()
            .min(usize::try_from(self.left + 1).unwrap_or(usize::MAX));
        let n = self.inner.read(&mut buf[..want])?;
        if n as u64 > self.left {
            return Err(std::io::Error::other(
                "the tarball unpacks to more than the inspection limit",
            ));
        }
        self.left -= n as u64;
        Ok(n)
    }
}

/// Whether an unpacked tarball contains a regular file ending in `.sol`
fn tarball_has_solidity(tar: impl Read) -> Result<bool> {
    let mut archive = tar::Archive::new(tar);
    for entry in archive.entries().context("not a gzipped tarball")? {
        let entry = entry.context("cannot read the tarball")?;
        if !entry.header().entry_type().is_file() {
            continue;
        }
        let path = entry.path().context("tarball entry with an invalid path")?;
        if path.extension() == Some(OsStr::new("sol")) {
            return Ok(true);
        }
    }
    Ok(false)
}

/// An in-memory TarballInspector for tests: answers from a fixed table and
/// records every call
#[cfg(test)]
#[derive(Debug, Default)]
pub struct MemoryInspector {
    verdicts: std::collections::BTreeMap<String, bool>,
    /// Every URL asked about, in order
    pub calls: Vec<String>,
    /// The integrity each call was given, in order
    pub integrities: Vec<Option<String>>,
}

#[cfg(test)]
impl MemoryInspector {
    /// This inspector, answering `has_solidity` for `url`
    pub fn with(mut self, url: &str, has_solidity: bool) -> Self {
        self.verdicts.insert(url.to_string(), has_solidity);
        self
    }
}

#[cfg(test)]
impl TarballInspector for MemoryInspector {
    fn has_solidity(&mut self, url: &str, integrity: Option<&str>) -> Result<bool> {
        self.calls.push(url.to_string());
        self.integrities.push(integrity.map(str::to_string));
        self.verdicts
            .get(url)
            .copied()
            .ok_or_else(|| anyhow::anyhow!("cannot fetch {}", url))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A gzipped tarball of `files` (path, contents), laid out as npm packs
    /// them, under package/
    fn tarball(files: &[(&str, &str)]) -> Vec<u8> {
        let gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
        let mut builder = tar::Builder::new(gz);
        for (path, contents) in files {
            let mut header = tar::Header::new_gnu();
            header.set_size(contents.len() as u64);
            header.set_mode(0o644);
            header.set_cksum();
            builder
                .append_data(&mut header, format!("package/{path}"), contents.as_bytes())
                .unwrap();
        }
        builder.into_inner().unwrap().finish().unwrap()
    }

    #[test]
    fn test_tarball_with_a_sol_file_holds_solidity() {
        let tgz = tarball(&[
            ("package.json", "{}"),
            ("token/ERC20/ERC20.sol", "pragma solidity ^0.8.0;"),
        ]);
        assert!(inspect(tgz.as_slice(), None, &LIMITS).unwrap());
    }

    #[test]
    fn test_tarball_without_a_sol_file_holds_none() {
        let tgz = tarball(&[
            ("package.json", "{}"),
            ("index.js", "module.exports = {}"),
            // Neither is a Solidity source
            ("docs/solidity.md", "# .sol"),
            ("dist/contract.sol.map", "{}"),
        ]);
        assert!(!inspect(tgz.as_slice(), None, &LIMITS).unwrap());
    }

    #[test]
    fn test_a_directory_named_like_a_sol_file_is_not_one() {
        let gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
        let mut builder = tar::Builder::new(gz);
        let mut header = tar::Header::new_gnu();
        header.set_entry_type(tar::EntryType::Directory);
        header.set_size(0);
        header.set_mode(0o755);
        header.set_cksum();
        builder
            .append_data(&mut header, "package/weird.sol/", std::io::empty())
            .unwrap();
        let tgz = builder.into_inner().unwrap().finish().unwrap();
        assert!(!inspect(tgz.as_slice(), None, &LIMITS).unwrap());
    }

    /// `bytes`' SRI integrity with `algorithm`, as pnpm records it
    fn sri(algorithm: &str, bytes: &[u8]) -> String {
        use base64::Engine;
        use sha2::Digest;
        let digest = match algorithm {
            "sha512" => sha2::Sha512::digest(bytes).to_vec(),
            "sha1" => sha1::Sha1::digest(bytes).to_vec(),
            _ => unreachable!(),
        };
        format!(
            "{algorithm}-{}",
            base64::engine::general_purpose::STANDARD.encode(digest)
        )
    }

    fn sol_tarball() -> Vec<u8> {
        tarball(&[("Token.sol", "pragma solidity ^0.8.0;")])
    }

    #[test]
    fn test_inspect_verifies_the_integrity_first() {
        let tgz = sol_tarball();
        for algorithm in ["sha512", "sha1"] {
            let integrity = sri(algorithm, &tgz);
            assert!(inspect(tgz.as_slice(), Some(&integrity), &LIMITS).unwrap());
        }
        // No integrity to check against: only the contents count
        assert!(inspect(tgz.as_slice(), None, &LIMITS).unwrap());
    }

    #[test]
    fn test_inspect_rejects_bytes_that_do_not_match_the_integrity() {
        let other = sri("sha512", &tarball(&[("index.js", "")]));
        let message = format!(
            "{:#}",
            inspect(sol_tarball().as_slice(), Some(&other), &LIMITS).unwrap_err()
        );
        assert!(message.contains("integrity"), "{message}");
    }

    #[test]
    fn test_inspect_rejects_an_integrity_it_cannot_check() {
        for integrity in ["md5-AAAA", "sha512", "sha512-not base64!"] {
            assert!(
                inspect(sol_tarball().as_slice(), Some(integrity), &LIMITS).is_err(),
                "{integrity}"
            );
        }
    }

    #[test]
    fn test_inspect_caps_the_download() {
        let tgz = sol_tarball();
        let limits = Limits {
            download: tgz.len() as u64 - 1,
            ..LIMITS
        };
        let message = format!("{:#}", inspect(tgz.as_slice(), None, &limits).unwrap_err());
        assert!(message.contains("larger than"), "{message}");
    }

    #[test]
    fn test_inspect_caps_the_unpacked_bytes() {
        let tgz = tarball(&[("index.js", &"x".repeat(64 * 1024)), ("Token.sol", "")]);
        let limits = Limits {
            unpacked: 1024,
            ..LIMITS
        };
        let message = format!("{:#}", inspect(tgz.as_slice(), None, &limits).unwrap_err());
        assert!(message.contains("unpacks to more than"), "{message}");
    }

    #[test]
    fn test_a_corrupt_tarball_is_an_error() {
        assert!(inspect(&b"not gzip"[..], None, &LIMITS).is_err());
    }
}
