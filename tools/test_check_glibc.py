#!/usr/bin/env python3
"""Tests for check-glibc.py (run: python3 -I tools/test_check_glibc.py)."""
import importlib.util
import unittest
from pathlib import Path

spec = importlib.util.spec_from_file_location("check_glibc", Path(__file__).parent / "check-glibc.py")
check_glibc = importlib.util.module_from_spec(spec)
spec.loader.exec_module(check_glibc)

SAMPLE = """
0000000000000000      DF *UND*  0000000000000000 (GLIBC_2.2.5) memcpy
0000000000000000      DF *UND*  0000000000000000 (GLIBC_2.34)  pthread_create
0000000000000000      DF *UND*  0000000000000000 (GLIBC_2.38)  __isoc23_strtol
0000000000000000      DO *UND*  0000000000000000  GLIBC_2.3.4  __chk_fail
"""


class GlibcTest(unittest.TestCase):
    def test_finds_the_highest_version(self):
        self.assertEqual(check_glibc.max_glibc(SAMPLE), (2, 38))

    def test_compares_numerically_not_as_text(self):
        self.assertGreater(check_glibc.max_glibc("GLIBC_2.10 GLIBC_2.9"), (2, 9))
        self.assertEqual(check_glibc.max_glibc("GLIBC_2.10 GLIBC_2.9"), (2, 10))

    def test_no_versions(self):
        self.assertIsNone(check_glibc.max_glibc("nothing here"))

    def test_three_part_versions(self):
        self.assertEqual(check_glibc.max_glibc("GLIBC_2.2.5"), (2, 2, 5))


if __name__ == "__main__":
    unittest.main()
