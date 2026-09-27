package conditions

import (
	"reflect"
	"strings"
	"testing"
)

// turnkey's default platforms
var platforms = []Platform{
	{OS: "linux", CPU: "x86_64"},
	{OS: "linux", CPU: "arm64"},
	{OS: "macos", CPU: "x86_64"},
	{OS: "macos", CPU: "arm64"},
}

// labelsBy returns a labels function giving each configuration the common
// labels plus those of the first matching entry of extra, keyed by
// "os=...", "cpu=..." or "cpu=...,os=...".
func labelsBy(common []string, extra map[string][]string) func(Configuration) []string {
	return func(c Configuration) []string {
		labels := append([]string(nil), common...)
		for _, key := range []string{c.String(), "os=" + c[OS], "cpu=" + c[CPU]} {
			if e, ok := extra[key]; ok {
				return append(labels, e...)
			}
		}
		return labels
	}
}

func TestSplitIdenticalEverywhereIsAPlainList(t *testing.T) {
	space := NewSpace(platforms, "")
	got := space.Split(labelsBy([]string{"//a:a", "//b:b"}, nil))
	want := Split{Common: []string{"//a:a", "//b:b"}}
	if !reflect.DeepEqual(got, want) {
		t.Errorf("split = %+v, want %+v", got, want)
	}
	if got.IsConditional() {
		t.Error("identical deps need no select()")
	}
}

func TestSplitOSOnlyDifferencesUseOSKeys(t *testing.T) {
	space := NewSpace(platforms, "")
	got := space.Split(labelsBy([]string{"//unix:unix"}, map[string][]string{
		"os=linux": {"//linux:only"},
		"os=macos": {"//macos:only"},
	}))
	want := Split{
		Common: []string{"//unix:unix"},
		Branches: []Branch{
			{Key: "config//os:linux", Labels: []string{"//linux:only"}},
			{Key: "config//os:macos", Labels: []string{"//macos:only"}},
		},
	}
	if !reflect.DeepEqual(got, want) {
		t.Errorf("split = %+v, want %+v", got, want)
	}
}

// An OS with no extra deps still gets its branch: there is no DEFAULT.
func TestSplitEmptyBranchForOtherOS(t *testing.T) {
	space := NewSpace(platforms, "")
	got := space.Split(labelsBy(nil, map[string][]string{"os=linux": {"//linux:only"}}))
	want := []Branch{
		{Key: "config//os:linux", Labels: []string{"//linux:only"}},
		{Key: "config//os:macos"},
	}
	if !reflect.DeepEqual(got.Branches, want) {
		t.Errorf("branches = %+v, want %+v", got.Branches, want)
	}
}

func TestSplitCPUDifferenceWithinOneOSUsesCombinedKey(t *testing.T) {
	space := NewSpace(platforms, "")
	got := space.Split(labelsBy(nil, map[string][]string{"cpu=arm64,os=linux": {"//linux-arm:only"}}))
	want := []Branch{
		{Key: "toolchains//conditions:linux-arm64", Labels: []string{"//linux-arm:only"}},
		{Key: "toolchains//conditions:linux-x86_64"},
		{Key: "toolchains//conditions:macos-arm64"},
		{Key: "toolchains//conditions:macos-x86_64"},
	}
	if !reflect.DeepEqual(got.Branches, want) {
		t.Errorf("branches = %+v, want %+v", got.Branches, want)
	}
}

func TestSplitCPUOnlyDifferencesUseCPUKeys(t *testing.T) {
	space := NewSpace(platforms, "")
	got := space.Split(labelsBy(nil, map[string][]string{"cpu=x86_64": {"//x86:only"}}))
	want := []Branch{
		{Key: "config//cpu:arm64"},
		{Key: "config//cpu:x86_64", Labels: []string{"//x86:only"}},
	}
	if !reflect.DeepEqual(got.Branches, want) {
		t.Errorf("branches = %+v, want %+v", got.Branches, want)
	}
}

func TestSplitNeverWritesDefault(t *testing.T) {
	space := NewSpace(platforms, "")
	cases := []map[string][]string{
		{"os=linux": {"//l:l"}},
		{"cpu=arm64": {"//a:a"}},
		{"cpu=arm64,os=macos": {"//m:m"}},
		{"os=linux": {"//l:l"}, "os=macos": {"//m:m"}},
	}
	for _, extra := range cases {
		for _, b := range space.Split(labelsBy([]string{"//c:c"}, extra)).Branches {
			if b.Key == DefaultKey || strings.Contains(b.Key, "DEFAULT") {
				t.Errorf("split of %v wrote a DEFAULT branch", extra)
			}
		}
	}
}

// Each platform's value round-trips: reading back what Split wrote gives
// every configuration the labels it had.
func TestReaderRoundTrip(t *testing.T) {
	space := NewSpace(platforms, "")
	labels := labelsBy([]string{"//c:c"}, map[string][]string{
		"cpu=arm64,os=linux": {"//la:la"},
		"os=macos":           {"//m:m"},
	})
	split := space.Split(labels)
	ev, err := space.Reader(split.Common, split.Branches)
	if err != nil {
		t.Fatal(err)
	}
	for _, config := range space.Configurations {
		if got, want := ev.Labels(config), labels(config); !sameSet(got, want) {
			t.Errorf("%s: labels = %v, want %v", config, got, want)
		}
	}
}

func TestReaderDefaultAndUnknownKeys(t *testing.T) {
	space := NewSpace(platforms, "")
	ev, err := space.Reader(nil, []Branch{
		{Key: "config//os:linux", Labels: []string{"//l:l"}},
		{Key: DefaultKey, Labels: []string{"//d:d"}},
	})
	if err != nil {
		t.Fatal(err)
	}
	if got := ev.Labels(Configuration{OS: "macos", CPU: "arm64"}); !reflect.DeepEqual(got, []string{"//d:d"}) {
		t.Errorf("macos labels = %v, want DEFAULT's", got)
	}
	if got := ev.Labels(Configuration{OS: "linux", CPU: "arm64"}); !reflect.DeepEqual(got, []string{"//l:l"}) {
		t.Errorf("linux labels = %v, want linux's", got)
	}

	if _, err := space.Reader(nil, []Branch{{Key: "//my:setting"}}); err == nil {
		t.Error("an unknown key was read, want an error")
	}
}

// The combined key wins over an OS key when both match.
func TestReaderPrefersTheMostSpecificKey(t *testing.T) {
	space := NewSpace(platforms, "")
	ev, err := space.Reader(nil, []Branch{
		{Key: "config//os:linux", Labels: []string{"//l:l"}},
		{Key: "toolchains//conditions:linux-arm64", Labels: []string{"//la:la"}},
	})
	if err != nil {
		t.Fatal(err)
	}
	if got := ev.Labels(Configuration{OS: "linux", CPU: "arm64"}); !reflect.DeepEqual(got, []string{"//la:la"}) {
		t.Errorf("labels = %v, want the combined key's", got)
	}
}

// With no platforms there is one configuration, and deps can't differ.
func TestSpaceWithoutPlatforms(t *testing.T) {
	space := NewSpace(nil, "")
	if len(space.Configurations) != 1 || len(space.Configurations[0]) != 0 {
		t.Fatalf("configurations = %v, want one empty", space.Configurations)
	}
	if got := space.Split(labelsBy([]string{"//a:a"}, nil)); got.IsConditional() {
		t.Errorf("split = %+v, want a plain list", got)
	}
}
