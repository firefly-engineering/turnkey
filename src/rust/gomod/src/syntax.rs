//! The go.mod and go.work syntax: a lexer and parser for the grammar the
//! Go Modules Reference documents (<https://go.dev/ref/mod#go-mod-file-lexical>),
//! ported from `golang.org/x/mod/modfile`'s `read.go` so that a file reads
//! the same: the same tokens, statements and blocks, and the same errors.
//!
//! A file is a sequence of statements, one per line. A statement is a
//! line of tokens, or a block: tokens, "(", lines of tokens, ")". Tokens
//! are punctuation (`( ) [ ] { } ,`), strings (interpreted `"..."` or raw
//! `` `...` ``, kept quoted here) and identifiers (any other run of
//! printable non-space characters). Comments run from `//` to the end of
//! the line; one after a statement's tokens is that line's suffix comment,
//! which is where `// indirect` lives.
//!
//! Only what directives are read from is kept: whole-line comments, which
//! `x/mod` keeps for rewriting files, are skipped.

use gostd::unicode::{is_print, is_space};
use std::fmt;

/// A position in a file
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Position {
    /// Line, from 1
    pub line: usize,
    /// Character (rune) in the line, from 1
    pub line_rune: usize,
    /// Byte offset, from 0
    pub byte: usize,
}

impl Position {
    const START: Position = Position {
        line: 1,
        line_rune: 1,
        byte: 0,
    };
}

/// A comment, `//` included, without its newline
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Comment {
    pub start: Position,
    pub token: String,
}

/// A line of tokens: a statement, or a line in a block (without the
/// block's tokens)
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Line {
    pub start: Position,
    pub tokens: Vec<String>,
    /// The comment after the tokens on the same line
    pub suffix: Option<Comment>,
}

/// A factored statement: `tokens ( lines )`
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LineBlock {
    pub start: Position,
    pub tokens: Vec<String>,
    pub lines: Vec<Line>,
}

/// A statement of a file
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Stmt {
    Line(Line),
    Block(LineBlock),
}

/// An error in a file, at a position, worded as `x/mod` words it
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Error {
    pub filename: String,
    pub pos: Option<Position>,
    /// What the error is about, as Go prefixes it: the directive and the
    /// module path ("require example.com/m"), the directive alone, or ""
    pub directive: String,
    pub message: String,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // modfile.Error.Error: the column only when it isn't the first
        match self.pos {
            Some(p) if p.line_rune > 1 => {
                write!(f, "{}:{}:{}: ", self.filename, p.line, p.line_rune)?
            }
            Some(p) if p.line > 0 => write!(f, "{}:{}: ", self.filename, p.line)?,
            _ if !self.filename.is_empty() => write!(f, "{}: ", self.filename)?,
            _ => {}
        }
        if !self.directive.is_empty() {
            write!(f, "{}: ", self.directive)?;
        }
        f.write_str(&self.message)
    }
}

impl std::error::Error for Error {}

/// Every error in a file, one per line (`modfile.ErrorList`)
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ErrorList(pub Vec<Error>);

impl fmt::Display for ErrorList {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (i, e) in self.0.iter().enumerate() {
            if i > 0 {
                f.write_str("\n")?;
            }
            write!(f, "{e}")?;
        }
        Ok(())
    }
}

impl std::error::Error for ErrorList {}

/// The statements of the file named `filename` (for errors) holding
/// `data`; the first syntax error stops the parse
pub fn parse(filename: &str, data: &str) -> Result<Vec<Stmt>, Error> {
    let mut p = Parser {
        lexer: Lexer {
            filename,
            src: data,
            pos: Position::START,
        },
        token: Token::eof(Position::START),
    };
    p.advance()?;
    p.parse_file()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Eof,
    /// A comment after tokens on its line
    EolComment,
    /// A comment alone on its line
    Comment,
    Ident,
    String,
    /// `\n ( ) [ ] { } ,`
    Punct(char),
}

impl Kind {
    fn is_eol(self) -> bool {
        matches!(self, Kind::Eof | Kind::EolComment | Kind::Punct('\n'))
    }
}

