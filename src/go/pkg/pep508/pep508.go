// Package pep508 parses Python dependency specifiers (PEP 508) and
// evaluates their environment markers.
//
// It mirrors pydeps-gen's parser (src/cmd/pydeps-gen), which the pydeps
// cell uses; testdata/pep508-vectors.json holds test cases both run.
//
// Reference: https://packaging.python.org/en/latest/specifications/dependency-specifiers/
package pep508

import (
	"fmt"
	"sort"
	"strconv"
	"strings"
)

// Requirement is a parsed dependency specifier.
type Requirement struct {
	// Name is the distribution's name, normalized (PEP 503): lowercase,
	// runs of -, _ and . replaced by -.
	Name string

	// Extras are the extras it asks for, normalized and sorted.
	Extras []string

	// Marker is the environment marker, nil when there is none.
	Marker *Marker
}

// Marker is a parsed environment marker.
type Marker struct {
	// Text is the marker as written.
	Text string

	expr expr
}

// Env is the environment a marker is evaluated in: a value for each marker
// variable (sys_platform, python_version, extra, ...).
type Env map[string]string

// Variables are the marker variables PEP 508 defines.
var Variables = []string{
	"os_name", "sys_platform", "platform_machine", "platform_python_implementation",
	"platform_release", "platform_system", "platform_version", "python_version",
	"python_full_version", "implementation_name", "implementation_version", "extra",
}

// NormalizeName normalizes a distribution or extra name (PEP 503).
func NormalizeName(name string) string {
	var b strings.Builder
	sep := false
	for _, c := range strings.ToLower(name) {
		if c == '-' || c == '_' || c == '.' {
			sep = true
			continue
		}
		if sep && b.Len() > 0 {
			b.WriteByte('-')
		}
		sep = false
		b.WriteRune(c)
	}
	return b.String()
}

// ParseRequirement parses a dependency specifier:
//
//	name [ "[" extra { "," extra } "]" ] [ versionspec | "@" url ] [ ";" marker ]
//
// The version or URL isn't interpreted.
func ParseRequirement(s string) (Requirement, error) {
	var req Requirement
	i := skipSpace(s, 0)
	start := i
	for i < len(s) && isNameChar(s[i]) {
		i++
	}
	if i == start || !isAlnum(s[start]) || !isAlnum(s[i-1]) {
		return req, fmt.Errorf("%q: expected a distribution name", s)
	}
	req.Name = NormalizeName(s[start:i])

	i = skipSpace(s, i)
	if i < len(s) && s[i] == '[' {
		end := strings.IndexByte(s[i:], ']')
		if end < 0 {
			return req, fmt.Errorf("%q: unterminated extras", s)
		}
		for extra := range strings.SplitSeq(s[i+1:i+end], ",") {
			extra = strings.TrimSpace(extra)
			if extra == "" {
				continue
			}
			req.Extras = append(req.Extras, NormalizeName(extra))
		}
		sort.Strings(req.Extras)
		i += end + 1
	}

	// The version spec or URL runs to the marker. A URL may hold ';' only
	// if followed by no whitespace, so a URL's marker follows " ;".
	rest := s[i:]
	isURL := strings.HasPrefix(strings.TrimSpace(rest), "@")
	semi := -1
	for j := 0; j < len(rest); j++ {
		if rest[j] != ';' {
			continue
		}
		if !isURL || (j > 0 && (rest[j-1] == ' ' || rest[j-1] == '\t')) {
			semi = j
			break
		}
	}
	if semi < 0 {
		return req, nil
	}
	marker, err := ParseMarker(strings.TrimSpace(rest[semi+1:]))
	if err != nil {
		return req, fmt.Errorf("%q: %w", s, err)
	}
	req.Marker = marker
	return req, nil
}

