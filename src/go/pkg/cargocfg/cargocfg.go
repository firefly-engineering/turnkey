// Package cargocfg evaluates the platform specs of Cargo's
// [target.'<spec>'.dependencies] tables, a cfg() expression or a target
// triple, for the platforms turnkey builds for.
//
// It mirrors turnkey.cfg (src/python/cfg), which the rustdeps cell uses for
// vendored crates; testdata/cfg-vectors.json holds test cases both run.
//
// Reference: https://doc.rust-lang.org/reference/conditional-compilation.html
package cargocfg

import (
	"fmt"
	"strings"

	"github.com/firefly-engineering/turnkey/src/go/pkg/conditions"
)

// Target is what a cfg() expression can ask about a platform.
type Target struct {
	// Triple is the platform's Rust target triple.
	Triple string

	Arch, Vendor, OS, Env, Family string

	// PointerWidth is the pointer width in bits, e.g. "64".
	PointerWidth string
}

// ForPlatform returns the Rust target of a platform, in Buck2's names. It
// reports false for an OS or CPU it doesn't know.
func ForPlatform(p conditions.Platform) (Target, bool) {
	var t Target
	switch p.CPU {
	case "x86_64":
		t.Arch = "x86_64"
	case "arm64":
		t.Arch = "aarch64"
	default:
		return Target{}, false
	}
	t.PointerWidth = "64"
	t.Family = "unix"
	switch p.OS {
	case "linux":
		t.OS, t.Vendor, t.Env = "linux", "unknown", "gnu"
		t.Triple = t.Arch + "-unknown-linux-gnu"
	case "macos":
		t.OS, t.Vendor = "macos", "apple"
		t.Triple = t.Arch + "-apple-darwin"
	default:
		return Target{}, false
	}
	return t, true
}

// Spec is a parsed platform spec.
type Spec struct {
	// triple is set for a target triple, pred for a cfg() expression.
	triple string
	pred   predicate
}

// Parse parses a platform spec: cfg(<predicate>) or a target triple.
func Parse(spec string) (Spec, error) {
	spec = strings.TrimSpace(spec)
	if !strings.HasPrefix(spec, "cfg(") {
		if spec == "" || strings.ContainsAny(spec, " ()=,\"") {
			return Spec{}, fmt.Errorf("%q is neither cfg(...) nor a target triple", spec)
		}
		return Spec{triple: spec}, nil
	}
	p := &parser{tokens: tokenize(spec)}
	if err := p.expectIdent("cfg"); err != nil {
		return Spec{}, err
	}
	if err := p.expect(tokLParen); err != nil {
		return Spec{}, err
	}
	pred, err := p.predicate()
	if err != nil {
		return Spec{}, err
	}
	if err := p.expect(tokRParen); err != nil {
		return Spec{}, err
	}
	if tok := p.next(); tok.kind != tokEOF {
		return Spec{}, fmt.Errorf("%s: unexpected %s after the cfg()", spec, tok)
	}
	return Spec{pred: pred}, nil
}

// Matches reports whether the spec holds for target.
func (s Spec) Matches(target Target) bool {
	if s.pred == nil {
		return s.triple == target.Triple
	}
	return s.pred.eval(target)
}

// predicate is a node of a cfg() expression.
type predicate interface {
	eval(Target) bool
}

type (
	// allPred is all(...): true if every child is, so all() is true.
	allPred []predicate
	// anyPred is any(...): true if one child is, so any() is false.
	anyPred []predicate
	// notPred is not(...).
	notPred struct{ child predicate }
	// keyPred is a bare key, e.g. unix.
	keyPred string
	// keyValuePred is key = "value", e.g. target_os = "linux".
	keyValuePred struct{ key, value string }
)

func (p allPred) eval(t Target) bool {
	for _, c := range p {
		if !c.eval(t) {
			return false
		}
	}
	return true
}

func (p anyPred) eval(t Target) bool {
	for _, c := range p {
		if c.eval(t) {
			return true
		}
	}
	return false
}

func (p notPred) eval(t Target) bool { return !p.child.eval(t) }

// A bare key other than a target family (test, miri, debug_assertions, a
// custom --cfg) isn't set when a dependency is built.
func (p keyPred) eval(t Target) bool {
	switch strings.ToLower(string(p)) {
	case "unix", "windows":
		return strings.EqualFold(string(p), t.Family)
	}
	return false
}

// A key Cargo doesn't set for the target (a custom cfg such as
// getrandom_backend) is false.
func (p keyValuePred) eval(t Target) bool {
	value := strings.ToLower(p.value)
	switch strings.ToLower(p.key) {
	case "target_os":
		return value == t.OS
	case "target_arch":
		return value == t.Arch
	case "target_family":
		return value == t.Family
	case "target_vendor":
		return value == t.Vendor
	case "target_env":
		return value == t.Env
	case "target_pointer_width":
		return value == t.PointerWidth
	case "target_endian":
		// Every platform turnkey knows is little-endian
		return value == "little"
	}
	return false
}