#[derive(Debug, Clone)]
struct Token {
    kind: Kind,
    pos: Position,
    text: String,
}

impl Token {
    fn eof(pos: Position) -> Token {
        Token {
            kind: Kind::Eof,
            pos,
            text: String::new(),
        }
    }
}

struct Lexer<'a> {
    filename: &'a str,
    src: &'a str,
    pos: Position,
}

impl Lexer<'_> {
    fn error(&self, pos: Position, message: impl Into<String>) -> Error {
        Error {
            filename: self.filename.to_string(),
            pos: Some(pos),
            directive: String::new(),
            message: message.into(),
        }
    }

    fn rest(&self) -> &str {
        &self.src[self.pos.byte..]
    }

    fn eof(&self) -> bool {
        self.pos.byte >= self.src.len()
    }

    fn peek_char(&self) -> Option<char> {
        self.rest().chars().next()
    }

    fn read_char(&mut self) -> Option<char> {
        let c = self.peek_char()?;
        self.pos.byte += c.len_utf8();
        if c == '\n' {
            self.pos.line += 1;
            self.pos.line_rune = 1;
        } else {
            self.pos.line_rune += 1;
        }
        Some(c)
    }

    /// The next token (`readToken`)
    fn next_token(&mut self) -> Result<Token, Error> {
        // Skip spaces; a comment is a token
        while let Some(c) = self.peek_char() {
            if c == ' ' || c == '\t' || c == '\r' {
                self.read_char();
                continue;
            }
            if self.rest().starts_with("//") {
                let start = self.pos;
                // A suffix comment has more than spaces before it on its line
                let line_start = self.src[..start.byte].rfind('\n').map_or(0, |i| i + 1);
                let suffix = !self.src[line_start..start.byte]
                    .trim_matches(is_space)
                    .is_empty();
                while let Some(c) = self.read_char() {
                    if c == '\n' {
                        break;
                    }
                }
                let text = &self.src[start.byte..self.pos.byte];
                let text = text
                    .strip_suffix("\r\n")
                    .or_else(|| text.strip_suffix('\n'))
                    .unwrap_or(text);
                return Ok(Token {
                    kind: if suffix {
                        Kind::EolComment
                    } else {
                        Kind::Comment
                    },
                    pos: start,
                    text: text.to_string(),
                });
            }
            if self.rest().starts_with("/*") {
                return Err(self.error(
                    self.pos,
                    "mod files must use // comments (not /* */ comments)",
                ));
            }
            break;
        }

        let start = self.pos;
        let Some(c) = self.peek_char() else {
            return Ok(Token::eof(start));
        };
        let kind = match c {
            '\n' | '(' | ')' | '[' | ']' | '{' | '}' | ',' => {
                self.read_char();
                Kind::Punct(c)
            }
            '"' | '`' => {
                self.read_char();
                loop {
                    let Some(next) = self.peek_char() else {
                        return Err(self.error(start, "unexpected EOF in string"));
                    };
                    if next == '\n' {
                        return Err(self.error(self.pos, "unexpected newline in string"));
                    }
                    self.read_char();
                    if next == c {
                        break;
                    }
                    if next == '\\' && c != '`' {
                        if self.eof() {
                            return Err(self.error(start, "unexpected EOF in string"));
                        }
                        // The escaped character, whatever it is
                        self.read_char();
                    }
                }
                Kind::String
            }
            c if !is_ident(c) => {
                return Err(self.error(
                    self.pos,
                    format!(
                        "unexpected input character {}",
                        gostd::strconv::quote_rune(c)
                    ),
                ));
            }
            _ => {
                while let Some(c) = self.peek_char() {
                    if !is_ident(c) || self.rest().starts_with("//") {
                        break;
                    }
                    if self.rest().starts_with("/*") {
                        return Err(self.error(
                            self.pos,
                            "mod files must use // comments (not /* */ comments)",
                        ));
                    }
                    self.read_char();
                }
                Kind::Ident
            }
        };
        Ok(Token {
            kind,
            pos: start,
            text: self.src[start.byte..self.pos.byte].to_string(),
        })
    }
}

