//! Go's `strconv.Quote` and `strconv.Unquote`
//!
//! `fmt`'s `%q` verb on a string is `strconv.Quote`, which is how Go's
//! generators wrote string values: Go escapes, not Rust's
//! (`"\u{1}"` is `"\x01"`, a non-printable non-ASCII character is `\uXXXX`
//! or `\UXXXXXXXX`, a printable one stays as it is).

use crate::unicode::is_print;
use std::fmt;

/// `strconv.Quote`: `s` as a double-quoted Go string literal
pub fn quote(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        escape_rune(&mut out, c, '"');
    }
    out.push('"');
    out
}

/// `strconv.QuoteRune`: `c` as a single-quoted Go character literal
pub fn quote_rune(c: char) -> String {
    let mut out = String::from("'");
    escape_rune(&mut out, c, '\'');
    out.push('\'');
    out
}

/// `appendEscapedRune` for a valid rune, without ASCII-only or
/// graphic-only quoting
fn escape_rune(out: &mut String, c: char, quote: char) {
    if c == quote || c == '\\' {
        out.push('\\');
        out.push(c);
        return;
    }
    if is_print(c) {
        out.push(c);
        return;
    }
    match c {
        '\u{7}' => out.push_str("\\a"),
        '\u{8}' => out.push_str("\\b"),
        '\u{c}' => out.push_str("\\f"),
        '\n' => out.push_str("\\n"),
        '\r' => out.push_str("\\r"),
        '\t' => out.push_str("\\t"),
        '\u{b}' => out.push_str("\\v"),
        c if c < ' ' || c == '\u{7f}' => out.push_str(&format!("\\x{:02x}", c as u32)),
        c if (c as u32) < 0x10000 => out.push_str(&format!("\\u{:04x}", c as u32)),
        c => out.push_str(&format!("\\U{:08x}", c as u32)),
    }
}

/// The error `strconv.Unquote` returns: `strconv.ErrSyntax`
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SyntaxError;

impl fmt::Display for SyntaxError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("invalid syntax")
    }
}

impl std::error::Error for SyntaxError {}

/// `strconv.Unquote`: the value of a double-quoted, single-quoted or
/// back-quoted Go literal, which must be all of `s`
///
/// Go's result is bytes, which `\x` and octal escapes can make invalid
/// UTF-8; here that is an error ([`unquote_bytes`] has the bytes).
pub fn unquote(s: &str) -> Result<String, SyntaxError> {
    String::from_utf8(unquote_bytes(s)?).map_err(|_| SyntaxError)
}

/// `strconv.Unquote`, with its result as Go has it: bytes, which need not
/// be UTF-8
pub fn unquote_bytes(s: &str) -> Result<Vec<u8>, SyntaxError> {
    let bytes = s.as_bytes();
    if bytes.len() < 2 {
        return Err(SyntaxError);
    }
    let quote = bytes[0];
    if bytes[bytes.len() - 1] != quote {
        return Err(SyntaxError);
    }
    let body = &s[1..s.len() - 1];
    match quote {
        b'`' => {
            if body.contains('`') {
                return Err(SyntaxError);
            }
            // Carriage returns are discarded from raw strings
            Ok(body.replace('\r', "").into_bytes())
        }
        b'"' | b'\'' => {
            let mut out: Vec<u8> = Vec::with_capacity(body.len());
            let mut rest = body;
            let mut runes = 0;
            while !rest.is_empty() {
                if rest.starts_with('\n') {
                    return Err(SyntaxError);
                }
                let (value, tail) = unquote_char(rest, quote)?;
                match value {
                    Unquoted::Byte(b) => out.push(b),
                    Unquoted::Char(c) => {
                        let mut buf = [0; 4];
                        out.extend_from_slice(c.encode_utf8(&mut buf).as_bytes());
                    }
                }
                rest = tail;
                runes += 1;
            }
            // A single-quoted literal is one character, or none: Go's
            // fast path takes '' for the empty string
            if quote == b'\'' && runes > 1 {
                return Err(SyntaxError);
            }
            Ok(out)
        }
        _ => Err(SyntaxError),
    }
}

