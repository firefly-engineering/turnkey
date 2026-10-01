//! The comments of a Go file, as Go's scanner finds them in any file,
//! whether or not it parses
//!
//! The Go version found `//go:embed` directives by tokenizing the whole
//! file with go/scanner, which goes on after an error. tree-sitter-go's
//! error recovery can swallow a comment in a body that doesn't parse, so
//! the comments are found here with the part of Go's lexical grammar that
//! decides where one is: comments, and the string, raw string and rune
//! literals that can hold `//` or `/*`.

/// A comment: where it starts, and its text as go/scanner returns it (a
/// `//` comment without its newline and carriage returns)
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Comment {
    pub start: usize,
    pub text: Vec<u8>,
}

/// Every comment of `src`, in order
pub(crate) fn comments(src: &[u8]) -> Vec<Comment> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < src.len() {
        match src[i] {
            b'/' if src.get(i + 1) == Some(&b'/') => {
                let end = src[i..]
                    .iter()
                    .position(|&b| b == b'\n')
                    .map_or(src.len(), |n| i + n);
                let text = src[i..end]
                    .iter()
                    .copied()
                    .filter(|&b| b != b'\r')
                    .collect();
                out.push(Comment { start: i, text });
                i = end;
            }
            b'/' if src.get(i + 1) == Some(&b'*') => {
                let end = src[i + 2..]
                    .windows(2)
                    .position(|w| w == b"*/")
                    .map_or(src.len(), |n| i + 2 + n + 2);
                out.push(Comment {
                    start: i,
                    text: src[i..end].to_vec(),
                });
                i = end;
            }
            b'`' => {
                // A raw string runs to the next backquote, or the end
                i = src[i + 1..]
                    .iter()
                    .position(|&b| b == b'`')
                    .map_or(src.len(), |n| i + 1 + n + 1);
            }
            quote @ (b'"' | b'\'') => i = skip_quoted(src, i + 1, quote),
            _ => i += 1,
        }
    }
    out
}

/// Where a string or rune literal whose body starts at `i` ends: after its
/// closing quote, or at the newline or end it stops at unterminated. An
/// escape's backslash takes the character after it, but never a newline.
fn skip_quoted(src: &[u8], mut i: usize, quote: u8) -> usize {
    while i < src.len() {
        match src[i] {
            b'\n' => return i,
            b'\\' => {
                i += 1;
                if i < src.len() && src[i] != b'\n' {
                    i += 1;
                }
            }
            c if c == quote => return i + 1,
            _ => i += 1,
        }
    }
    i
}

#[cfg(test)]
mod tests {
    use super::*;

    fn texts(src: &str) -> Vec<String> {
        comments(src.as_bytes())
            .into_iter()
            .map(|c| String::from_utf8(c.text).unwrap())
            .collect()
    }

    #[test]
    fn comments_outside_literals() {
        assert_eq!(
            texts("// a\r\nx := \"// no\" + `/* no\n*/` + '/' /* b\r\n */ // c"),
            ["// a", "/* b\r\n */", "// c"]
        );
        // Escaped quotes don't end a literal; a newline ends an
        // unterminated one, and an escape doesn't take it
        assert_eq!(
            texts("s := \"\\\" // no\"\n'\\'' // a\n\"open // no\n// b\n\"\\\n// c"),
            ["// a", "// b", "// c"]
        );
        // An unterminated block comment or raw string runs to the end
        assert_eq!(texts("/* a\n// no"), ["/* a\n// no"]);
        assert!(texts("`a\n// no").is_empty());
        assert_eq!(comments(b"x // y").first().map(|c| c.start), Some(2));
    }
}