// ParseMarker parses an environment marker.
func ParseMarker(s string) (*Marker, error) {
	tokens, err := tokenize(s)
	if err != nil {
		return nil, err
	}
	p := &parser{tokens: tokens}
	e, err := p.or()
	if err != nil {
		return nil, fmt.Errorf("marker %q: %w", s, err)
	}
	if tok := p.peek(); tok.kind != tokEOF {
		return nil, fmt.Errorf("marker %q: unexpected %s", s, tok)
	}
	return &Marker{Text: s, expr: e}, nil
}

// Evaluate reports whether the marker holds in env. A nil marker always
// does.
func (m *Marker) Evaluate(env Env) bool {
	if m == nil {
		return true
	}
	return m.expr.eval(env)
}

// Variables returns the variables the marker reads, sorted.
func (m *Marker) Variables() []string {
	if m == nil {
		return nil
	}
	seen := make(map[string]bool)
	m.expr.variables(seen)
	vars := make([]string, 0, len(seen))
	for v := range seen {
		vars = append(vars, v)
	}
	sort.Strings(vars)
	return vars
}

type expr interface {
	eval(Env) bool
	variables(map[string]bool)
}

type (
	orExpr  struct{ x, y expr }
	andExpr struct{ x, y expr }
	// cmpExpr compares two values; a value is a variable's or a string.
	cmpExpr struct {
		left, right value
		op          string
	}
	value struct {
		variable string // "" for a string
		str      string
	}
)

func (e orExpr) eval(env Env) bool  { return e.x.eval(env) || e.y.eval(env) }
func (e andExpr) eval(env Env) bool { return e.x.eval(env) && e.y.eval(env) }

func (e orExpr) variables(seen map[string]bool) {
	e.x.variables(seen)
	e.y.variables(seen)
}

func (e andExpr) variables(seen map[string]bool) {
	e.x.variables(seen)
	e.y.variables(seen)
}

func (e cmpExpr) variables(seen map[string]bool) {
	for _, v := range []value{e.left, e.right} {
		if v.variable != "" {
			seen[v.variable] = true
		}
	}
}

func (v value) in(env Env) string {
	if v.variable == "" {
		return v.str
	}
	if v.variable == "extra" {
		return NormalizeName(env["extra"])
	}
	return env[v.variable]
}

// eval compares as PEP 508 does: versions as versions (PEP 440) when both
// sides are, and otherwise as strings, for which only ==, != , in and not
// in hold.
func (e cmpExpr) eval(env Env) bool {
	left, right := e.left.in(env), e.right.in(env)
	if e.left.variable == "extra" || e.right.variable == "extra" {
		if e.left.variable == "" {
			left = NormalizeName(left)
		}
		if e.right.variable == "" {
			right = NormalizeName(right)
		}
	}
	switch e.op {
	case "in":
		return strings.Contains(right, left)
	case "not in":
		return !strings.Contains(right, left)
	case "===":
		return left == right
	}
	if result, ok := compareVersions(left, e.op, right); ok {
		return result
	}
	switch e.op {
	case "==":
		return left == right
	case "!=":
		return left != right
	}
	return false
}

// compareVersions evaluates `left op right` on release versions (1, 3.13,
// 3.13.2; right may end with .* for == and !=). It reports false if either
// side isn't one.
func compareVersions(left, op, right string) (bool, bool) {
	a, ok := parseRelease(left)
	if !ok {
		return false, false
	}
	if prefix, wildcard := strings.CutSuffix(right, ".*"); wildcard {
		b, ok := parseRelease(prefix)
		if !ok || (op != "==" && op != "!=") {
			return false, false
		}
		match := len(a) >= len(b) && compareRelease(a[:len(b)], b) == 0
		return match == (op == "=="), true
	}
	b, ok := parseRelease(right)
	if !ok {
		return false, false
	}
	c := compareRelease(a, b)
	switch op {
	case "==":
		return c == 0, true
	case "!=":
		return c != 0, true
	case "<":
		return c < 0, true
	case "<=":
		return c <= 0, true
	case ">":
		return c > 0, true
	case ">=":
		return c >= 0, true
	case "~=":
		if len(b) < 2 {
			return false, true
		}
		return c >= 0 && compareRelease(pad(a, len(b)-1)[:len(b)-1], b[:len(b)-1]) == 0, true
	}
	return false, false
}

