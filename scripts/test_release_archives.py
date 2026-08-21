#!/usr/bin/env python3
"""Negative regressions for release archive validation."""

import importlib.util
from pathlib import Path
import tarfile
import tempfile
import unittest
import zipfile


def load(name: str, filename: str):
    spec = importlib.util.spec_from_file_location(name, Path(__file__).with_name(filename))
    assert spec is not None and spec.loader is not None
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


tar_tools = load("reproducible_tar", "reproducible_tar.py")
zip_tools = load("reproducible_zip", "reproducible_zip.py")


class ArchiveValidationTests(unittest.TestCase):
    def test_tar_rejects_parent_traversal(self):
        with tempfile.TemporaryDirectory() as directory:
            archive = Path(directory) / "bad.tar.gz"
            with tarfile.open(archive, "w:gz") as output:
                output.addfile(tarfile.TarInfo("../escape"))
            with self.assertRaises(SystemExit):
                tar_tools.validate(archive, "arandu-0.0.1", "x86_64-unknown-linux-gnu", "0.0.1")

    def test_zip_rejects_case_insensitive_duplicate(self):
        with tempfile.TemporaryDirectory() as directory:
            archive = Path(directory) / "bad.zip"
            with zipfile.ZipFile(archive, "w") as output:
                output.writestr("arandu-0.0.1/bin/arandu.exe", b"one")
                output.writestr("arandu-0.0.1/BIN/ARANDU.EXE", b"two")
            with self.assertRaises(SystemExit):
                zip_tools.validate(archive, "arandu-0.0.1", "0.0.1", "x86_64-pc-windows-msvc")

    def test_zip_rejects_parent_traversal(self):
        with tempfile.TemporaryDirectory() as directory:
            archive = Path(directory) / "bad.zip"
            with zipfile.ZipFile(archive, "w") as output:
                output.writestr("arandu-0.0.1/../../escape", b"bad")
            with self.assertRaises(SystemExit):
                zip_tools.validate(archive, "arandu-0.0.1", "0.0.1", "x86_64-pc-windows-msvc")


if __name__ == "__main__":
    unittest.main()
