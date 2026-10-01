//! Editing a parsed file: each edit records what it changed, for
//! [`File::write`](crate::File::write) to rewrite only that

use crate::{Attribute, DepsValue, File, SelectBranch, SelectValue, Span, Target, Value, render};
use std::collections::{HashMap, HashSet};

impl Target {
    /// Sets the deps attribute (see [`Target::set_labels`]).
    pub fn set_deps(&mut self, deps: Vec<String>) {
        self.set_labels("deps", deps);
    }

    /// Sets a label-list attribute (deps, npm_deps, ...). If it has
    /// markers, only its auto-managed section is set.
    pub fn set_labels(&mut self, name: &str, labels: Vec<String>) {
        if let Some(attr) = self.attributes.get(name)
            && let Value::Deps(deps) = &attr.value
            && deps.has_markers
        {
            if deps.auto_deps == labels {
                return;
            }
            let value = Value::Deps(DepsValue {
                auto_deps: labels,
                preserved_deps: deps.preserved_deps.clone(),
                has_markers: true,
                raw_deps: Vec::new(),
            });
            self.replace(name, value);
            return;
        }
        self.set_string_list(name, labels);
    }

    /// Sets a label-list attribute to `[<common>] + select({<key>:
    /// [<labels>], ...})`, with `branches`' keys and values as given, or to
    /// the plain list `common` when there are no branches. As with
    /// [`Target::set_labels`], the markers of the attribute's plain part are
    /// kept: `common` becomes its auto-managed section.
    pub fn set_select(&mut self, name: &str, common: Vec<String>, branches: Vec<SelectBranch>) {
        let mut plain = Value::StringList(common.clone());
        if let Some(attr) = self.attributes.get(name) {
            let prev = match &attr.value {
                Value::Select(sel) => sel.common.as_deref(),
                value => Some(value),
            };
            if let Some(Value::Deps(deps)) = prev
                && deps.has_markers
            {
                plain = Value::Deps(DepsValue {
                    auto_deps: common,
                    preserved_deps: deps.preserved_deps.clone(),
                    has_markers: true,
                    raw_deps: Vec::new(),
                });
            }
        }
        if branches.is_empty() {
            self.set_value(name, plain);
            return;
        }
        let common = match plain {
            Value::StringList(values) if values.is_empty() => None,
            plain => Some(Box::new(plain)),
        };
        self.set_value(name, Value::Select(SelectValue { common, branches }));
    }

    /// Sets an attribute's value, creating the attribute if needed. A value
    /// written the same as the current one is no change.
    fn set_value(&mut self, name: &str, value: Value) {
        if let Some(attr) = self.attributes.get(name)
            && render(&attr.value) == render(&value)
        {
            return;
        }
        if !self.attributes.contains_key(name) {
            self.new_attribute(name, value.clone());
        }
        self.replace(name, value);
    }

    /// Adds a dep, unless the target has it.
    pub fn add_dep(&mut self, dep: &str) {
        let mut deps = self.get_deps();
        if deps.iter().any(|d| d == dep) {
            return;
        }
        deps.push(dep.to_string());
        self.set_deps(deps);
    }

    /// Removes a dep.
    pub fn remove_dep(&mut self, dep: &str) {
        let current = self.get_deps();
        let deps: Vec<String> = current.iter().filter(|d| *d != dep).cloned().collect();
        if deps.len() != current.len() {
            self.set_deps(deps);
        }
    }

    /// Sets a list-of-strings attribute.
    fn set_string_list(&mut self, name: &str, values: Vec<String>) {
        match self.attributes.get(name).map(|a| &a.value) {
            Some(Value::StringList(current)) if *current == values => {}
            _ => self.set(name, Value::StringList(values)),
        }
    }

    /// Sets a string attribute.
    pub fn set_string(&mut self, name: &str, value: &str) {
        match self.attributes.get(name).map(|a| &a.value) {
            Some(Value::String(current)) if current == value => {}
            _ => self.set(name, Value::String(value.to_string())),
        }
    }

    /// Sets a boolean attribute.
    pub fn set_bool(&mut self, name: &str, value: bool) {
        match self.attributes.get(name).map(|a| &a.value) {
            Some(Value::Bool(current)) if *current == value => {}
            _ => self.set(name, Value::Bool(value)),
        }
    }

    /// Sets an integer attribute.
    pub fn set_int(&mut self, name: &str, value: i64) {
        match self.attributes.get(name).map(|a| &a.value) {
            Some(Value::Int(current)) if *current == value => {}
            _ => self.set(name, Value::Int(value)),
        }
    }

    /// Sets an attribute that changed, creating it if needed.
    fn set(&mut self, name: &str, value: Value) {
        if !self.attributes.contains_key(name) {
            self.new_attribute(name, value.clone());
        }
        self.replace(name, value);
    }

    /// Adds a new attribute, last.
    fn new_attribute(&mut self, name: &str, value: Value) {
        self.attributes.insert(
            name.to_string(),
            Attribute {
                name: name.to_string(),
                value,
                syntax: None,
                original: None,
                modified: true,
            },
        );
        self.attribute_order.push(name.to_string());
    }

    /// Replaces an existing attribute's value, recording the change.
    fn replace(&mut self, name: &str, value: Value) {
        let attr = self
            .attributes
            .get_mut(name)
            .expect("an existing attribute");
        attr.value = value;
        attr.modified = true;
        self.modified = true;
        self.modified_attrs.insert(name.to_string());
    }

    /// Removes an attribute.
    pub fn remove_attribute(&mut self, name: &str) {
        if self.attributes.remove(name).is_none() {
            return;
        }
        self.attribute_order.retain(|n| n != name);
        self.modified = true;
        self.modified_attrs.insert(name.to_string());
    }

    /// Whether the attribute was changed
    pub fn is_attribute_modified(&self, name: &str) -> bool {
        self.modified_attrs.contains(name)
    }

    /// Sorts the deps.
    pub fn sort_deps(&mut self) {
        let deps = self.get_deps();
        if deps.len() <= 1 {
            return;
        }
        let mut sorted = deps.clone();
        sorted.sort();
        if sorted != deps {
            self.set_deps(sorted);
        }
    }
}

impl File {
    /// Adds a new target, with its name, and returns it.
    pub fn add_target(&mut self, rule: &str, name: &str) -> &mut Target {
        let mut attributes = HashMap::new();
        attributes.insert(
            "name".to_string(),
            Attribute {
                name: "name".to_string(),
                value: Value::String(name.to_string()),
                syntax: None,
                original: None,
                modified: true,
            },
        );
        self.targets.push(Target {
            rule: rule.to_string(),
            name: name.to_string(),
            no_sync: false,
            attributes,
            attribute_order: vec!["name".to_string()],
            call: None,
            span: Span::default(),
            modified: true,
            modified_attrs: HashSet::new(),
        });
        self.modified = true;
        self.targets.last_mut().expect("the target just added")
    }

    /// Removes the target named `name`, reporting whether there was one.
    pub fn remove_target(&mut self, name: &str) -> bool {
        match self.targets.iter().position(|t| t.name == name) {
            Some(i) => {
                self.targets.remove(i);
                self.modified = true;
                true
            }
            None => false,
        }
    }
}