func parseRelease(s string) ([]int, bool) {
	if s == "" {
		return nil, false
	}
	var parts []int
	for p := range strings.SplitSeq(s, ".") {
		n, err := strconv.Atoi(p)
		if err != nil || n < 0 {
			return nil, false
		}
		parts = append(parts, n)
	}
	return parts, true
}

func pad(v []int, n int) []int {
	for len(v) < n {
		v = append(v, 0)
	}
	return v
}

func compareRelease(a, b []int) int {
	n := max(len(a), len(b))
	a, b = pad(append([]int(nil), a...), n), pad(append([]int(nil), b...), n)
	for i := range n {
		if a[i] != b[i] {
			if a[i] < b[i] {
				return -1
			}
			return 1
		}
	}
	return 0
}

// parser is a recursive-descent parser over a marker's tokens:
//
//	or   = and { "or" and }
//	and  = atom { "and" atom }
//	atom = "(" or ")" | value op value
type parser struct {
	tokens []token
	pos    int
}

func (p *parser) peek() token { return p.tokens[p.pos] }

func (p *parser) next() token {
	tok := p.tokens[p.pos]
	if tok.kind != tokEOF {
		p.pos++
	}
	return tok
}

func (p *parser) or() (expr, error) {
	x, err := p.and()
	if err != nil {
		return nil, err
	}
	for p.peek().is(tokIdent, "or") {
		p.next()
		y, err := p.and()
		if err != nil {
			return nil, err
		}
		x = orExpr{x, y}
	}
	return x, nil
}

func (p *parser) and() (expr, error) {
	x, err := p.atom()
	if err != nil {
		return nil, err
	}
	for p.peek().is(tokIdent, "and") {
		p.next()
		y, err := p.atom()
		if err != nil {
			return nil, err
		}
		x = andExpr{x, y}
	}
	return x, nil
}

func (p *parser) atom() (expr, error) {
	if p.peek().kind == tokLParen {
		p.next()
		e, err := p.or()
		if err != nil {
			return nil, err
		}
		if tok := p.next(); tok.kind != tokRParen {
			return nil, fmt.Errorf("expected ), got %s", tok)
		}
		return e, nil
	}
	left, err := p.value()
	if err != nil {
		return nil, err
	}
	op, err := p.op()
	if err != nil {
		return nil, err
	}
	right, err := p.value()
	if err != nil {
		return nil, err
	}
	return cmpExpr{left: left, right: right, op: op}, nil
}

func (p *parser) value() (value, error) {
	tok := p.next()
	switch tok.kind {
	case tokString:
		return value{str: tok.text}, nil
	case tokIdent:
		for _, v := range Variables {
			if tok.text == v {
				return value{variable: v}, nil
			}
		}
		return value{}, fmt.Errorf("unknown marker variable %q", tok.text)
	}
	return value{}, fmt.Errorf("expected a variable or a string, got %s", tok)
}

func (p *parser) op() (string, error) {
	tok := p.next()
	switch {
	case tok.kind == tokOp:
		return tok.text, nil
	case tok.is(tokIdent, "in"):
		return "in", nil
	case tok.is(tokIdent, "not"):
		if next := p.next(); next.is(tokIdent, "in") {
			return "not in", nil
		}
		return "", fmt.Errorf("expected in after not")
	}
	return "", fmt.Errorf("expected a comparison, got %s", tok)
}

type tokenKind int

const (
	tokEOF tokenKind = iota
	tokIdent
	tokString
	tokOp
	tokLParen
	tokRParen
)

type token struct {
	kind tokenKind
	text string
}

func (t token) is(kind tokenKind, text string) bool { return t.kind == kind && t.text == text }

func (t token) String() string {
	switch t.kind {
	case tokEOF:
		return "end of marker"
	case tokString:
		return fmt.Sprintf("string %q", t.text)
	case tokLParen:
		return "("
	case tokRParen:
		return ")"
	}
	return fmt.Sprintf("%q", t.text)
}

