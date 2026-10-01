//! Go's `strings` case mapping
//!
//! Go lowers a string one character at a time, with each character's
//! simple lowercase mapping from its own Unicode tables. Rust's
//! [`str::to_lowercase`] differs: it applies the full mappings (U+0130 `İ`
//! becomes two characters), lowers a final `Σ` to `ς`, and knows the
//! characters of the Unicode version Rust ships, which is later than Go's.

use unicode_properties::{GeneralCategory, UnicodeGeneralCategory};

/// `strings.ToLower`: `s` with every character mapped to its simple
/// lowercase, as Go's `unicode.ToLower` maps it
pub fn to_lower(s: &str) -> String {
    if s.is_ascii() {
        return s.to_ascii_lowercase();
    }
    s.chars().map(to_lower_char).collect()
}

/// `unicode.ToLower`: `c`'s simple lowercase mapping
///
/// Rust's mapping is the simple one except for U+0130, the one character
/// whose full lowercase is more than one character. A character Go's
/// Unicode version (15.0, as `unicode-properties` is pinned) doesn't
/// assign has no mapping in Go.
pub fn to_lower_char(c: char) -> char {
    if c.is_ascii() {
        return c.to_ascii_lowercase();
    }
    if c == '\u{130}' {
        return 'i';
    }
    if c.general_category() == GeneralCategory::Unassigned {
        return c;
    }
    let mut lower = c.to_lowercase();
    match (lower.next(), lower.next()) {
        (Some(l), None) => l,
        _ => c,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lowers_as_go_does() {
        // Checked against Go 1.26's strings.ToLower
        for (s, want) in [
            ("", ""),
            ("Foo_Bar.BAZ", "foo_bar.baz"),
            ("ÉTÉ", "été"),
            ("İstanbul", "istanbul"),
            ("ΟΔΟΣ", "οδοσ"),
            ("\u{212a}", "k"),
            ("ǅ", "ǆ"),
            // Unicode 16.0's Latin capital letters, which Go 1.26 doesn't
            // know
            ("\u{a7cb}\u{a7dc}", "\u{a7cb}\u{a7dc}"),
            ("日本", "日本"),
        ] {
            assert_eq!(to_lower(s), want, "{s:?}");
        }
    }
}
