"""Target compatibility checking for cfg() expressions."""

from .parser import CfgParser
from .evaluator import TargetSpec, evaluate_cfg


# The platforms we support and their Buck2 config_setting targets.
SUPPORTED_PLATFORMS = {
    "config//os:linux": TargetSpec.linux_x86_64(),
    "config//os:macos": TargetSpec.macos_aarch64(),
}


def classify_target_platforms(target_spec: str) -> set[str]:
    """Classify which supported platforms a target specification matches.

    Returns a set of Buck2 config_setting keys (e.g., {"config//os:linux"})
    that the target_spec is compatible with.
    """
    target_spec = target_spec.strip()

    # Try parsing as cfg() expression
    parser = CfgParser(target_spec)
    predicate = parser.parse()

    if predicate:
        return {
            platform_key
            for platform_key, spec in SUPPORTED_PLATFORMS.items()
            if evaluate_cfg(predicate, spec)
        }

    # Fallback for non-cfg expressions (e.g., direct target triples)
    target = target_spec.lower()

    # Direct target triple matching
    if "linux" in target or "x86_64-unknown-linux" in target:
        return {"config//os:linux"}

    if "darwin" in target or "macos" in target or "apple" in target:
        return {"config//os:macos"}

    # Exclude known non-supported targets
    if any(
        os in target
        for os in ["windows", "ios", "android", "wasm", "wasi"]
    ):
        return set()

    # Unknown - assume compatible with all
    return set(SUPPORTED_PLATFORMS.keys())


def is_linux_compatible_target(target_spec: str) -> bool:
    """Check if a target specification is compatible with Linux x86_64.

    Kept for backward compatibility. Prefer classify_target_platforms().
    """
    return "config//os:linux" in classify_target_platforms(target_spec)
