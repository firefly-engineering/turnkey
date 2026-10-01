//! JSON as Go's `encoding/json` decodes it into a struct
//!
//! The cell tools' configurations (pydeps-cell.json, buckgen.json) were
//! decoded by the Go versions with `encoding/json`, which a port decodes
//! the same way with a hand-written serde visitor and these helpers:
//!
//! - a key names a field when the two are equal once folded as
//!   `bytes.EqualFold` folds them ([`key_is`]);
//! - keys are taken in document order, so the last of two keys naming the
//!   same field wins, and an unknown key is ignored;
//! - a `null` leaves a string or a struct as it is, and empties a list;
//! - invalid UTF-8 in the input reads as U+FFFD ([`text`]).

/// A key folded as `encoding/json` folds field names: ASCII letters
/// uppercased, and the two non-ASCII letters that fold to an ASCII one
/// (U+017F `ſ` and U+212A, the Kelvin sign) folded to it. Fields' names
/// are ASCII, so nothing else can match them.
pub fn fold(key: &str) -> String {
    key.chars()
        .map(|c| match c {
            '\u{17f}' => 'S',
            '\u{212a}' => 'K',
            c => c.to_ascii_uppercase(),
        })
        .collect()
}

/// Whether a JSON key names the field `field`
pub fn key_is(key: &str, field: &str) -> bool {
    fold(key) == fold(field)
}

/// JSON input as `encoding/json` reads it: each byte that isn't part of a
/// valid UTF-8 sequence is U+FFFD (in a string; anywhere else it is a
/// syntax error either way)
pub fn text(mut data: &[u8]) -> String {
    let mut out = String::with_capacity(data.len());
    loop {
        match std::str::from_utf8(data) {
            Ok(rest) => {
                out.push_str(rest);
                return out;
            }
            Err(e) => {
                let valid = e.valid_up_to();
                out.push_str(std::str::from_utf8(&data[..valid]).expect("valid up to here"));
                out.push('\u{fffd}');
                data = &data[valid + 1..];
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_fold_as_go_folds_them() {
        assert!(key_is("Settings", "settings"));
        assert!(key_is("ſettings", "settings"));
        assert!(key_is("\u{212a}ey", "key"));
        assert!(!key_is("setting", "settings"));
        assert!(!key_is("séttings", "settings"));
    }

    #[test]
    fn invalid_utf8_is_replaced_byte_by_byte() {
        assert_eq!(text(b"a\xe2\x82b\xffc"), "a\u{fffd}\u{fffd}b\u{fffd}c");
        assert_eq!(text("été".as_bytes()), "été");
    }
}
