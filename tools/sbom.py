#!/usr/bin/env python3
"""CycloneDX SBOM of what actually ships, from `cargo metadata` and Cargo.lock.

Only packages reachable from the app through normal or build dependencies are listed (dev-only
crates never ship). Hashes come from Cargo.lock's registry checksums. Standard library only.

    tools/sbom.py [--package putio-adder] [--out sbom.cdx.json] [--licenses third-party.txt]
"""
import argparse
import json
import subprocess
import sys
import re
from pathlib import Path


def reachable(metadata, root_name):
    """Names+versions of packages the root depends on at build or run time."""
    packages = {p["id"]: p for p in metadata["packages"]}
    nodes = {n["id"]: n for n in metadata["resolve"]["nodes"]}
    root = next(p["id"] for p in metadata["packages"] if p["name"] == root_name)
    seen, stack = {root}, [root]
    while stack:
        node = nodes[stack.pop()]
        for dep in node["deps"]:
            kinds = {k.get("kind") for k in dep["dep_kinds"]}
            if kinds & {None, "build"} and dep["pkg"] not in seen:
                seen.add(dep["pkg"])
                stack.append(dep["pkg"])
    return root, seen, packages, nodes


def parse_lock(text):
    """Packages of a Cargo.lock as dicts with name, version and (for registry crates) checksum.

    Cargo.lock is regular TOML (`[[package]]` tables of `key = "value"` lines). A tiny reader keeps this
    tool independent of the Python version (`tomllib` needs 3.11).
    """
    packages = []
    for block in text.split("[[package]]")[1:]:
        fields = dict(re.findall(r'^(\w+) = "([^"]*)"$', block, flags=re.MULTILINE))
        if "name" in fields and "version" in fields:
            packages.append(fields)
    return packages


def purl(package):
    return f"pkg:cargo/{package['name']}@{package['version']}"


def build(metadata, lock_text, root_name):
    root, seen, packages, nodes = reachable(metadata, root_name)
    checksums = {(p["name"], p["version"]): p.get("checksum") for p in parse_lock(lock_text)}
    components, dependencies = [], []
    for pid in sorted(seen, key=lambda i: (packages[i]["name"], packages[i]["version"])):
        package = packages[pid]
        component = {
            "type": "application" if pid == root else "library",
            "bom-ref": purl(package),
            "name": package["name"],
            "version": package["version"],
            "purl": purl(package),
        }
        if package.get("license"):
            component["licenses"] = [{"expression": package["license"]}]
        checksum = checksums.get((package["name"], package["version"]))
        if checksum:
            component["hashes"] = [{"alg": "SHA-256", "content": checksum}]
        if package.get("source"):
            component["externalReferences"] = [{"type": "distribution", "url": package["source"]}]
        components.append(component)
        refs = sorted(
            purl(packages[d["pkg"]])
            for d in nodes[pid]["deps"]
            if d["pkg"] in seen and {k.get("kind") for k in d["dep_kinds"]} & {None, "build"}
        )
        dependencies.append({"ref": purl(package), "dependsOn": refs})
    top = packages[root]
    return {
        "bomFormat": "CycloneDX",
        "specVersion": "1.5",
        "version": 1,
        "metadata": {
            "component": {
                "type": "application",
                "bom-ref": purl(top),
                "name": top["name"],
                "version": top["version"],
                "purl": purl(top),
            }
        },
        "components": [c for c in components if c["bom-ref"] != purl(top)],
        "dependencies": dependencies,
    }


def licenses_text(bom):
    lines = ["Third-party components compiled into put.io adder", ""]
    for c in bom["components"]:
        expr = c.get("licenses", [{"expression": "UNKNOWN"}])[0]["expression"]
        lines.append(f"{c['name']} {c['version']}: {expr}")
    return "\n".join(lines) + "\n"


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--package", default="putio-adder")
    parser.add_argument("--out", default="sbom.cdx.json")
    parser.add_argument("--licenses")
    args = parser.parse_args()
    root = Path(__file__).resolve().parent.parent
    metadata = json.loads(
        subprocess.run(
            ["cargo", "metadata", "--format-version", "1", "--locked"],
            cwd=root, check=True, capture_output=True, text=True,
        ).stdout
    )
    bom = build(metadata, (root / "Cargo.lock").read_text(), args.package)
    Path(args.out).write_text(json.dumps(bom, indent=2, sort_keys=True) + "\n")
    if args.licenses:
        Path(args.licenses).write_text(licenses_text(bom))
    print(f"{len(bom['components'])} components -> {args.out}", file=sys.stderr)


if __name__ == "__main__":
    main()
