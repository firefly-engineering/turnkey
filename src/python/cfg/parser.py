"""
Parser for Cargo cfg() expressions.

Grammar:
  cfg_expr    = "cfg(" predicate ")"
  predicate   = key | key "=" value | "all(" pred_list ")" | "any(" pred_list ")" | "not(" predicate ")"
  pred_list   = predicate ("," predicate)*
  key         = identifier
  value       = quoted_string

Reference: https://doc.rust-lang.org/cargo/reference/specifying-dependencies.html#platform-specific-dependencies
"""

import re
from dataclasses import dataclass
from typing import Union


@dataclass
class CfgKey:
    """A simple key predicate like 'unix' or 'windows'."""

    key: str


@dataclass
class CfgKeyValue:
    """A key-value predicate like 'target_os = "linux"'."""

    key: str
    value: str


@dataclass
class CfgAll:
    """An all(...) combinator - true if all children are true."""

    children: list["CfgPredicate"]


@dataclass
class CfgAny:
    """An any(...) combinator - true if any child is true."""

    children: list["CfgPredicate"]


@dataclass
class CfgNot:
    """A not(...) combinator - negates the child."""

    child: "CfgPredicate"


CfgPredicate = Union[CfgKey, CfgKeyValue, CfgAll, CfgAny, CfgNot]


class CfgParser:
    """Parser for cfg() expressions."""

    def __init__(self, text: str):
        self.text = text
        self.pos = 0

    def parse(self) -> CfgPredicate | None:
        """Parse a cfg() expression. Returns None if not a valid cfg expression."""
        self.skip_whitespace()

        # Check for cfg( prefix
        if not self.text.lower().startswith("cfg("):
            return None

        self.pos = 4  # Skip "cfg("
        predicate = self.parse_predicate()

        self.skip_whitespace()
        if self.pos < len(self.text) and self.text[self.pos] == ")":
            self.pos += 1

        return predicate

    def parse_predicate(self) -> CfgPredicate | None:
        """Parse a single predicate."""
        self.skip_whitespace()

        if self.pos >= len(self.text):
            return None

        # Check for combinators
        remaining = self.text[self.pos :].lower()

        if remaining.startswith("all("):
            self.pos += 4
            children = self.parse_predicate_list()
            self.expect(")")
            return CfgAll(children)

        if remaining.startswith("any("):
            self.pos += 4
            children = self.parse_predicate_list()
            self.expect(")")
            return CfgAny(children)

        if remaining.startswith("not("):
            self.pos += 4
            child = self.parse_predicate()
            self.expect(")")
            return CfgNot(child) if child else None

        # Parse key or key = value
        return self.parse_key_or_key_value()

    def parse_predicate_list(self) -> list[CfgPredicate]:
        """Parse a comma-separated list of predicates."""
        predicates = []

        while True:
            self.skip_whitespace()
            if self.pos >= len(self.text) or self.text[self.pos] == ")":
                break

            pred = self.parse_predicate()
            if pred:
                predicates.append(pred)

            self.skip_whitespace()
            if self.pos < len(self.text) and self.text[self.pos] == ",":
                self.pos += 1  # Skip comma
            else:
                break

        return predicates

    def parse_key_or_key_value(self) -> CfgPredicate | None:
        """Parse either a key or a key = value pair."""
        self.skip_whitespace()
        key = self.parse_identifier()

        if not key:
            return None

        self.skip_whitespace()

        # Check for = value
        if self.pos < len(self.text) and self.text[self.pos] == "=":
            self.pos += 1  # Skip =
            self.skip_whitespace()
            value = self.parse_string()
            return CfgKeyValue(key, value) if value else CfgKey(key)

        return CfgKey(key)

    def parse_identifier(self) -> str | None:
        """Parse an identifier (alphanumeric + underscores)."""
        self.skip_whitespace()
        match = re.match(r"[a-zA-Z_][a-zA-Z0-9_]*", self.text[self.pos :])
        if match:
            self.pos += match.end()
            return match.group()
        return None

    def parse_string(self) -> str | None:
        """Parse a quoted string."""
        self.skip_whitespace()
        if self.pos >= len(self.text):
            return None

        quote = self.text[self.pos]
        if quote not in ('"', "'"):
            return None

        self.pos += 1  # Skip opening quote
        end = self.text.find(quote, self.pos)
        if end == -1:
            return None

        value = self.text[self.pos : end]
        self.pos = end + 1
        return value

    def skip_whitespace(self):
        """Skip whitespace characters."""
        while self.pos < len(self.text) and self.text[self.pos] in " \t\n\r":
            self.pos += 1

    def expect(self, char: str):
        """Expect and consume a specific character."""
        self.skip_whitespace()
        if self.pos < len(self.text) and self.text[self.pos] == char:
            self.pos += 1