/// `strconv.QuotedPrefix`: the quoted string (as `unquote` reads it) at
/// the start of `s`, quotes included
pub fn quoted_prefix(s: &str) -> Result<&str, SyntaxError> {
    let bytes = s.as_bytes();
    if bytes.len() < 2 {
        return Err(SyntaxError);
    }
    let quote = bytes[0];
    // Where the literal ends if it has no escaped quote
    let end = bytes[1..]
        .iter()
        .position(|&b| b == quote)
        .ok_or(SyntaxError)?
        + 2;
    match quote {
        b'`' => Ok(&s[..end]),
        b'"' | b'\'' => {
            let lit = &s[..end];
            // Without escapes or newlines, it ends there: a single-quoted
            // literal must hold one character, or none
            if !lit.contains('\\') && !lit.contains('\n') {
                let body = &s[1..end - 1];
                if quote == b'"' || body.chars().count() <= 1 {
                    return Ok(lit);
                }
            }
            let mut rest = &s[1..];
            while !rest.is_empty() && rest.as_bytes()[0] != quote {
                if rest.starts_with('\n') {
                    return Err(SyntaxError);
                }
                let (_, tail) = unquote_char(rest, quote)?;
                rest = tail;
                if quote == b'\'' {
                    break;
                }
            }
            if rest.as_bytes().first() != Some(&quote) {
                return Err(SyntaxError);
            }
            Ok(&s[..s.len() - rest.len() + 1])
        }
        _ => Err(SyntaxError),
    }
}

/// One decoded character of a quoted literal: a byte from `\x` or an
/// octal escape, which need not be UTF-8, or a character
enum Unquoted {
    Byte(u8),
    Char(char),
}

