"""Cargo cfg() expression parsing and evaluation library."""

from .parser import (
    CfgParser,
    CfgKey,
    CfgKeyValue,
    CfgAll,
    CfgAny,
    CfgNot,
    CfgPredicate,
)
from .evaluator import TargetSpec, evaluate_cfg
from .target import is_linux_compatible_target

__all__ = [
    "CfgParser",
    "CfgKey",
    "CfgKeyValue",
    "CfgAll",
    "CfgAny",
    "CfgNot",
    "CfgPredicate",
    "TargetSpec",
    "evaluate_cfg",
    "is_linux_compatible_target",
]
