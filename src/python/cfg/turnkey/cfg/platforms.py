"""The platforms turnkey builds for, and the select() keys that tell them apart.

This mirrors src/go/pkg/conditions, which rules sync uses: a platform is an
(os, cpu) pair in Buck2's constraint names, and values that differ between
platforms are written as a select() keyed on the smallest exact key --
config//os:<os> when they differ only by OS (config//cpu:<cpu> by CPU
alone), otherwise the combined config_setting <settings>:<os>-<cpu>. Every
platform gets a branch, and there is never a DEFAULT, so building for a
platform that isn't listed fails instead of silently missing values.
tests/test_split_vectors.py runs the Go module's test cases against split.

The platforms come from turnkey's buck2.platforms option, as JSON:
{"settings": "toolchains//conditions", "platforms": [{"os": ..., "cpu": ...}]}.
"""

import json
from dataclasses import dataclass
from itertools import combinations
from typing import Callable, Hashable, TypeVar

from .evaluator import TargetSpec

T = TypeVar("T")
H = TypeVar("H", bound=Hashable)

DIMENSIONS = ("os", "cpu")


@dataclass(frozen=True)
class Platform:
    """A platform, named by the values of its Buck2 os and cpu constraints."""

    os: str
    cpu: str

    @property
    def name(self) -> str:
        return f"{self.os}-{self.cpu}"

    def target_spec(self) -> TargetSpec:
        return TargetSpec.from_platform(self.os, self.cpu)


@dataclass(frozen=True)
class Platforms:
    """The platforms turnkey builds for, and the package of their combined config_settings."""

    platforms: tuple[Platform, ...]
    settings: str

    @classmethod
    def from_json(cls, text: str) -> "Platforms":
        doc = json.loads(text)
        platforms = tuple(dict.fromkeys(Platform(p["os"], p["cpu"]) for p in doc["platforms"]))
        if not platforms:
            raise ValueError("no platforms")
        return cls(platforms=platforms, settings=doc["settings"])

    def __iter__(self):
        return iter(self.platforms)

    def key(self, dims: tuple[str, ...], platform: Platform) -> str:
        """The select() key matching platform's values of dims."""
        if dims == ("os",):
            return f"config//os:{platform.os}"
        if dims == ("cpu",):
            return f"config//cpu:{platform.cpu}"
        return f"{self.settings}:" + "-".join(getattr(platform, d) for d in dims)

    def branches(self, values: Callable[[Platform], T]) -> dict[str, T] | None:
        """Key each platform's value on the smallest exact key.

        Returns None when every platform has the same value: no select() is
        needed. Values are compared with ==.
        """
        chosen = self._choose(values)
        if chosen is None:
            return None
        _, keyed = chosen
        return keyed

    def split(self, items: Callable[[Platform], list[H]]) -> tuple[list[H], dict[str, list[H]]]:
        """Split each platform's items into the common ones and select() branches.

        Returns the items every platform has (in the first platform's order)
        and, when platforms differ, the extra items per key (in the order of
        the first platform the key matches); the branches are empty when
        they don't differ.
        """
        per = {p: list(dict.fromkeys(items(p))) for p in self.platforms}
        first = per[self.platforms[0]]
        common = [i for i in first if all(i in per[p] for p in self.platforms)]
        extra = {p: [i for i in per[p] if i not in common] for p in self.platforms}
        chosen = self._choose(lambda p: frozenset(extra[p]))
        if chosen is None:
            return common, {}
        dims, _ = chosen
        result: dict[str, list[H]] = {}
        for p in self.platforms:
            result.setdefault(self.key(dims, p), extra[p])
        return common, dict(sorted(result.items()))

    def _choose(self, values: Callable[[Platform], T]) -> tuple[tuple[str, ...], dict[str, T]] | None:
        """The fewest dimensions whose values determine each platform's value.

        Returns them with the value per key, sorted by key, or None when
        every platform has the same value.
        """
        first = values(self.platforms[0])
        if all(values(p) == first for p in self.platforms[1:]):
            return None
        for size in range(1, len(DIMENSIONS) + 1):
            for dims in combinations(DIMENSIONS, size):
                keyed: dict[str, T] = {}
                if all(
                    keyed.setdefault(self.key(dims, p), values(p)) == values(p)
                    for p in self.platforms
                ):
                    return dims, dict(sorted(keyed.items()))
        raise AssertionError("os and cpu together tell every platform apart")