/// `strconv.UnquoteChar`: the first character of `s` (an escape sequence
/// or a literal character) and the rest of `s`
fn unquote_char(s: &str, quote: u8) -> Result<(Unquoted, &str), SyntaxError> {
    let c = s.chars().next().ok_or(SyntaxError)?;
    if c as u32 == quote as u32 && (quote == b'\'' || quote == b'"') {
        return Err(SyntaxError);
    }
    if c != '\\' {
        return Ok((Unquoted::Char(c), &s[c.len_utf8()..]));
    }

    let bytes = s.as_bytes();
    if bytes.len() <= 1 {
        return Err(SyntaxError);
    }
    let e = bytes[1];
    let rest = &s[2..];
    let simple = |c: char| Ok((Unquoted::Char(c), rest));
    match e {
        b'a' => simple('\u{7}'),
        b'b' => simple('\u{8}'),
        b'f' => simple('\u{c}'),
        b'n' => simple('\n'),
        b'r' => simple('\r'),
        b't' => simple('\t'),
        b'v' => simple('\u{b}'),
        b'x' | b'u' | b'U' => {
            let n = match e {
                b'x' => 2,
                b'u' => 4,
                _ => 8,
            };
            let digits = rest.as_bytes().get(..n).ok_or(SyntaxError)?;
            let mut v: u32 = 0;
            for &d in digits {
                v = (v << 4) | (d as char).to_digit(16).ok_or(SyntaxError)?;
            }
            let tail = &rest[n..];
            if e == b'x' {
                return Ok((Unquoted::Byte(v as u8), tail));
            }
            // A surrogate or a value above U+10FFFF is no rune
            let c = char::from_u32(v).ok_or(SyntaxError)?;
            Ok((Unquoted::Char(c), tail))
        }
        b'0'..=b'7' => {
            let digits = rest.as_bytes().get(..2).ok_or(SyntaxError)?;
            let mut v: u32 = (e - b'0') as u32;
            for &d in digits {
                if !(b'0'..=b'7').contains(&d) {
                    return Err(SyntaxError);
                }
                v = (v << 3) | (d - b'0') as u32;
            }
            if v > 255 {
                return Err(SyntaxError);
            }
            Ok((Unquoted::Byte(v as u8), &rest[2..]))
        }
        b'\\' => simple('\\'),
        b'\'' | b'"' if e == quote => simple(e as char),
        _ => Err(SyntaxError),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quote_escapes_as_go_does() {
        // Results from Go 1.26's strconv.Quote
        let cases = [
            ("", r#""""#),
            ("go.work", r#""go.work""#),
            ("a\"b\\c", r#""a\"b\\c""#),
            ("\u{7}\u{8}\u{c}\n\r\t\u{b}", r#""\a\b\f\n\r\t\v""#),
            ("\u{1}\u{1f}\u{7f}", r#""\x01\x1f\x7f""#),
            ("café 日本", "\"café 日本\""),
            ("\u{85}\u{a0}\u{200b}", "\"\\u0085\\u00a0\\u200b\""),
            ("\u{e0001}", r#""\U000e0001""#),
            ("'", r#""'""#),
        ];
        for (input, want) in cases {
            assert_eq!(quote(input), want, "quote({input:?})");
        }
    }

    #[test]
    fn quote_rune_escapes_as_go_does() {
        let cases = [
            ('a', "'a'"),
            ('\'', r"'\''"),
            ('"', "'\"'"),
            ('\\', r"'\\'"),
            ('\u{7}', r"'\a'"),
            ('\u{a0}', r"'\u00a0'"),
            ('é', "'é'"),
        ];
        for (input, want) in cases {
            assert_eq!(quote_rune(input), want, "quote_rune({input:?})");
        }
    }

    #[test]
    fn unquote_reads_go_literals() {
        let ok = [
            (r#""""#, ""),
            (r#""example.com/m""#, "example.com/m"),
            (r#""a\tb\n""#, "a\tb\n"),
            (r#""\x41\101é\U0001F600""#, "AAé😀"),
            (r#""\"\\""#, "\"\\"),
            ("`raw\\n`", "raw\\n"),
            ("`a\rb`", "ab"),
            ("'x'", "x"),
            (r"'\''", "'"),
            ("\"café\"", "café"),
            ("''", ""),
        ];
        for (input, want) in ok {
            assert_eq!(unquote(input).as_deref(), Ok(want), "unquote({input:?})");
        }
        let bad = [
            "",
            "\"",
            "x",
            "\"abc",
            "\"a\"b\"",
            r#""\'""#,
            r#""\q""#,
            r#""\x4""#,
            r#""\400""#,
            r#""\uD800""#,
            r#""\U00110000""#,
            "\"a\nb\"",
            "'ab'",
            "`a`b`",
            r#""\xff""#,
        ];
        for input in bad {
            assert_eq!(unquote(input), Err(SyntaxError), "unquote({input:?})");
        }
    }

    /// Go 1.26's strconv tests of QuotedPrefix (testUnquote): a quoted
    /// literal's prefix is the literal, whatever follows it, and a
    /// malformed one has none
    #[test]
    fn unquote_bytes_keeps_what_isnt_utf8() {
        assert_eq!(unquote_bytes(r#""abc\xffdef""#), Ok(b"abc\xffdef".to_vec()));
        assert_eq!(unquote_bytes(r#""\377""#), Ok(vec![0xff]));
        assert_eq!(unquote_bytes("`\\x00`"), Ok(b"\\x00".to_vec()));
        assert_eq!(unquote_bytes(r#""\q""#), Err(SyntaxError));
    }

    #[test]
    fn quoted_prefix_as_go() {
        let ok = [
            r#""""#,
            r#""a""#,
            r#""☺""#,
            r#""\xFF""#,
            r#""\377""#,
            r#""\u1234""#,
            r#""\U00010111""#,
            r#""\a\b\f\n\r\t\v\\\"""#,
            r#""'""#,
            "'a'",
            "'☹'",
            r"'\a'",
            r"'\x10'",
            r"'\u1234'",
            r"'\''",
            "'\"'",
            "''",
            "``",
            "`\\`",
            "`\n`",
            "`a\rb`",
        ];
        for lit in ok {
            let suffix: String = "\n\r\\\"`'"
                .chars()
                .filter(|&c| !lit.starts_with(c))
                .collect();
            let input = format!("{lit}{suffix}");
            assert_eq!(quoted_prefix(&input), Ok(lit), "quoted_prefix({input:?})");
        }
        // Go's misquoted strings with no valid prefix
        let bad = [
            "",
            "\"",
            "\"a",
            "\"'",
            "b\"",
            r#""\9""#,
            r#""\19""#,
            r#""\129""#,
            r"'\9'",
            r"'\19'",
            r"'\129'",
            "'ab'",
            r#""\x1!""#,
            r#""\U12345678""#,
            r#""\z""#,
            "`",
            "`xxx",
            r#""\'""#,
            r#"'\"'"#,
            "\"\n\"",
            "\"\\n\n\"",
            "'\n'",
            r#""\udead""#,
        ];
        for input in bad {
            assert_eq!(
                quoted_prefix(input),
                Err(SyntaxError),
                "quoted_prefix({input:?})"
            );
        }
        // A literal followed by more is its prefix
        assert_eq!(quoted_prefix("\"a\" b"), Ok("\"a\""));
        assert_eq!(quoted_prefix("'' x"), Ok("''"));
        assert_eq!(quoted_prefix("`x` y"), Ok("`x`"));
        assert_eq!(quoted_prefix(r#""\x41"z"#), Ok(r#""\x41""#));
    }
}
