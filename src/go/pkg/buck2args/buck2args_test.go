package buck2args

import "testing"

func TestSubcommand(t *testing.T) {
	for _, tc := range []struct {
		args      []string
		want      string
		wantIndex int
	}{
		{[]string{"test", "//..."}, "test", 0},
		{[]string{"--isolation-dir", "x", "test", "//..."}, "test", 2},
		{[]string{"--isolation-dir=x", "build"}, "build", 1},
		{[]string{"-v", "2", "--oncall", "me", "run", "//:bin"}, "run", 4},
		{[]string{"-v=2", "--client-metadata", "k=v", "test"}, "test", 3},
		{[]string{"--setting", "a.b=c", "--agent-context", "intent=build", "targets"}, "targets", 4},
		{[]string{"--help"}, "", -1},
		{[]string{"--isolation-dir", "test"}, "", -1},
		{nil, "", -1},
	} {
		name, index := Subcommand(tc.args)
		if name != tc.want || index != tc.wantIndex {
			t.Errorf("Subcommand(%q) = %q, %d; want %q, %d", tc.args, name, index, tc.want, tc.wantIndex)
		}
	}
}
