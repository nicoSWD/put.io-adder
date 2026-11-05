#!/usr/bin/env python3
"""Tests for sbom.py (run: python3 -I tools/test_sbom.py)."""
import sys
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent))
import sbom  # noqa: E402

LOCK = '''
[[package]]
name = "app"
version = "1.0.0"
[[package]]
name = "lib"
version = "0.2.0"
source = "registry+https://github.com/rust-lang/crates.io-index"
checksum = "aa11"
[[package]]
name = "devonly"
version = "3.0.0"
source = "registry+https://github.com/rust-lang/crates.io-index"
checksum = "bb22"
[[package]]
name = "buildtool"
version = "0.1.0"
source = "registry+https://github.com/rust-lang/crates.io-index"
checksum = "cc33"
'''


def package(name, version, license_=None, source=None):
    return {"id": f"{name} {version}", "name": name, "version": version, "license": license_, "source": source}


METADATA = {
    "packages": [
        package("app", "1.0.0"),
        package("lib", "0.2.0", "MIT OR Apache-2.0", "registry+https://github.com/rust-lang/crates.io-index"),
        package("devonly", "3.0.0", "MIT"),
        package("buildtool", "0.1.0", "ISC"),
    ],
    "resolve": {
        "nodes": [
            {"id": "app 1.0.0", "deps": [
                {"pkg": "lib 0.2.0", "dep_kinds": [{"kind": None}]},
                {"pkg": "devonly 3.0.0", "dep_kinds": [{"kind": "dev"}]},
                {"pkg": "buildtool 0.1.0", "dep_kinds": [{"kind": "build"}]},
            ]},
            {"id": "lib 0.2.0", "deps": []},
            {"id": "devonly 3.0.0", "deps": []},
            {"id": "buildtool 0.1.0", "deps": []},
        ]
    },
}


class LockTest(unittest.TestCase):
    def test_reads_names_versions_and_checksums(self):
        packages = sbom.parse_lock(LOCK)
        self.assertEqual([p["name"] for p in packages], ["app", "lib", "devonly", "buildtool"])
        self.assertNotIn("checksum", packages[0])
        self.assertEqual(packages[1]["checksum"], "aa11")

    def test_ignores_dependency_lists(self):
        text = '[[package]]\nname = "a"\nversion = "1.0.0"\ndependencies = [\n "b",\n]\n'
        self.assertEqual(sbom.parse_lock(text), [{"name": "a", "version": "1.0.0"}])

    def test_real_lockfile_parses(self):
        lock = (Path(__file__).parent.parent / "Cargo.lock").read_text()
        packages = sbom.parse_lock(lock)
        self.assertGreater(len(packages), 100)
        self.assertTrue(all(p.get("checksum") for p in packages if p.get("source", "").startswith("registry")))


class SbomTest(unittest.TestCase):
    def setUp(self):
        self.bom = sbom.build(METADATA, LOCK, "app")

    def test_dev_only_crates_are_excluded(self):
        names = {c["name"] for c in self.bom["components"]}
        self.assertEqual(names, {"lib", "buildtool"})

    def test_root_is_the_application(self):
        self.assertEqual(self.bom["metadata"]["component"]["name"], "app")
        self.assertEqual(self.bom["bomFormat"], "CycloneDX")

    def test_hashes_come_from_the_lockfile(self):
        lib = next(c for c in self.bom["components"] if c["name"] == "lib")
        self.assertEqual(lib["hashes"], [{"alg": "SHA-256", "content": "aa11"}])
        self.assertEqual(lib["licenses"], [{"expression": "MIT OR Apache-2.0"}])

    def test_dependency_edges_skip_dev_dependencies(self):
        edges = {d["ref"]: d["dependsOn"] for d in self.bom["dependencies"]}
        self.assertEqual(edges["pkg:cargo/app@1.0.0"], ["pkg:cargo/buildtool@0.1.0", "pkg:cargo/lib@0.2.0"])

    def test_licence_listing(self):
        text = sbom.licenses_text(self.bom)
        self.assertIn("lib 0.2.0: MIT OR Apache-2.0", text)
        self.assertIn("buildtool 0.1.0: ISC", text)
        self.assertNotIn("devonly", text)


if __name__ == "__main__":
    unittest.main()
