#!/usr/bin/env python3
"""Simple Python app demonstrating external package usage in Buck2.

Uses the `six` package for Python 2/3 compatibility utilities.
Dependencies are managed via python-deps.toml and the pydeps cell.
"""

import six
import sys


def main() -> None:
    py_version = "Python 3" if six.PY3 else "Python 2"
    print(f"Python: Hello from {py_version}!")
    print(f"six version: {six.__version__}")


if __name__ == "__main__":
    main()
