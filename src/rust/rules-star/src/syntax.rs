//! The syntax tree of a `rules.star` file, as go.starlark.net builds it
//!
//! What rules sync decides depends on the tree go.starlark.net's parser
//! builds (`syntax.Parse` with `RetainComments`): which statement a
//! `# turnkey:no-sync` comment is attached to, which comments are inside a
//! deps list (its turnkey markers), and the byte spans rewritten. The file
//! is parsed with starlark_syntax, the parser Buck2 itself uses (#193), and
//! this module rebuilds go.starlark.net's tree from its AST:
//!
//! - the same nodes: a parenthesized expression is a node of its own
//!   (starlark_syntax drops the parentheses), a named argument is a
//!   `name = value` binary expression, `x[a, b]` indexes with a tuple;
//! - visited in the order `syntax.Walk` visits them (a conditional
//!   expression's condition first, a load statement's original names before
//!   its local ones);
//! - spanning what `syntax.Node.Span` reports (`go_end`), which ends an
//!   index, a slice, an empty tuple and a load statement at their closing
//!   bracket rather than after it, and so ends any node ending with one;
//!   `end` is where the node's text really ends (the starlark package's
//!   nodeEnd);
//! - with the comments attached as parse.go's `assignComments` attaches
//!   them: each comment alone on its line to the first node, in pre-order,
//!   that starts after it; each comment following code on its line to the
//!   last node, in post-order, that ends before it.
//!
//! Offsets are bytes. go.starlark.net's positions are lines and columns of
//! runes, which order the same way.

use starlark_syntax::codemap::CodeMap;
use starlark_syntax::lexer::Lexer;
use starlark_syntax::lexer::Token;
use starlark_syntax::syntax::AstModule;
use starlark_syntax::syntax::Dialect;
use starlark_syntax::syntax::ast::*;

/// A node of the tree, by its index in [`Tree::nodes`]
pub(crate) type NodeId = usize;

/// The tree's root, the file
pub(crate) const ROOT: NodeId = 0;

/// A `#` comment
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Comment {
    /// Where its `#` is
    pub start: usize,
    /// The comment, `#` included, up to the end of its line
    pub text: String,
}

/// The comments attached to a node
#[derive(Debug, Default)]
pub(crate) struct Comments {
    /// Comments alone on their line, before the node
    pub before: Vec<Comment>,
    /// A comment following the node on its line
    pub suffix: Vec<Comment>,
}

/// A binary expression's operator, as far as rules sync is concerned
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Op {
    /// `=`: a named argument, `name = value`
    Eq,
    /// `+`
    Plus,
    /// Any other operator
    Other,
}

/// What a node is
#[derive(Debug)]
pub(crate) enum Kind {
    /// The file: children are its statements
    File,
    /// An expression statement: children `[x]`
    ExprStmt,
    /// A load statement: children are the module, the original names and
    /// the local ones. Symbols are (local name, original name).
    Load {
        module: String,
        symbols: Vec<(String, String)>,
    },
    /// Any other statement
    Stmt,
    /// An identifier
    Ident(String),
    /// A string literal, decoded
    String(String),
    /// An integer literal, as written
    Int(String),
    /// Any other literal
    Literal,
    /// `[x, ...]`
    List,
    /// `{entry, ...}`: children are entries
    Dict,
    /// `key: value`: children `[key, value]`
    DictEntry,
    /// `fn(args)`: children `[fn, args...]`; `rparen` is where its `)` is
    Call { rparen: usize },
    /// `x op y`: children `[x, y]`
    Binary(Op),
    /// `(x)`: children `[x]`
    Paren,
    /// A tuple: in parentheses only when empty, `()`
    Tuple { parens: bool },
    /// Any other expression
    Expr,
}

/// A node of the tree
#[derive(Debug)]
pub(crate) struct Node {
    pub kind: Kind,
    /// Where it starts
    pub start: usize,
    /// Where go.starlark.net's `Span` ends it
    pub go_end: usize,
    /// Where its text ends
    pub end: usize,
    /// Its children, in `syntax.Walk` order
    pub children: Vec<NodeId>,
    /// The comments attached to it
    pub comments: Comments,
}

/// A file's syntax tree
#[derive(Debug)]
pub(crate) struct Tree {
    /// Every node, [`ROOT`] first
    pub nodes: Vec<Node>,
}

