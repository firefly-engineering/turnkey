"""requests and its dependencies, vendored as their locked wheels, hold what
an installer would put in site-packages. No test touches the network."""

import importlib.metadata
import os
import unittest

import certifi
import requests

# requests' version in pylock.toml
LOCKED_REQUESTS = "2.34.2"


class TestRequests(unittest.TestCase):
    def test_imports_from_the_installed_layout(self):
        # requests' sdist keeps it under src/: the wheel installs it at the root
        self.assertEqual(requests.__name__, "requests")
        prepared = requests.Request(
            "GET", "https://example.com/", params={"q": "x"}
        ).prepare()
        self.assertEqual(prepared.url, "https://example.com/?q=x")

    def test_certifi_bundle_is_a_file(self):
        # cacert.pem is a data file, a resource of certifi's library
        self.assertTrue(os.path.isfile(certifi.where()), certifi.where())

    def test_metadata_is_the_locked_version(self):
        # The dist-info is a resource too, read from the vendored wheel.
        # LOCKED_REQUESTS is pylock.toml's version: bump it with the lock.
        version = importlib.metadata.version("requests")
        self.assertEqual(version, LOCKED_REQUESTS)
        self.assertEqual(version, requests.__version__)


if __name__ == "__main__":
    unittest.main()