/// Whether `c` can be in an identifier: printable, not a space, not
/// punctuation
fn is_ident(c: char) -> bool {
    !matches!(c, ' ' | '(' | ')' | '[' | ']' | '{' | '}' | ',') && !is_space(c) && is_print(c)
}

/// A recursive-descent parser with one token of lookahead, as `x/mod`'s
struct Parser<'a> {
    lexer: Lexer<'a>,
    /// The next token
    token: Token,
}

impl Parser<'_> {
    /// Read the token after the next one
    fn advance(&mut self) -> Result<(), Error> {
        self.token = self.lexer.next_token()?;
        Ok(())
    }

    fn peek(&self) -> Kind {
        self.token.kind
    }

    /// The next token, reading the one after it
    fn lex(&mut self) -> Result<Token, Error> {
        let next = self.lexer.next_token()?;
        Ok(std::mem::replace(&mut self.token, next))
    }

    fn error(&self, message: impl Into<String>) -> Error {
        self.lexer.error(self.lexer.pos, message)
    }

    fn parse_file(&mut self) -> Result<Vec<Stmt>, Error> {
        let mut stmts = Vec::new();
        loop {
            match self.peek() {
                Kind::Punct('\n') | Kind::Comment => {
                    self.lex()?;
                }
                Kind::Eof => return Ok(stmts),
                _ => stmts.push(self.parse_stmt()?),
            }
        }
    }

    fn parse_stmt(&mut self) -> Result<Stmt, Error> {
        let first = self.lex()?;
        let start = first.pos;
        let mut tokens = vec![first.text];
        loop {
            let tok = self.lex()?;
            if tok.kind.is_eol() {
                return Ok(Stmt::Line(Line {
                    start,
                    tokens,
                    suffix: suffix_of(tok),
                }));
            }
            if tok.kind == Kind::Punct('(') {
                let next = self.peek();
                if next.is_eol() {
                    return self.parse_line_block(start, tokens).map(Stmt::Block);
                }
                if next == Kind::Punct(')') {
                    let rparen = self.lex()?;
                    if self.peek().is_eol() {
                        // An empty block: `require ()`
                        self.lex()?;
                        return Ok(Stmt::Block(LineBlock {
                            start,
                            tokens,
                            lines: Vec::new(),
                        }));
                    }
                    tokens.push(tok.text);
                    tokens.push(rparen.text);
                    continue;
                }
            }
            tokens.push(tok.text);
        }
    }

    fn parse_line_block(
        &mut self,
        start: Position,
        tokens: Vec<String>,
    ) -> Result<LineBlock, Error> {
        let mut lines = Vec::new();
        loop {
            match self.peek() {
                Kind::EolComment | Kind::Comment | Kind::Punct('\n') => {
                    self.lex()?;
                }
                Kind::Eof => {
                    return Err(self.error(format!(
                        "syntax error (unterminated block started at {}:{}:{})",
                        self.lexer.filename, start.line, start.line_rune
                    )));
                }
                Kind::Punct(')') => {
                    self.lex()?;
                    if !self.peek().is_eol() {
                        return Err(
                            self.error("syntax error (expected newline after closing paren)")
                        );
                    }
                    self.lex()?;
                    return Ok(LineBlock {
                        start,
                        tokens,
                        lines,
                    });
                }
                _ => lines.push(self.parse_line()?),
            }
        }
    }

    fn parse_line(&mut self) -> Result<Line, Error> {
        let first = self.lex()?;
        let start = first.pos;
        let mut tokens = vec![first.text];
        loop {
            let tok = self.lex()?;
            if tok.kind.is_eol() {
                return Ok(Line {
                    start,
                    tokens,
                    suffix: suffix_of(tok),
                });
            }
            tokens.push(tok.text);
        }
    }
}