impl Tree {
    /// The file's statements
    pub fn stmts(&self) -> &[NodeId] {
        &self.nodes[ROOT].children
    }

    /// A node
    pub fn node(&self, id: NodeId) -> &Node {
        &self.nodes[id]
    }

    /// The node and every node below it, in pre-order
    pub fn walk(&self, id: NodeId) -> Vec<NodeId> {
        let mut pre = Vec::new();
        let mut stack = vec![id];
        while let Some(n) = stack.pop() {
            pre.push(n);
            stack.extend(self.nodes[n].children.iter().rev());
        }
        pre
    }
}

/// The dialect go.starlark.net's parser accepts: `def`, `lambda`, `load`,
/// keyword-only arguments, and `if` and `for` at the top level (the file
/// options that restrict them are the resolver's, which turnkey doesn't
/// run); no types, positional-only arguments or f-strings.
fn dialect() -> Dialect {
    Dialect {
        enable_keyword_only_arguments: true,
        enable_top_level_stmt: true,
        ..Dialect::Standard
    }
}

/// Parses a file's source into its tree, or returns the parser's error.
pub(crate) fn parse(path: &str, source: &str) -> Result<Tree, String> {
    let dialect = dialect();
    let module =
        AstModule::parse(path, source.to_string(), &dialect).map_err(|err| match err.span() {
            Some(span) => format!("{span}: {}", err.without_diagnostic()),
            None => err.without_diagnostic().to_string(),
        })?;

    let codemap = CodeMap::new(path.to_string(), source.to_string());
    let tokens = Lexer::new(source, &dialect, codemap)
        .filter_map(|lexeme| {
            let (start, token, end) = lexeme.ok()?;
            let tok = match token {
                Token::OpeningRound => Tok::LParen,
                Token::ClosingRound => Tok::RParen,
                Token::Comma => Tok::Comma,
                Token::For => Tok::For,
                Token::If => Tok::If,
                Token::Comment(_) | Token::Newline | Token::Indent | Token::Dedent => return None,
                _ => Tok::Other,
            };
            Some((start, tok, end))
        })
        .collect();

    let mut b = Builder {
        source,
        tokens,
        nodes: Vec::new(),
    };
    b.add(Kind::File, 0, 0, 0, Vec::new());
    let mut stmts = Vec::new();
    b.stmt(module.statement(), &mut stmts);
    let (start, go_end) = match (stmts.first(), stmts.last()) {
        (Some(&first), Some(&last)) => (b.nodes[first].start, b.nodes[last].go_end),
        _ => (0, 0),
    };
    let root = &mut b.nodes[ROOT];
    (root.start, root.go_end, root.end) = (start, go_end, go_end);
    root.children = stmts;

    let comments = module
        .comments()
        .iter()
        .map(|span| {
            let (start, end) = (span.begin().get() as usize, span.end().get() as usize);
            Comment {
                start,
                text: source[start..end].to_string(),
            }
        })
        .collect();
    b.attach(comments);
    Ok(Tree { nodes: b.nodes })
}

/// A token, as far as rebuilding the tree needs to tell them apart
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Tok {
    LParen,
    RParen,
    Comma,
    For,
    If,
    Other,
}

/// Builds the tree
struct Builder<'a> {
    source: &'a str,
    /// The file's tokens, without comments and layout
    tokens: Vec<(usize, Tok, usize)>,
    nodes: Vec<Node>,
}

/// A starlark_syntax span's byte offsets
fn offsets(span: starlark_syntax::codemap::Span) -> (usize, usize) {
    (span.begin().get() as usize, span.end().get() as usize)
}

