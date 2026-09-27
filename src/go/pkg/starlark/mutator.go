package starlark

import (
	"sort"
)

// SetDeps sets the deps attribute of the target.
// If the deps attribute has markers, it only updates the auto-managed section.
func (t *Target) SetDeps(deps []string) {
	t.SetLabels("deps", deps)
}

// SetLabels sets a label-list attribute (deps, npm_deps, ...) of the target.
// If the attribute has markers, it only updates the auto-managed section.
func (t *Target) SetLabels(name string, deps []string) {
	attr := t.GetAttribute(name)
	if attr == nil {
		// No existing attribute, create as simple list
		t.setStringList(name, deps)
		return
	}

	// Check if existing deps have markers
	if depsVal, ok := attr.Value.(DepsValue); ok {
		// Preserve the preserved section, update only auto section
		if depsVal.HasMarkers {
			// Check if auto deps changed
			if stringSlicesEqual(depsVal.AutoDeps, deps) {
				return // No change
			}

			newDepsVal := DepsValue{
				AutoDeps:      deps,
				PreservedDeps: depsVal.PreservedDeps,
				HasMarkers:    true,
			}
			attr.Value = newDepsVal
			attr.modified = true
			t.modified = true
			t.modifiedAttrs[name] = true
			return
		}
	}

	// No markers or not a DepsValue, use simple list
	t.setStringList(name, deps)
}

// SetSelect sets a label-list attribute to
// [<common>] + select({<key>: [<labels>], ...}), with branches' keys and
// values as given, or to the plain list common when there are no branches.
// As with SetLabels, the markers of the attribute's plain part are kept:
// common becomes its auto-managed section.
func (t *Target) SetSelect(name string, common []string, branches []SelectBranch) {
	var plain AttributeValue = StringListValue{Values: common}
	if attr := t.GetAttribute(name); attr != nil {
		prev := attr.Value
		if sel, ok := prev.(SelectValue); ok {
			prev = sel.Common
		}
		if deps, ok := prev.(DepsValue); ok && deps.HasMarkers {
			plain = DepsValue{AutoDeps: common, PreservedDeps: deps.PreservedDeps, HasMarkers: true}
		}
	}
	if len(branches) == 0 {
		t.setValue(name, plain)
		return
	}
	if list, ok := plain.(StringListValue); ok && len(list.Values) == 0 {
		plain = nil
	}
	t.setValue(name, SelectValue{Common: plain, Branches: branches})
}

// setValue sets an attribute's value, creating the attribute if needed. A
// value written the same as the current one is no change.
func (t *Target) setValue(name string, value AttributeValue) {
	attr := t.GetAttribute(name)
	if attr == nil {
		attr = &Attribute{Name: name}
		t.Attributes[name] = attr
		t.AttributeOrder = append(t.AttributeOrder, name)
	} else if attr.Value != nil && renderValue(attr.Value, "") == renderValue(value, "") {
		return
	}
	attr.Value = value
	attr.modified = true
	t.modified = true
	t.modifiedAttrs[name] = true
}

// AddDep adds a dependency to the target.
func (t *Target) AddDep(dep string) {
	current := t.GetDeps()
	for _, d := range current {
		if d == dep {
			return // Already present
		}
	}
	t.SetDeps(append(current, dep))
}

// RemoveDep removes a dependency from the target.
func (t *Target) RemoveDep(dep string) {
	current := t.GetDeps()
	var newDeps []string
	for _, d := range current {
		if d != dep {
			newDeps = append(newDeps, d)
		}
	}
	if len(newDeps) != len(current) {
		t.SetDeps(newDeps)
	}
}

// SetStringList sets a string list attribute.
func (t *Target) setStringList(name string, values []string) {
	attr := t.GetAttribute(name)
	if attr == nil {
		// Create new attribute
		attr = &Attribute{
			Name:     name,
			modified: true,
		}
		t.Attributes[name] = attr
		t.AttributeOrder = append(t.AttributeOrder, name)
	}

	// Check if values changed
	if list, ok := attr.Value.(StringListValue); ok {
		if stringSlicesEqual(list.Values, values) {
			return // No change
		}
	}

	attr.Value = StringListValue{Values: values}
	attr.modified = true
	t.modified = true
	t.modifiedAttrs[name] = true
}

