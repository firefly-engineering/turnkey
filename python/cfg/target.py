"""Target compatibility checking for cfg() expressions."""

from .parser import CfgParser
from .evaluator import TargetSpec, evaluate_cfg


def is_linux_compatible_target(target_spec: str) -> bool:
    """Check if a target specification is compatible with Linux x86_64.

    Uses proper cfg() expression parsing for complex expressions.
    Falls back to string matching for non-cfg expressions.
    """
    target_spec = target_spec.strip()

    # Try parsing as cfg() expression
    parser = CfgParser(target_spec)
    predicate = parser.parse()

    if predicate:
        target = TargetSpec.linux_x86_64()
        return evaluate_cfg(predicate, target)

    # Fallback for non-cfg expressions (e.g., direct target triples)
    target = target_spec.lower()

    # Direct target triple matching
    if "linux" in target or "x86_64-unknown-linux" in target:
        return True

    # Exclude non-Linux targets
    if any(
        os in target
        for os in ["windows", "darwin", "macos", "ios", "android", "wasm", "wasi"]
    ):
        return False

    # Unknown - assume compatible
    return True
