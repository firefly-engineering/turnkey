"""
Evaluator for cfg() predicates against target specifications.
"""

from dataclasses import dataclass

from .parser import CfgPredicate, CfgKey, CfgKeyValue, CfgAll, CfgAny, CfgNot


@dataclass
class TargetSpec:
    """A target specification (e.g., x86_64-unknown-linux-gnu)."""

    arch: str
    vendor: str
    os: str
    env: str | None
    family: str

    @classmethod
    def linux_x86_64(cls) -> "TargetSpec":
        """Create a spec for x86_64-unknown-linux-gnu."""
        return cls(
            arch="x86_64",
            vendor="unknown",
            os="linux",
            env="gnu",
            family="unix",
        )

    @classmethod
    def macos_aarch64(cls) -> "TargetSpec":
        """Create a spec for aarch64-apple-darwin."""
        return cls(
            arch="aarch64",
            vendor="apple",
            os="macos",
            env=None,
            family="unix",
        )


def evaluate_cfg(predicate: CfgPredicate, target: TargetSpec) -> bool:
    """Evaluate a cfg predicate against a target specification."""
    if isinstance(predicate, CfgKey):
        # Handle shorthand keys
        key = predicate.key.lower()
        if key == "unix":
            return target.family == "unix"
        if key == "windows":
            return target.family == "windows"
        # Unknown standalone keys (like miri, test, doc) are not set during
        # normal compilation, so default to False
        return False

    if isinstance(predicate, CfgKeyValue):
        key = predicate.key.lower()
        value = predicate.value.lower()

        if key == "target_os":
            return target.os.lower() == value
        if key == "target_arch":
            return target.arch.lower() == value
        if key == "target_family":
            return target.family.lower() == value
        if key == "target_vendor":
            return target.vendor.lower() == value
        if key == "target_env":
            return (target.env or "").lower() == value
        if key == "target_pointer_width":
            return (value == "64" and target.arch in ("x86_64", "aarch64")) or (
                value == "32" and target.arch in ("x86", "arm")
            )
        if key == "target_endian":
            # Most common architectures are little-endian
            return value == "little"
        if key == "feature":
            # Features are handled separately
            return True

        # Unknown key-value pairs (like getrandom_backend = "custom") should
        # default to False since these cfg keys are not set unless explicitly
        # configured. Previously we defaulted to True which caused issues with
        # crates like getrandom that use custom cfg keys to exclude dependencies.
        return False

    if isinstance(predicate, CfgAll):
        return all(evaluate_cfg(child, target) for child in predicate.children)

    if isinstance(predicate, CfgAny):
        # Empty any() is false
        if not predicate.children:
            return False
        return any(evaluate_cfg(child, target) for child in predicate.children)

    if isinstance(predicate, CfgNot):
        return not evaluate_cfg(predicate.child, target)

    return True
