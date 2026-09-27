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
from .platforms import Platform, Platforms
from .target import classify_target_platforms

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
    "Platform",
    "Platforms",
    "classify_target_platforms",
]
