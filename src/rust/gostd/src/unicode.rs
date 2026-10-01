//! Go's `unicode` character classes
//!
//! Go classifies characters with its own Unicode tables. The general
//! categories come from `unicode-properties`, pinned to the Unicode version
//! of the Go toolchain turnkey pins (`unicode.Version`, 15.0.0 for Go
//! 1.26), so a character new in a later Unicode version is classified as
//! Go classifies it.

use unicode_properties::{GeneralCategoryGroup, UnicodeGeneralCategory};

/// `unicode.IsPrint`: letters, marks, numbers, punctuation, symbols and
/// the ASCII space (U+0020), but no other space
pub fn is_print(c: char) -> bool {
    use GeneralCategoryGroup::*;
    c == ' '
        || matches!(
            c.general_category_group(),
            Letter | Mark | Number | Punctuation | Symbol
        )
}

/// `unicode.IsLetter`: category L
pub fn is_letter(c: char) -> bool {
    c.general_category_group() == GeneralCategoryGroup::Letter
}

/// `unicode.IsSpace`: '\t', '\n', '\v', '\f', '\r', ' ', U+0085, U+00A0,
/// and the other characters with the White_Space property, which is
/// exactly Rust's [`char::is_whitespace`]
pub fn is_space(c: char) -> bool {
    c.is_whitespace()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn print_is_graphic_and_the_ascii_space() {
        for c in [
            'a',
            'Z',
            '0',
            '~',
            '!',
            ' ',
            'é',
            '日',
            '€',
            '\u{0301}',
            '\u{1fbca}',
        ] {
            assert!(is_print(c), "{c:?} is printable");
        }
        // Other spaces, controls, format characters, private use, and
        // characters Unicode 15.1 and 16.0 assigned, which Go 1.26 doesn't
        // know (checked against Go's unicode.IsPrint)
        for c in [
            '\t',
            '\n',
            '\u{7f}',
            '\u{85}',
            '\u{a0}',
            '\u{2028}',
            '\u{200b}',
            '\u{feff}',
            '\u{e000}',
            '\u{2ffc}',
            '\u{31ef}',
            '\u{1cc00}',
        ] {
            assert!(!is_print(c), "{c:?} is not printable");
        }
    }

    #[test]
    fn space_is_white_space() {
        for c in [
            '\t', '\n', '\u{b}', '\u{c}', '\r', ' ', '\u{85}', '\u{a0}', '\u{3000}',
        ] {
            assert!(is_space(c), "{c:?} is a space");
        }
        assert!(!is_space('\u{200b}'));
        assert!(!is_space('a'));
    }

    #[test]
    fn letters() {
        assert!(is_letter('a') && is_letter('é') && is_letter('日'));
        assert!(!is_letter('1') && !is_letter('-') && !is_letter('\u{0301}'));
    }
}
