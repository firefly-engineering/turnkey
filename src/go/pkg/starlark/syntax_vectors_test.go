package starlark

import (
	_ "embed"
	"encoding/json"
	"fmt"
	"reflect"
	"testing"

	"go.starlark.net/syntax"
)

// The syntax trees go.starlark.net builds for a corpus of sources. The
// rules-star crate (src/rust/rules-star), which rebuilds them from another
// parser, runs the same cases.
//
//go:embed testdata/syntax-vectors.json
var syntaxVectorsJSON []byte

func TestSyntaxVectors(t *testing.T) {
	var doc struct {
		Cases []struct {
			Name  string   `json:"name"`
			Src   string   `json:"src"`
			Nodes []string `json:"nodes"`
		} `json:"cases"`
	}
	if err := json.Unmarshal(syntaxVectorsJSON, &doc); err != nil {
		t.Fatal(err)
	}
	for _, c := range doc.Cases {
		got, err := dumpTree([]byte(c.Src))
		if err != nil {
			t.Errorf("%s: %v", c.Name, err)
			continue
		}
		if !reflect.DeepEqual(got, c.Nodes) {
			t.Errorf("%s: nodes\n%q\nwant\n%q", c.Name, got, c.Nodes)
		}
	}
}

// dumpTree renders the nodes of a source's tree below the file, in
// pre-order, each as "<kind> <start>-<span end>", with "/<end>" when its
// text ends elsewhere (nodeEnd), then its comments: " <before:#...>" for
// each comment attached before it, " <suffix:#...>" for each after it.
// Offsets are bytes.
func dumpTree(src []byte) ([]string, error) {
	f, err := syntax.Parse("rules.star", src, syntax.RetainComments)
	if err != nil {
		return nil, err
	}
	var nodes []string
	syntax.Walk(f, func(n syntax.Node) bool {
		if n == nil || n == syntax.Node(f) {
			return true
		}
		start, end := n.Span()
		spanEnd := positionToOffset(end, src)
		s := fmt.Sprintf("%s %d-%d", nodeKind(n), positionToOffset(start, src), spanEnd)
		if textEnd := positionToOffset(nodeEnd(n), src); textEnd != spanEnd {
			s += fmt.Sprintf("/%d", textEnd)
		}
		if c := n.Comments(); c != nil {
			for _, x := range c.Before {
				s += " <before:" + x.Text + ">"
			}
			for _, x := range c.Suffix {
				s += " <suffix:" + x.Text + ">"
			}
		}
		nodes = append(nodes, s)
		return true
	})
	return nodes, nil
}

// nodeKind names what a node is, as far as rules sync tells them apart.
func nodeKind(n syntax.Node) string {
	switch n := n.(type) {
	case *syntax.ExprStmt:
		return "ExprStmt"
	case *syntax.LoadStmt:
		return "Load"
	case syntax.Stmt:
		return "Stmt"
	case *syntax.Ident:
		return "Ident(" + n.Name + ")"
	case *syntax.Literal:
		switch n.Token {
		case syntax.STRING:
			return "String(" + n.Value.(string) + ")"
		case syntax.INT:
			return "Int(" + n.Raw + ")"
		}
		return "Literal"
	case *syntax.ListExpr:
		return "List"
	case *syntax.DictExpr:
		return "Dict"
	case *syntax.DictEntry:
		return "DictEntry"
	case *syntax.CallExpr:
		return "Call"
	case *syntax.ParenExpr:
		return "Paren"
	case *syntax.TupleExpr:
		return "Tuple"
	case *syntax.BinaryExpr:
		switch n.Op {
		case syntax.EQ:
			return "Binary(Eq)"
		case syntax.PLUS:
			return "Binary(Plus)"
		}
		return "Binary(Other)"
	}
	return "Expr"
}
