"""The platforms a cfg() expression or target triple applies to."""

from .parser import CfgParser
from .evaluator import evaluate_cfg
from .platforms import Platform


def classify_target_platforms(target_spec: str, platforms: list[Platform]) -> set[Platform]:
    """The platforms a [target.'<spec>'] spec holds on.

    The spec is a cfg() expression, evaluated for each platform, or a target
    triple, which holds on the platform whose triple it is.
    """
    target_spec = target_spec.strip()

    predicate = CfgParser(target_spec).parse()
    if predicate:
        return {p for p in platforms if evaluate_cfg(predicate, p.target_spec())}

    return {p for p in platforms if p.target_spec().triple() == target_spec}
