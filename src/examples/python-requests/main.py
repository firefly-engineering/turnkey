#!/usr/bin/env python3
"""Example using requests from the pydeps cell, without the network.

requests keeps its code under src/ in its sdist, and its dependency certifi
ships its CA bundle as a data file. Both are vendored as their locked wheels
(docs/adr/0013-pydeps-distributions-are-their-locked-wheels.md), so
`import requests` works and the bundle is there.
"""

import importlib.metadata

import certifi
import requests


def main() -> None:
    request = requests.Request("GET", "https://example.com/", params={"q": "turnkey"})
    prepared = request.prepare()
    print(f"requests {importlib.metadata.version('requests')}")
    print(f"prepared {prepared.method} {prepared.url}")
    print(f"CA bundle: {certifi.where()}")


if __name__ == "__main__":
    main()