impl Builder<'_> {
    fn add(
        &mut self,
        kind: Kind,
        start: usize,
        go_end: usize,
        end: usize,
        children: Vec<NodeId>,
    ) -> NodeId {
        self.nodes.push(Node {
            kind,
            start,
            go_end,
            end,
            children,
            comments: Comments::default(),
        });
        self.nodes.len() - 1
    }

    /// A node spanning its children: from the first's start to the last's
    /// end, as most of go.starlark.net's nodes are
    fn spanning(&mut self, kind: Kind, children: Vec<NodeId>) -> NodeId {
        let first = &self.nodes[children[0]];
        let last = &self.nodes[children[children.len() - 1]];
        let (start, go_end, end) = (first.start, last.go_end, last.end);
        self.add(kind, start, go_end, end, children)
    }

    /// A node from `start` to its last child's end
    fn from(&mut self, kind: Kind, start: usize, children: Vec<NodeId>) -> NodeId {
        let last = &self.nodes[children[children.len() - 1]];
        let (go_end, end) = (last.go_end, last.end);
        self.add(kind, start, go_end, end, children)
    }

    /// A statement or a comprehension's clause from `start` to its last
    /// child's end. nodeEnd leaves it where go.starlark.net ends it: its
    /// text is never rewritten on its own.
    fn clause_from(&mut self, kind: Kind, start: usize, children: Vec<NodeId>) -> NodeId {
        let go_end = self.nodes[children[children.len() - 1]].go_end;
        self.add(kind, start, go_end, go_end, children)
    }

    /// A statement spanning its children
    fn stmt_spanning(&mut self, kind: Kind, children: Vec<NodeId>) -> NodeId {
        let start = self.nodes[children[0]].start;
        self.clause_from(kind, start, children)
    }

    /// The last token ending at or before `pos`
    fn token_before(&self, pos: usize) -> Option<(usize, Tok, usize)> {
        let i = self.tokens.partition_point(|t| t.2 <= pos);
        i.checked_sub(1).map(|i| self.tokens[i])
    }

    /// The first token starting at or after `pos`
    fn token_after(&self, pos: usize) -> Option<(usize, Tok, usize)> {
        let i = self.tokens.partition_point(|t| t.0 < pos);
        self.tokens.get(i).copied()
    }

    /// The last token of kind `tok` ending at or before `pos`
    fn last_before(&self, pos: usize, tok: Tok) -> Option<usize> {
        let i = self.tokens.partition_point(|t| t.2 <= pos);
        self.tokens[..i]
            .iter()
            .rev()
            .find(|t| t.1 == tok)
            .map(|t| t.0)
    }

    /// The node `id` within the parentheses written around it, each pair a
    /// parenthesized expression, as go.starlark.net parses them. `enclosing`
    /// are the offsets of a call's parentheses, which aren't its argument's
    /// own.
    fn parenthesized(&mut self, mut id: NodeId, enclosing: Option<(usize, usize)>) -> NodeId {
        loop {
            let node = &self.nodes[id];
            let Some((lparen, Tok::LParen, _)) = self.token_before(node.start) else {
                break;
            };
            let mut after = node.end;
            if let Kind::Tuple { parens: false } = node.kind {
                // a tuple's trailing comma is inside the parentheses
                if let Some((_, Tok::Comma, comma_end)) = self.token_after(after) {
                    after = comma_end;
                }
            }
            let Some((rparen, Tok::RParen, end)) = self.token_after(after) else {
                break;
            };
            if enclosing == Some((lparen, rparen)) {
                break;
            }
            id = self.add(Kind::Paren, lparen, end, end, vec![id]);
        }
        id
    }

    /// Appends the statements `stmt` is to `out`
    fn stmt(&mut self, stmt: &AstStmt, out: &mut Vec<NodeId>) {
        let (s, t) = offsets(stmt.span);
        let id = match &stmt.node {
            StmtP::Statements(stmts) => {
                for stmt in stmts {
                    self.stmt(stmt, out);
                }
                return;
            }
            StmtP::Break | StmtP::Continue | StmtP::Pass => self.add(Kind::Stmt, s, t, t, vec![]),
            StmtP::Return(None) => self.add(Kind::Stmt, s, t, t, vec![]),
            StmtP::Return(Some(x)) => {
                let x = self.expr(x, None);
                self.clause_from(Kind::Stmt, s, vec![x])
            }
            StmtP::Expression(x) => {
                let x = self.expr(x, None);
                self.stmt_spanning(Kind::ExprStmt, vec![x])
            }
            StmtP::Assign(assign) => {
                let lhs = self.target(&assign.lhs);
                let rhs = self.expr(&assign.rhs, None);
                self.stmt_spanning(Kind::Stmt, vec![lhs, rhs])
            }
            StmtP::AssignModify(lhs, _, rhs) => {
                let lhs = self.target(lhs);
                let rhs = self.expr(rhs, None);
                self.stmt_spanning(Kind::Stmt, vec![lhs, rhs])
            }
            StmtP::If(cond, body) => {
                let mut children = vec![self.expr(cond, None)];
                self.stmt(body, &mut children);
                self.clause_from(Kind::Stmt, s, children)
            }
            StmtP::IfElse(cond, bodies) => {
                // elif is an if statement in the else branch, as in
                // go.starlark.net
                let mut children = vec![self.expr(cond, None)];
                self.stmt(&bodies.0, &mut children);
                self.stmt(&bodies.1, &mut children);
                self.clause_from(Kind::Stmt, s, children)
            }
            StmtP::For(f) => {
                let mut children = vec![self.target(&f.var), self.expr(&f.over, None)];
                self.stmt(&f.body, &mut children);
                self.clause_from(Kind::Stmt, s, children)
            }
            StmtP::Def(def) => {
                let (ns, ne) = offsets(def.name.span);
                let mut children =
                    vec![self.add(Kind::Ident(def.name.node.ident.clone()), ns, ne, ne, vec![])];
                for param in &def.params {
                    children.push(self.param(param));
                }
                self.stmt(&def.body, &mut children);
                self.clause_from(Kind::Stmt, s, children)
            }
            StmtP::Load(load) => self.load(load, s, t),
        };
        out.push(id);
    }

    /// A load statement: go.starlark.net's names are identifiers, an
    /// original name's starting a byte into its string, and a symbol
    /// loaded under its own name is one identifier, walked twice (so its
    /// comments are attached once, to both).
    fn load(&mut self, load: &Load, s: usize, t: usize) -> NodeId {
        let (ms, me) = offsets(load.module.span);
        let module = self.add(Kind::String(load.module.node.clone()), ms, me, me, vec![]);
        let (mut from, mut to, mut symbols) = (Vec::new(), Vec::new(), Vec::new());
        for arg in &load.args {
            let their = &arg.their.node;
            let start = arg.their.span.begin().get() as usize + 1;
            let end = start
                + self.source[start..]
                    .chars()
                    .take(their.chars().count())
                    .map(char::len_utf8)
                    .sum::<usize>();
            let original = self.add(Kind::Ident(their.clone()), start, end, end, vec![]);
            from.push(original);
            let local = &arg.local.node.ident;
            if arg.local.span == arg.their.span {
                to.push(original);
            } else {
                let (ls, le) = offsets(arg.local.span);
                to.push(self.add(Kind::Ident(local.clone()), ls, le, le, vec![]));
            }
            symbols.push((local.clone(), their.clone()));
        }
        let mut children = vec![module];
        children.extend(from);
        children.extend(to);
        let kind = Kind::Load {
            module: load.module.node.clone(),
            symbols,
        };
        self.add(kind, s, t - 1, t, children)
    }

    /// A parameter of a def or a lambda: an identifier, `name = default`,
    /// or `*`, `*args`, `**kwargs` as unary expressions
    fn param(&mut self, param: &AstParameter) -> NodeId {
        let (s, _) = offsets(param.span);
        match &param.node {
            ParameterP::Normal(name, _, default) => {
                let (ns, ne) = offsets(name.span);
                let ident = self.add(Kind::Ident(name.node.ident.clone()), ns, ne, ne, vec![]);
                match default {
                    Some(default) => {
                        let value = self.expr(default, None);
                        self.spanning(Kind::Binary(Op::Eq), vec![ident, value])
                    }
                    None => ident,
                }
            }
            ParameterP::Args(name, _) | ParameterP::KwArgs(name, _) => {
                let (ns, ne) = offsets(name.span);
                let ident = self.add(Kind::Ident(name.node.ident.clone()), ns, ne, ne, vec![]);
                self.from(Kind::Expr, s, vec![ident])
            }
            ParameterP::NoArgs | ParameterP::Slash => self.add(Kind::Expr, s, s + 1, s + 1, vec![]),
        }
    }

    /// An assignment's or a for loop's target
    fn target(&mut self, target: &AstAssignTarget) -> NodeId {
        let (s, t) = offsets(target.span);
        let id = match &target.node {
            AssignTargetP::Identifier(ident) => {
                self.add(Kind::Ident(ident.node.ident.clone()), s, t, t, vec![])
            }
            AssignTargetP::Tuple(items) => {
                let children: Vec<NodeId> = items.iter().map(|x| self.target(x)).collect();
                if self.source.as_bytes().get(s) == Some(&b'[') {
                    self.add(Kind::List, s, t, t, children)
                } else {
                    self.spanning(Kind::Tuple { parens: false }, children)
                }
            }
            AssignTargetP::Index(pair) => {
                let x = self.expr(&pair.0, None);
                let y = self.expr(&pair.1, None);
                let start = self.nodes[x].start;
                self.add(Kind::Expr, start, t - 1, t, vec![x, y])
            }
            AssignTargetP::Dot(x, name) => {
                let x = self.expr(x, None);
                let (ns, ne) = offsets(name.span);
                let name = self.add(Kind::Ident(name.node.clone()), ns, ne, ne, vec![]);
                self.spanning(Kind::Expr, vec![x, name])
            }
        };
        self.parenthesized(id, None)
    }

    /// An expression, within the parentheses written around it
    fn expr(&mut self, expr: &AstExpr, enclosing: Option<(usize, usize)>) -> NodeId {
        let id = self.bare_expr(expr);
        self.parenthesized(id, enclosing)
    }

    /// An expression, without the parentheses written around it
    fn bare_expr(&mut self, expr: &AstExpr) -> NodeId {
        let (s, t) = offsets(expr.span);
        match &expr.node {
            ExprP::Tuple(items) if items.is_empty() => {
                self.add(Kind::Tuple { parens: true }, s, t - 1, t, vec![])
            }
            ExprP::Tuple(items) => {
                let children: Vec<NodeId> = items.iter().map(|x| self.expr(x, None)).collect();
                self.spanning(Kind::Tuple { parens: false }, children)
            }
            ExprP::Dot(x, name) => {
                let x = self.expr(x, None);
                let (ns, ne) = offsets(name.span);
                let name = self.add(Kind::Ident(name.node.clone()), ns, ne, ne, vec![]);
                self.spanning(Kind::Expr, vec![x, name])
            }
            ExprP::Call(func, args) => {
                let func = self.expr(func, None);
                let rparen = t - 1;
                let lparen = self
                    .token_after(self.nodes[func].end)
                    .map_or(rparen, |token| token.0);
                let mut children = vec![func];
                for arg in &args.args {
                    children.push(self.argument(arg, (lparen, rparen)));
                }
                let start = self.nodes[func].start;
                self.add(Kind::Call { rparen }, start, t, t, children)
            }
            ExprP::Index(pair) => {
                let x = self.expr(&pair.0, None);
                let y = self.expr(&pair.1, None);
                let start = self.nodes[x].start;
                self.add(Kind::Expr, start, t - 1, t, vec![x, y])
            }
            ExprP::Index2(triple) => {
                let x = self.expr(&triple.0, None);
                let a = self.expr(&triple.1, None);
                let b = self.expr(&triple.2, None);
                let tuple = self.spanning(Kind::Tuple { parens: false }, vec![a, b]);
                let start = self.nodes[x].start;
                self.add(Kind::Expr, start, t - 1, t, vec![x, tuple])
            }
            ExprP::Slice(x, lo, hi, step) => {
                let mut children = vec![self.expr(x, None)];
                for part in [lo, hi, step].into_iter().flatten() {
                    children.push(self.expr(part, None));
                }
                let start = self.nodes[children[0]].start;
                self.add(Kind::Expr, start, t - 1, t, children)
            }
            ExprP::Identifier(ident) => {
                self.add(Kind::Ident(ident.node.ident.clone()), s, t, t, vec![])
            }
            ExprP::Lambda(lambda) => {
                let mut children: Vec<NodeId> =
                    lambda.params.iter().map(|p| self.param(p)).collect();
                children.push(self.expr(&lambda.body, None));
                self.from(Kind::Expr, s, children)
            }
            ExprP::Literal(AstLiteral::String(string)) => {
                self.add(Kind::String(string.node.clone()), s, t, t, vec![])
            }
            ExprP::Literal(AstLiteral::Int(_)) => {
                self.add(Kind::Int(self.source[s..t].to_string()), s, t, t, vec![])
            }
            ExprP::Literal(_) | ExprP::FString(_) => self.add(Kind::Literal, s, t, t, vec![]),
            ExprP::Not(x) | ExprP::Minus(x) | ExprP::Plus(x) | ExprP::BitNot(x) => {
                let x = self.expr(x, None);
                self.from(Kind::Expr, s, vec![x])
            }
            ExprP::Op(x, op, y) => {
                let x = self.expr(x, None);
                let y = self.expr(y, None);
                let op = match op {
                    BinOp::Add => Op::Plus,
                    _ => Op::Other,
                };
                self.spanning(Kind::Binary(op), vec![x, y])
            }
            ExprP::If(triple) => {
                // walked condition first; spanning from the value if true
                let cond = self.expr(&triple.0, None);
                let yes = self.expr(&triple.1, None);
                let no = self.expr(&triple.2, None);
                let start = self.nodes[yes].start;
                let (go_end, end) = (self.nodes[no].go_end, self.nodes[no].end);
                self.add(Kind::Expr, start, go_end, end, vec![cond, yes, no])
            }
            ExprP::List(items) => {
                let children = items.iter().map(|x| self.expr(x, None)).collect();
                self.add(Kind::List, s, t, t, children)
            }
            ExprP::Dict(entries) => {
                let children = entries
                    .iter()
                    .map(|(k, v)| {
                        let k = self.expr(k, None);
                        let v = self.expr(v, None);
                        self.spanning(Kind::DictEntry, vec![k, v])
                    })
                    .collect();
                self.add(Kind::Dict, s, t, t, children)
            }
            ExprP::ListComprehension(body, first, clauses) => {
                let mut children = vec![self.expr(body, None), self.for_clause(first)];
                for clause in clauses {
                    children.push(self.clause(clause));
                }
                self.add(Kind::Expr, s, t, t, children)
            }
            ExprP::DictComprehension(pair, first, clauses) => {
                let k = self.expr(&pair.0, None);
                let v = self.expr(&pair.1, None);
                let mut children = vec![
                    self.spanning(Kind::DictEntry, vec![k, v]),
                    self.for_clause(first),
                ];
                for clause in clauses {
                    children.push(self.clause(clause));
                }
                self.add(Kind::Expr, s, t, t, children)
            }
        }
    }

    /// A call's argument: a named one is `name = value`, `*x` and `**x`
    /// are unary expressions
    fn argument(&mut self, arg: &AstArgument, parens: (usize, usize)) -> NodeId {
        let (s, _) = offsets(arg.span);
        let id = match &arg.node {
            ArgumentP::Positional(x) => return self.expr(x, Some(parens)),
            ArgumentP::Named(name, x) => {
                let (ns, ne) = offsets(name.span);
                let name = self.add(Kind::Ident(name.node.clone()), ns, ne, ne, vec![]);
                let x = self.expr(x, None);
                self.spanning(Kind::Binary(Op::Eq), vec![name, x])
            }
            ArgumentP::Args(x) | ArgumentP::KwArgs(x) => {
                let x = self.expr(x, None);
                self.from(Kind::Expr, s, vec![x])
            }
        };
        self.parenthesized(id, Some(parens))
    }

    /// A comprehension's `for vars in x` clause
    fn for_clause(&mut self, clause: &ForClause) -> NodeId {
        let vars = self.target(&clause.var);
        let x = self.expr(&clause.over, None);
        let start = self.nodes[vars].start;
        let start = self.last_before(start, Tok::For).unwrap_or(start);
        self.clause_from(Kind::Expr, start, vec![vars, x])
    }

    /// A comprehension's `for` or `if` clause after the first
    fn clause(&mut self, clause: &Clause) -> NodeId {
        match clause {
            ClauseP::For(clause) => self.for_clause(clause),
            ClauseP::If(cond) => {
                let cond = self.expr(cond, None);
                let start = self.nodes[cond].start;
                let start = self.last_before(start, Tok::If).unwrap_or(start);
                self.clause_from(Kind::Expr, start, vec![cond])
            }
        }
    }

    /// Attaches the comments to the nodes, as go.starlark.net's
    /// assignComments does.
    fn attach(&mut self, comments: Vec<Comment>) {
        let (line, mut suffix): (Vec<Comment>, Vec<Comment>) =
            comments.into_iter().partition(|c| self.alone(c.start));
        if line.is_empty() && suffix.is_empty() {
            return;
        }
        let (pre, post) = self.flatten();

        // A comment alone on its line goes to the first node, in pre-order,
        // that doesn't start before it. Those left over go to the file.
        let mut line = line.into_iter().peekable();
        for &id in &pre[1..] {
            while let Some(c) = line.next_if(|c| self.nodes[id].start >= c.start) {
                self.nodes[id].comments.before.push(c);
            }
        }

        // A suffix comment goes to the node that ends before it, the last
        // in post-order.
        for &id in post.iter().rev() {
            if id == ROOT {
                continue;
            }
            if suffix
                .last()
                .is_some_and(|c| self.nodes[id].go_end < c.start)
            {
                let c = suffix.pop().expect("a suffix comment");
                self.nodes[id].comments.suffix.push(c);
            }
        }
    }

    /// Whether the comment at `start` is alone on its line: nothing but
    /// spaces and tabs before it
    fn alone(&self, start: usize) -> bool {
        self.source.as_bytes()[..start]
            .iter()
            .rev()
            .find(|&&b| b != b' ' && b != b'\t')
            .is_none_or(|&b| b == b'\n' || b == b'\r')
    }

    /// The tree's nodes in pre-order and in post-order
    fn flatten(&self) -> (Vec<NodeId>, Vec<NodeId>) {
        let (mut pre, mut post) = (Vec::new(), Vec::new());
        // (node, whether its children were pushed)
        let mut stack = vec![(ROOT, false)];
        while let Some((id, expanded)) = stack.pop() {
            if expanded {
                post.push(id);
                continue;
            }
            pre.push(id);
            stack.push((id, true));
            for &child in self.nodes[id].children.iter().rev() {
                stack.push((child, false));
            }
        }
        (pre, post)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A node as the shared vectors render it: `<kind> <start>-<span end>`,
    /// `/<end>` when its text ends elsewhere, then its comments
    fn render(tree: &Tree, id: NodeId) -> String {
        let n = tree.node(id);
        let kind = match &n.kind {
            Kind::ExprStmt => "ExprStmt".to_string(),
            Kind::Load { .. } => "Load".to_string(),
            Kind::Stmt => "Stmt".to_string(),
            Kind::Ident(name) => format!("Ident({name})"),
            Kind::String(s) => format!("String({s})"),
            Kind::Int(raw) => format!("Int({raw})"),
            Kind::Literal => "Literal".to_string(),
            Kind::List => "List".to_string(),
            Kind::Dict => "Dict".to_string(),
            Kind::DictEntry => "DictEntry".to_string(),
            Kind::Call { .. } => "Call".to_string(),
            Kind::Binary(op) => format!("Binary({op:?})"),
            Kind::Paren => "Paren".to_string(),
            Kind::Tuple { .. } => "Tuple".to_string(),
            Kind::Expr | Kind::File => "Expr".to_string(),
        };
        let mut s = format!("{kind} {}-{}", n.start, n.go_end);
        if n.end != n.go_end {
            s += &format!("/{}", n.end);
        }
        for c in &n.comments.before {
            s += &format!(" <before:{}>", c.text);
        }
        for c in &n.comments.suffix {
            s += &format!(" <suffix:{}>", c.text);
        }
        s
    }

    #[derive(serde::Deserialize)]
    struct Vectors {
        cases: Vec<Case>,
    }

    #[derive(serde::Deserialize)]
    struct Case {
        name: String,
        src: String,
        nodes: Vec<String>,
    }

    /// The tree is go.starlark.net's, on the cases src/go/pkg/starlark
    /// checks against it: its nodes, their spans and their comments.
    /// testdata/ links to the file, which Buck2 maps there.
    #[test]
    fn shared_vectors() {
        let vectors: Vectors =
            serde_json::from_str(include_str!("../testdata/syntax-vectors.json")).unwrap();
        assert!(!vectors.cases.is_empty());
        for case in vectors.cases {
            let tree =
                parse("rules.star", &case.src).unwrap_or_else(|err| panic!("{}: {err}", case.name));
            let nodes: Vec<String> = tree.walk(ROOT)[1..]
                .iter()
                .map(|&id| render(&tree, id))
                .collect();
            assert_eq!(nodes, case.nodes, "{}", case.name);
        }
    }

    /// A syntax error is reported with where it is
    #[test]
    fn syntax_error() {
        let err = parse("rules.star", "go_library(\n").unwrap_err();
        assert!(err.starts_with("rules.star:"), "{err}");
    }
}
