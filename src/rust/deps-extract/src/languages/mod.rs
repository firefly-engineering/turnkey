//! Language-specific extractors using tree-sitter.

#[cfg(feature = "python")]
pub mod python;

#[cfg(feature = "rust")]
pub mod rust;

#[cfg(feature = "solidity")]
pub mod solidity;

#[cfg(feature = "typescript")]
pub mod typescript;
