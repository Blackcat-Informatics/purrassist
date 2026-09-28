#!/usr/bin/env python3
# SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
# SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0
"""Verify coverage novelty boundaries and fail-closed campaign behavior."""
import importlib.util
from pathlib import Path
import subprocess
import tempfile
import unittest
from unittest.mock import patch

SPEC = importlib.util.spec_from_file_location('fuzz', Path(__file__).with_name('fuzz-json.py'))
FUZZ = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(FUZZ)


class Coverage(unittest.TestCase):
    def document(self):
        return {'type': 'llvm.coverage.json.export', 'version': '3.1.0', 'data': [{'functions': [{
            'filenames': [str(FUZZ.ROOT / 'crates/core/src/json/parser.rs'),
                          str(FUZZ.ROOT / 'crates/core/examples/fuzz_json.rs'),
                          '/rustc/stdlib.rs'],
            'regions': [[1, 1, 2, 1, 1, 0, 0, 0], [1, 1, 2, 1, 20, 0, 0, 0],
                        [3, 1, 4, 1, 0, 0, 0, 0], [5, 1, 6, 1, 1, 0, 0, 3],
                        [7, 1, 8, 1, 1, 1, 0, 0], [9, 1, 10, 1, 1, 2, 0, 0]],
        }]}]}

    def test_only_hit_core_code_regions_supply_novelty(self):
        self.assertEqual(FUZZ.covered_regions(self.document()),
                         {('crates/core/src/json/parser.rs', 1, 1, 2, 1)})

    def test_bad_export_cannot_be_reported_as_no_new_coverage(self):
        for document in ({}, {'type': 'llvm.coverage.json.export', 'version': '3.1.0', 'data': []}):
            with self.assertRaises(ValueError):
                FUZZ.covered_regions(document)
        for region in ([1] * 7, [1, 1, 2, 1, 1, 3, 0, 0], [1, 1, 2, 1, -1, 0, 0, 0]):
            document = self.document()
            document['data'][0]['functions'][0]['regions'] = [region]
            with self.assertRaises(ValueError):
                FUZZ.covered_regions(document)

    def test_timeout_preserves_input_and_discards_stale_profile(self):
        with tempfile.TemporaryDirectory() as name:
            directory = Path(name)
            (directory / 'current.profraw').write_bytes(b'stale')
            (directory / 'current.profdata').write_bytes(b'stale')
            with patch.object(FUZZ.subprocess, 'run', side_effect=subprocess.TimeoutExpired('probe', 1)):
                with self.assertRaises(subprocess.TimeoutExpired):
                    FUZZ.probe(Path('unused-probe'), b'broken input', directory, 1)
            self.assertEqual((directory / 'current.bin').read_bytes(), b'broken input')
            self.assertFalse((directory / 'current.profraw').exists())
            self.assertFalse((directory / 'current.profdata').exists())

    def test_probe_failure_cannot_supply_coverage(self):
        for result in (subprocess.CompletedProcess([], 1, b'ok\n', b'panic'),
                       subprocess.CompletedProcess([], 0, b'wrong\n', b''),
                       subprocess.CompletedProcess([], 0, b'ok\n', b'warning')):
            with tempfile.TemporaryDirectory() as name:
                with patch.object(FUZZ.subprocess, 'run', return_value=result) as run:
                    with self.assertRaises((ValueError, subprocess.CalledProcessError)):
                        FUZZ.probe(Path('unused-probe'), b'null', Path(name), 1)
                    self.assertEqual(run.call_count, 1)

    def test_mutations_reproduce_and_respect_probe_bound(self):
        left, right = FUZZ.Random(123), FUZZ.Random(123)
        for seed in FUZZ.SEEDS + [b'x' * 4096]:
            for _ in range(100):
                mutated = FUZZ.mutate(seed, left)
                self.assertEqual(mutated, FUZZ.mutate(seed, right))
                self.assertLessEqual(len(mutated), 4096)


if __name__ == '__main__':
    unittest.main()