// tokenize splits a marker into tokens, ending with tokEOF.
func tokenize(s string) ([]token, error) {
	var tokens []token
	for i := 0; i < len(s); {
		c := s[i]
		switch {
		case c == ' ' || c == '\t':
			i++
		case c == '(':
			tokens = append(tokens, token{kind: tokLParen})
			i++
		case c == ')':
			tokens = append(tokens, token{kind: tokRParen})
			i++
		case c == '\'' || c == '"':
			end := strings.IndexByte(s[i+1:], c)
			if end < 0 {
				return nil, fmt.Errorf("marker %q: unterminated string", s)
			}
			tokens = append(tokens, token{tokString, s[i+1 : i+1+end]})
			i += end + 2
		case strings.ContainsRune("<>=!~", rune(c)):
			j := i
			for j < len(s) && strings.ContainsRune("<>=!~", rune(s[j])) {
				j++
			}
			op := s[i:j]
			switch op {
			case "<", "<=", ">", ">=", "==", "!=", "~=", "===":
			default:
				return nil, fmt.Errorf("marker %q: unknown operator %q", s, op)
			}
			tokens = append(tokens, token{tokOp, op})
			i = j
		case isAlnum(c) || c == '_':
			j := i
			for j < len(s) && (isAlnum(s[j]) || s[j] == '_' || s[j] == '.') {
				j++
			}
			tokens = append(tokens, token{tokIdent, s[i:j]})
			i = j
		default:
			return nil, fmt.Errorf("marker %q: unexpected %q", s, string(c))
		}
	}
	return append(tokens, token{kind: tokEOF}), nil
}

func skipSpace(s string, i int) int {
	for i < len(s) && (s[i] == ' ' || s[i] == '\t') {
		i++
	}
	return i
}

func isAlnum(c byte) bool {
	return (c >= 'a' && c <= 'z') || (c >= 'A' && c <= 'Z') || (c >= '0' && c <= '9')
}

func isNameChar(c byte) bool { return isAlnum(c) || c == '-' || c == '_' || c == '.' }

// PlatformEnv returns the marker environment of a platform, in Buck2's
// names (os "linux" or "macos", cpu "x86_64" or "arm64"), for a CPython of
// the given full version (e.g. "3.13.12"), with no extra. Values it can't
// know (platform_release, platform_version) are empty. It reports false for
// a platform it doesn't know.
func PlatformEnv(os, cpu, pythonVersion string) (Env, bool) {
	env := Env{
		"os_name":                        "posix",
		"implementation_name":            "cpython",
		"platform_python_implementation": "CPython",
		"python_full_version":            pythonVersion,
		"implementation_version":         pythonVersion,
		"python_version":                 majorMinor(pythonVersion),
		"platform_release":               "",
		"platform_version":               "",
		"extra":                          "",
	}
	switch os {
	case "linux":
		env["sys_platform"], env["platform_system"] = "linux", "Linux"
		env["platform_machine"] = map[string]string{"x86_64": "x86_64", "arm64": "aarch64"}[cpu]
	case "macos":
		env["sys_platform"], env["platform_system"] = "darwin", "Darwin"
		env["platform_machine"] = map[string]string{"x86_64": "x86_64", "arm64": "arm64"}[cpu]
	default:
		return nil, false
	}
	if env["platform_machine"] == "" {
		return nil, false
	}
	return env, true
}

// PlatformVariables are the marker variables whose values depend on the
// platform, rather than on the Python toolchain or the extra.
var PlatformVariables = []string{"os_name", "sys_platform", "platform_machine", "platform_system", "platform_release", "platform_version"}

// majorMinor returns "3.13" for "3.13.12".
func majorMinor(version string) string {
	parts := strings.SplitN(version, ".", 3)
	if len(parts) < 2 {
		return version
	}
	return parts[0] + "." + parts[1]
}
