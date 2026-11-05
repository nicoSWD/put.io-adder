#!/usr/bin/env python3
"""Fail if a Linux binary needs a newer glibc than the oldest distribution we support.

A binary needs the glibc it was linked against, so building on a new image silently raises the floor.
Usage: tools/check-glibc.py BINARY [--max 2.35]   (needs `objdump`)
"""
import argparse
import re
import subprocess
import sys


def max_glibc(objdump_output):
    """Highest GLIBC_x.y[.z] version referenced in `objdump -T` output, or None."""
    versions = [
        tuple(int(part) for part in match.split("."))
        for match in re.findall(r"GLIBC_(\d+(?:\.\d+)+)", objdump_output)
    ]
    return max(versions) if versions else None


def parse_version(text):
    return tuple(int(part) for part in text.split("."))


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("binary")
    parser.add_argument("--max", default="2.35", help="highest glibc allowed (default: Ubuntu 22.04's 2.35)")
    args = parser.parse_args()
    out = subprocess.run(["objdump", "-T", args.binary], check=True, capture_output=True, text=True).stdout
    found, allowed = max_glibc(out), parse_version(args.max)
    if found is None:
        print(f"{args.binary}: no glibc symbol versions found (static or not Linux?)", file=sys.stderr)
        sys.exit(1)
    label = ".".join(map(str, found))
    if found > allowed:
        print(f"FAIL {args.binary} needs glibc {label}, but the floor is {args.max}", file=sys.stderr)
        sys.exit(1)
    print(f"ok {args.binary} needs glibc {label} (floor {args.max})")


if __name__ == "__main__":
    main()
