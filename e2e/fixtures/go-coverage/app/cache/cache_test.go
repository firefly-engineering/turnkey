package cache

import "testing"

func TestSharedEvictsOldest(t *testing.T) {
	c, err := Shared(2)
	if err != nil {
		t.Fatal(err)
	}
	c.Add("a", "1")
	c.Add("b", "2")
	c.Add("c", "3")
	if c.Contains("a") {
		t.Error("a should have been evicted")
	}
	if v, ok := c.Get("c"); !ok || v != "3" {
		t.Errorf("Get(c) = %q, %v; want 3, true", v, ok)
	}
}

func TestLocalEvictsOldest(t *testing.T) {
	c, err := Local(1)
	if err != nil {
		t.Fatal(err)
	}
	c.Add("a", "1")
	c.Add("b", "2")
	if c.Len() != 1 || !c.Contains("b") {
		t.Errorf("want only b, have %v", c.Keys())
	}
}