/// The suffix comment a line ends with, if it ends with one
fn suffix_of(eol: Token) -> Option<Comment> {
    (eol.kind == Kind::EolComment).then_some(Comment {
        start: eol.pos,
        token: eol.text,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(tokens: &[&str]) -> Vec<String> {
        tokens.iter().map(|t| t.to_string()).collect()
    }

    fn shape(stmts: &[Stmt]) -> Vec<(Vec<String>, Vec<Vec<String>>)> {
        stmts
            .iter()
            .map(|s| match s {
                Stmt::Line(l) => (l.tokens.clone(), vec![]),
                Stmt::Block(b) => (
                    b.tokens.clone(),
                    b.lines.iter().map(|l| l.tokens.clone()).collect(),
                ),
            })
            .collect()
    }

    #[test]
    fn reads_lines_and_blocks() {
        let src = "// A comment\nmodule \"example.com/q\" // the module\n\ngo 1.21\n\nrequire (\n\t// first\n\texample.com/a v1.0.0 // indirect\n\n\texample.com/b v1.2.0\n)\n\nrequire ()\nreplace x => ./y\r\n";
        let stmts = parse("go.mod", src).unwrap();
        assert_eq!(
            shape(&stmts),
            vec![
                (line(&["module", "\"example.com/q\""]), vec![]),
                (line(&["go", "1.21"]), vec![]),
                (
                    line(&["require"]),
                    vec![
                        line(&["example.com/a", "v1.0.0"]),
                        line(&["example.com/b", "v1.2.0"])
                    ]
                ),
                (line(&["require"]), vec![]),
                (line(&["replace", "x", "=>", "./y"]), vec![]),
            ]
        );
        let Stmt::Line(module) = &stmts[0] else {
            panic!()
        };
        assert_eq!(module.suffix.as_ref().unwrap().token, "// the module");
        let Stmt::Block(require) = &stmts[2] else {
            panic!()
        };
        assert_eq!(
            require.lines[0].suffix.as_ref().unwrap().token,
            "// indirect"
        );
        assert_eq!(require.lines[1].suffix, None);
        assert_eq!(
            require.lines[0].start,
            Position {
                line: 8,
                line_rune: 2,
                byte: 81
            }
        );
    }

    #[test]
    fn tokens_split_on_punctuation_and_comments() {
        let stmts = parse(
            "go.mod",
            "retract [v1.0.0,v1.1.0]//why\nx `raw str`\"q\"y\n",
        )
        .unwrap();
        assert_eq!(
            shape(&stmts),
            vec![
                (
                    line(&["retract", "[", "v1.0.0", ",", "v1.1.0", "]"]),
                    vec![]
                ),
                (line(&["x", "`raw str`", "\"q\"", "y"]), vec![]),
            ]
        );
        let Stmt::Line(retract) = &stmts[0] else {
            panic!()
        };
        assert_eq!(retract.suffix.as_ref().unwrap().token, "//why");
    }

    #[test]
    fn parens_not_at_line_end_are_tokens() {
        let stmts = parse("go.mod", "a ( b\nc () d\ne (\n f )\n)\n").unwrap();
        assert_eq!(
            shape(&stmts),
            vec![
                (line(&["a", "(", "b"]), vec![]),
                (line(&["c", "(", ")", "d"]), vec![]),
                (line(&["e"]), vec![line(&["f", ")"])]),
            ]
        );
    }

    #[test]
    fn syntax_errors() {
        let cases = [
            (
                "module x /* no */\n",
                "go.mod:1:10: mod files must use // comments (not /* */ comments)",
            ),
            ("module \"x\n", "go.mod:1:10: unexpected newline in string"),
            ("module \"x", "go.mod:1:8: unexpected EOF in string"),
            (
                "require (\n\tx v1.0.0\n",
                "go.mod:3: syntax error (unterminated block started at go.mod:1:1)",
            ),
            (
                "require (\n) x\n",
                "go.mod:2:4: syntax error (expected newline after closing paren)",
            ),
            (
                "module x\u{7}\n",
                "go.mod:1:9: unexpected input character '\\a'",
            ),
        ];
        for (src, want) in cases {
            assert_eq!(
                parse("go.mod", src).unwrap_err().to_string(),
                want,
                "{src:?}"
            );
        }
    }
}
