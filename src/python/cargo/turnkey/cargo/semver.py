"""Cargo version requirement matching.

Implements the requirement syntax Cargo accepts in a dependency's `version`
(https://doc.rust-lang.org/cargo/reference/specifying-dependencies.html):
caret (the default), tilde, wildcard, `=`, comparisons, and comma-joined
comparators. It is used to pick which vendored version of a crate a
dependency resolves to when several are vendored.
"""

from dataclasses import dataclass

# Longest operators first, so ">=" is not read as ">".
_OPERATORS = (">=", "<=", "=", ">", "<", "^", "~")
_WILDCARDS = ("*", "x", "X")

# Sort key of a pre-release-less release; any pre-release of the same
# major.minor.patch sorts below it.
_RELEASE = (1,)
# Sorts below every pre-release: an exclusive upper bound of X.Y.Z built with
# it also excludes X.Y.Z's pre-releases, as Cargo does.
_LOWEST_PRE = (0,)


@dataclass(frozen=True)
class _Partial:
    """A version as written in a requirement: trailing parts may be absent."""

    major: int | None
    minor: int | None
    patch: int | None
    pre: tuple[str, ...]


def _pre_key(pre: tuple[str, ...]) -> tuple:
    if not pre:
        return _RELEASE
    # Numeric identifiers sort numerically and below alphanumeric ones.
    return (0, *((0, int(p), "") if p.isdigit() else (1, 0, p) for p in pre))


def _parse(text: str) -> _Partial:
    text = text.strip().split("+", 1)[0]
    core, _, pre = text.partition("-")
    parts: list[int | None] = []
    for part in core.split("."):
        part = part.strip()
        if not part or part in _WILDCARDS or (parts and parts[-1] is None):
            parts.append(None)
        else:
            parts.append(int(part))
    parts += [None] * (3 - len(parts))
    return _Partial(parts[0], parts[1], parts[2], tuple(pre.split(".")) if pre else ())


def _version_key(version: str) -> tuple:
    v = _parse(version)
    return (v.major or 0, v.minor or 0, v.patch or 0, _pre_key(v.pre))


def _floor(p: _Partial) -> tuple:
    return (p.major or 0, p.minor or 0, p.patch or 0, _pre_key(p.pre))


def _bump(p: _Partial, part: str) -> tuple:
    """The first version past `p` at the precision of `part`."""
    if part == "major":
        return (p.major + 1, 0, 0, _LOWEST_PRE)
    if part == "minor":
        return (p.major, p.minor + 1, 0, _LOWEST_PRE)
    return (p.major, p.minor, p.patch + 1, _LOWEST_PRE)


def _precision(p: _Partial) -> str:
    """The last part `p` spells out."""
    if p.minor is None:
        return "major"
    if p.patch is None:
        return "minor"
    return "patch"


@dataclass(frozen=True)
class _Comparator:
    """Bounds on a version key; None means unbounded on that side."""

    low: tuple | None
    low_inclusive: bool
    high: tuple | None
    high_inclusive: bool
    partial: _Partial

    def admits(self, key: tuple) -> bool:
        if self.low is not None:
            if key < self.low or (key == self.low and not self.low_inclusive):
                return False
        if self.high is not None:
            if key > self.high or (key == self.high and not self.high_inclusive):
                return False
        return True


def _comparator(text: str) -> _Comparator:
    text = text.strip()
    op = next((o for o in _OPERATORS if text.startswith(o)), None)
    if op is not None:
        text = text[len(op) :]
    p = _parse(text)
    if op is None:
        # A bare version is a caret requirement, but a wildcard one ("1.2.*")
        # pins the parts it spells out.
        wildcard = p.major is None or p.minor is None or p.patch is None
        op = "=" if wildcard and any(w in text for w in _WILDCARDS) else "^"

    if p.major is None:
        # "*": any version.
        return _Comparator(None, True, None, True, p)

    floor = _floor(p)
    full = p.patch is not None
    if op == "^":
        if p.major > 0 or p.minor is None:
            high = _bump(p, "major")
        elif p.minor > 0 or p.patch is None:
            high = _bump(p, "minor")
        else:
            high = _bump(p, "patch")
        return _Comparator(floor, True, high, False, p)
    if op == "~":
        high = _bump(p, "major" if p.minor is None else "minor")
        return _Comparator(floor, True, high, False, p)
    if op == "=":
        if full:
            return _Comparator(floor, True, floor, True, p)
        return _Comparator(floor, True, _bump(p, _precision(p)), False, p)
    if op == ">=":
        return _Comparator(floor, True, None, True, p)
    if op == ">":
        if full:
            return _Comparator(floor, False, None, True, p)
        return _Comparator(_bump(p, _precision(p)), True, None, True, p)
    if op == "<":
        high = floor if p.pre else floor[:3] + (_LOWEST_PRE,)
        return _Comparator(None, True, high, False, p)
    # "<="
    if full:
        return _Comparator(None, True, floor, True, p)
    return _Comparator(None, True, _bump(p, _precision(p)), False, p)


def matches(req: str, version: str) -> bool:
    """Whether `version` satisfies the Cargo requirement `req`."""
    comparators = [_comparator(c) for c in req.split(",") if c.strip()]
    key = _version_key(version)
    if not all(c.admits(key) for c in comparators):
        return False
    v = _parse(version)
    if v.pre:
        # A pre-release only matches a requirement that names a pre-release
        # of the same major.minor.patch.
        return any(
            c.partial.pre and _floor(c.partial)[:3] == key[:3] for c in comparators
        )
    return True


def best_match(req: str | None, versions) -> str | None:
    """The highest of `versions` satisfying `req` (any version if None)."""
    candidates = [v for v in versions if req is None or matches(req, v)]
    if not candidates:
        return None
    return max(candidates, key=_version_key)
