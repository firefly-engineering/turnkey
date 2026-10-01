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
//! - an array decodes into the slice that is there ([`Slice`]);
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

/// A slice field, decoded as `encoding/json` decodes into a Go slice
///
/// An array decodes each element into the one already at its index, if
/// any, the slice ending as long as the array; the elements past its end
/// that its capacity still holds are reused by a later array. An empty
/// array or a `null` leaves a slice with no capacity.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Slice<T> {
    /// The field's value
    pub items: Vec<T>,
    backing: Vec<T>,
}

impl<T: Default> Slice<T> {
    /// Decodes `array` (`None` for a `null`) into the slice, `apply`
    /// decoding each element into its slot: a new slot is `T::default()`
    pub fn decode<A>(&mut self, array: Option<Vec<A>>, mut apply: impl FnMut(A, &mut T)) {
        let array = match array {
            Some(array) if !array.is_empty() => array,
            _ => {
                self.items.clear();
                self.backing.clear();
                return;
            }
        };
        let mut all = std::mem::take(&mut self.items);
        all.append(&mut self.backing);
        let n = array.len();
        for (i, element) in array.into_iter().enumerate() {
            if i >= all.len() {
                all.push(T::default());
            }
            apply(element, &mut all[i]);
        }
        self.backing = all.split_off(n);
        self.items = all;
    }
}

/// Decodes a JSON string or `null` into a string: a `null` leaves it as it
/// is
pub fn set_string(value: Option<String>, slot: &mut String) {
    if let Some(value) = value {
        *slot = value;
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
    fn slices_reuse_their_capacity() {
        let mut s = Slice::<String>::default();
        let strings = |v: &[Option<&str>]| Some(v.iter().map(|s| s.map(str::to_string)).collect());
        s.decode(strings(&[Some("a"), Some("b")]), set_string);
        s.decode(strings(&[None]), set_string);
        assert_eq!(s.items, ["a"]);
        // The second element is still in the capacity
        s.decode(strings(&[Some("x"), None, None]), set_string);
        assert_eq!(s.items, ["x", "b", ""]);
        // An empty array drops the capacity, as null does
        s.decode(strings(&[]), set_string);
        assert!(s.items.is_empty());
        s.decode(strings(&[None]), set_string);
        assert_eq!(s.items, [""]);
        s.decode(None::<Vec<Option<String>>>, set_string);
        s.decode(strings(&[None]), set_string);
        assert_eq!(s.items, [""]);
    }

    #[test]
    fn invalid_utf8_is_replaced_byte_by_byte() {
        assert_eq!(text(b"a\xe2\x82b\xffc"), "a\u{fffd}\u{fffd}b\u{fffd}c");
        assert_eq!(text("été".as_bytes()), "été");
    }
}
