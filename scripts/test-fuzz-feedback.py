#!/usr/bin/env python3
# SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
# SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0
"""Execute real LLVM corpus-feedback qualification with the pinned nightly."""
import importlib.util
import json
import os
import subprocess
import sys
import tomllib
import unittest
from pathlib import Path

spec = importlib.util.spec_from_file_location('fuzz', Path(__file__).with_name('fuzz.py'))
if spec is None or spec.loader is None:
    raise SystemExit('could not load fuzz.py')
FUZZ = importlib.util.module_from_spec(spec)
spec.loader.exec_module(FUZZ)


class CorpusGrowth(unittest.TestCase):
    """Proves coverage feedback actually grows the retained corpus on a real,
    lightweight, non-differential target end to end."""

    def test_corpus_grows_from_new_coverage_on_a_seeded_target(self):
        target = 'patch'
        toolchain = tomllib.loads((FUZZ.ROOT / 'rust-toolchain.toml').read_text())['toolchain']['channel']
        if not toolchain.startswith('nightly-'):
            raise ValueError('corpus-feedback qualification requires the pinned nightly toolchain')
        environment = dict(os.environ, RUSTUP_TOOLCHAIN=toolchain)
        result = subprocess.run(
            [sys.executable, str(Path(__file__).with_name('fuzz.py')),
             '--target', target, '--iterations', '80', '--seed', '424242'],
            cwd=FUZZ.ROOT, env=environment, capture_output=True, text=True, timeout=300,
        )
        self.assertEqual(result.returncode, 0, result.stderr)
        line = next(line for line in result.stdout.splitlines() if line.startswith('Campaign artifacts: '))
        directory = Path(line.removeprefix('Campaign artifacts: '))
        report = json.loads((directory / 'report.json').read_text())
        self.assertEqual(report['status'], 'passed')
        self.assertEqual(report['differential_mismatches'], 0)
        seed_count = len(FUZZ.TARGETS[target]['seeds'])
        self.assertGreater(
            len(report['retained']), seed_count,
            'mutations must retain at least one input beyond the fixed seeds, proving coverage feedback works',
        )
        self.assertTrue(
            any(entry['parent_corpus_index'] is not None for entry in report['retained']),
            'at least one retained input must be a mutation (nonzero parent_corpus_index) rather than a seed',
        )
        self.assertGreater(report['regions'], 0)


if __name__ == '__main__':
    unittest.main()