// parser is a recursive-descent parser over a spec's tokens:
//
//	predicate = ident "(" [ predicate { "," predicate } [ "," ] ] ")"   (all, any, not)
//	          | ident [ "=" string ]
type parser struct {
	tokens []token
	pos    int
}

func (p *parser) next() token {
	tok := p.tokens[p.pos]
	if tok.kind != tokEOF {
		p.pos++
	}
	return tok
}

func (p *parser) peek() token { return p.tokens[p.pos] }

func (p *parser) expect(kind tokenKind) error {
	if tok := p.next(); tok.kind != kind {
		return fmt.Errorf("expected %s, got %s", kind, tok)
	}
	return nil
}

func (p *parser) expectIdent(name string) error {
	if tok := p.next(); tok.kind != tokIdent || tok.text != name {
		return fmt.Errorf("expected %s, got %s", name, tok)
	}
	return nil
}

func (p *parser) predicate() (predicate, error) {
	tok := p.next()
	if tok.kind != tokIdent {
		return nil, fmt.Errorf("expected a cfg predicate, got %s", tok)
	}
	switch p.peek().kind {
	case tokLParen:
		p.next()
		children, err := p.list()
		if err != nil {
			return nil, err
		}
		switch tok.text {
		case "all":
			return allPred(children), nil
		case "any":
			return anyPred(children), nil
		case "not":
			if len(children) != 1 {
				return nil, fmt.Errorf("not() takes one predicate, got %d", len(children))
			}
			return notPred{children[0]}, nil
		}
		return nil, fmt.Errorf("unknown cfg operator %s()", tok.text)
	case tokEq:
		p.next()
		value := p.next()
		if value.kind != tokString {
			return nil, fmt.Errorf("expected a string after %s =, got %s", tok.text, value)
		}
		return keyValuePred{tok.text, value.text}, nil
	}
	return keyPred(tok.text), nil
}

// list parses predicates up to and including the closing parenthesis.
func (p *parser) list() ([]predicate, error) {
	var preds []predicate
	for p.peek().kind != tokRParen {
		pred, err := p.predicate()
		if err != nil {
			return nil, err
		}
		preds = append(preds, pred)
		if p.peek().kind != tokComma {
			break
		}
		p.next()
	}
	return preds, p.expect(tokRParen)
}

type tokenKind int

const (
	tokEOF tokenKind = iota
	tokIdent
	tokString
	tokLParen
	tokRParen
	tokComma
	tokEq
	tokInvalid
)

func (k tokenKind) String() string {
	return [...]string{"end of input", "identifier", "string", "(", ")", ",", "=", "invalid character"}[k]
}

type token struct {
	kind tokenKind
	text string
}

func (t token) String() string {
	switch t.kind {
	case tokIdent, tokInvalid:
		return fmt.Sprintf("%s %q", t.kind, t.text)
	case tokString:
		return fmt.Sprintf("string %q", t.text)
	}
	return t.kind.String()
}

// tokenize splits a spec into tokens, ending with tokEOF. An unterminated
// string or a character no token starts with becomes tokInvalid.
func tokenize(s string) []token {
	var tokens []token
	for i := 0; i < len(s); {
		c := s[i]
		switch {
		case c == ' ' || c == '\t' || c == '\n' || c == '\r':
			i++
		case c == '(':
			tokens = append(tokens, token{kind: tokLParen})
			i++
		case c == ')':
			tokens = append(tokens, token{kind: tokRParen})
			i++
		case c == ',':
			tokens = append(tokens, token{kind: tokComma})
			i++
		case c == '=':
			tokens = append(tokens, token{kind: tokEq})
			i++
		case c == '"':
			end := strings.IndexByte(s[i+1:], '"')
			if end < 0 {
				return append(tokens, token{tokInvalid, s[i:]}, token{kind: tokEOF})
			}
			tokens = append(tokens, token{tokString, s[i+1 : i+1+end]})
			i += end + 2
		case isIdentStart(c):
			j := i + 1
			for j < len(s) && (isIdentStart(s[j]) || (s[j] >= '0' && s[j] <= '9')) {
				j++
			}
			tokens = append(tokens, token{tokIdent, s[i:j]})
			i = j
		default:
			return append(tokens, token{tokInvalid, string(c)}, token{kind: tokEOF})
		}
	}
	return append(tokens, token{kind: tokEOF})
}

func isIdentStart(c byte) bool {
	return c == '_' || (c >= 'a' && c <= 'z') || (c >= 'A' && c <= 'Z')
}