// SetString sets a string attribute.
func (t *Target) SetString(name string, value string) {
	attr := t.GetAttribute(name)
	if attr == nil {
		// Create new attribute
		attr = &Attribute{
			Name:     name,
			modified: true,
		}
		t.Attributes[name] = attr
		t.AttributeOrder = append(t.AttributeOrder, name)
	}

	// Check if value changed
	if str, ok := attr.Value.(StringValue); ok {
		if str.Value == value {
			return // No change
		}
	}

	attr.Value = StringValue{Value: value}
	attr.modified = true
	t.modified = true
	t.modifiedAttrs[name] = true
}

// SetBool sets a boolean attribute.
func (t *Target) SetBool(name string, value bool) {
	attr := t.GetAttribute(name)
	if attr == nil {
		attr = &Attribute{
			Name:     name,
			modified: true,
		}
		t.Attributes[name] = attr
		t.AttributeOrder = append(t.AttributeOrder, name)
	}

	if b, ok := attr.Value.(BoolValue); ok {
		if b.Value == value {
			return
		}
	}

	attr.Value = BoolValue{Value: value}
	attr.modified = true
	t.modified = true
	t.modifiedAttrs[name] = true
}

// SetInt sets an integer attribute.
func (t *Target) SetInt(name string, value int64) {
	attr := t.GetAttribute(name)
	if attr == nil {
		attr = &Attribute{
			Name:     name,
			modified: true,
		}
		t.Attributes[name] = attr
		t.AttributeOrder = append(t.AttributeOrder, name)
	}

	if i, ok := attr.Value.(IntValue); ok {
		if i.Value == value {
			return
		}
	}

	attr.Value = IntValue{Value: value}
	attr.modified = true
	t.modified = true
	t.modifiedAttrs[name] = true
}

// RemoveAttribute removes an attribute from the target.
func (t *Target) RemoveAttribute(name string) {
	if _, exists := t.Attributes[name]; !exists {
		return
	}

	delete(t.Attributes, name)

	// Remove from order
	var newOrder []string
	for _, n := range t.AttributeOrder {
		if n != name {
			newOrder = append(newOrder, n)
		}
	}
	t.AttributeOrder = newOrder

	t.modified = true
	t.modifiedAttrs[name] = true
}

// IsAttributeModified returns true if the specified attribute was modified.
func (t *Target) IsAttributeModified(name string) bool {
	return t.modifiedAttrs[name]
}

// SortDeps sorts the deps list alphabetically.
func (t *Target) SortDeps() {
	deps := t.GetDeps()
	if len(deps) <= 1 {
		return
	}

	sorted := make([]string, len(deps))
	copy(sorted, deps)
	sort.Strings(sorted)

	if !stringSlicesEqual(deps, sorted) {
		t.SetDeps(sorted)
	}
}

// AddTarget adds a new target to the file.
func (f *File) AddTarget(rule, name string) *Target {
	t := &Target{
		Rule:          rule,
		Name:          name,
		Attributes:    make(map[string]*Attribute),
		modifiedAttrs: make(map[string]bool),
		modified:      true,
	}

	// Add name attribute
	t.Attributes["name"] = &Attribute{
		Name:     "name",
		Value:    StringValue{Value: name},
		modified: true,
	}
	t.AttributeOrder = []string{"name"}

	f.Targets = append(f.Targets, t)
	f.modified = true

	return t
}

// RemoveTarget removes a target from the file.
func (f *File) RemoveTarget(name string) bool {
	for i, t := range f.Targets {
		if t.Name == name {
			f.Targets = append(f.Targets[:i], f.Targets[i+1:]...)
			f.modified = true
			return true
		}
	}
	return false
}

// stringSlicesEqual compares two string slices for equality.
func stringSlicesEqual(a, b []string) bool {
	if len(a) != len(b) {
		return false
	}
	for i := range a {
		if a[i] != b[i] {
			return false
		}
	}
	return true
}
