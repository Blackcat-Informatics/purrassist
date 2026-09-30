#!/usr/bin/env python3
# SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
# SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0
"""Check measurement parsing and fail-closed behavior without invoking profilers."""
import copy
import hashlib
import json
import importlib.util
from pathlib import Path
import tempfile
import unittest
from types import SimpleNamespace
from unittest.mock import patch

SPEC = importlib.util.spec_from_file_location('resources', Path(__file__).with_name('measure-resources.py'))
if SPEC is None or SPEC.loader is None:
    raise ImportError('could not load scripts/measure-resources.py as a module')
RESOURCES = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(RESOURCES)
if RESOURCES.__file__ is None:
    raise ImportError('scripts/measure-resources.py module has no __file__')
RESOURCES_PATH = Path(RESOURCES.__file__)


class Reports(unittest.TestCase):
    def test_allocation_summary(self):
        text = '==1== total heap usage: 1,234 allocs, 1,233 frees, 56,789 bytes allocated\n==1== ERROR SUMMARY: 0 errors from 0 contexts\n'
        self.assertEqual(RESOURCES.memcheck(text), {'allocation_calls': 1234, 'free_calls': 1233, 'total_allocated_bytes': 56789})
        for broken in (text.replace('0 errors', '1 errors'), text.split('\n')[0], ''):
            with self.assertRaises(ValueError):
                RESOURCES.memcheck(broken)

    def test_peak_semantics(self):
        text = 'snapshot=0\nmem_heap_B=100\nmem_heap_extra_B=16\nmem_stacks_B=0\nheap_tree=peak\nsnapshot=1\nmem_heap_B=105\nmem_heap_extra_B=0\nmem_stacks_B=0\nheap_tree=empty\n'
        self.assertEqual(RESOURCES.massif(text), {'observed_peak_requested_heap_bytes': 105, 'observed_peak_heap_plus_estimated_overhead_bytes': 116, 'snapshots': 2})
        for broken in ('', text.replace('heap_tree=peak', 'heap_tree=empty'), text.replace('mem_heap_extra_B=16\n', '')):
            with self.assertRaises(ValueError):
                RESOURCES.massif(broken)

    def test_native_rss(self):
        self.assertEqual(RESOURCES.rss(b'ok empty\nrss_bytes 4096\n', 'empty'), 4096)
        for output in (b'ok empty\n', b'ok empty\nrss_bytes 0\n', b'ok empty\nrss_bytes -1\n', b'ok empty\nrss_bytes 4096\nextra'):
            with self.assertRaises(ValueError):
                RESOURCES.rss(output, 'empty')

    def test_workload_output(self):
        RESOURCES.verify_output(b'ok empty\n', 'empty')
        for output in (b'', b'ok another\n', b'ok empty\nextra'):
            with self.assertRaises(ValueError):
                RESOURCES.verify_output(output, 'empty')


class ExtendedSchema(unittest.TestCase):
    """Task 13 additions: artifact sizes, latency/recovery bench capture,
    and cold-build cost. Only the pure, no-subprocess pieces are exercised
    here (this file's own contract is parsing/fail-closed behavior without
    invoking profilers, builders, or the network); the full pipeline is
    exercised by actually running `make resources`."""

    def test_identity_reports_size_and_digest(self):
        with tempfile.NamedTemporaryFile() as handle:
            handle.write(b'purrassist')
            handle.flush()
            report = RESOURCES.identity(Path(handle.name))
        self.assertEqual(report, {
            'bytes': 10,
            'sha256': hashlib.sha256(b'purrassist').hexdigest(),
        })

    def test_cold_build_isolates_intermediates_and_both_wrapper_layers(self):
        def fake_build(command, **kwargs):
            environment = kwargs['env']
            for name in ('RUSTC_WRAPPER', 'RUSTC_WORKSPACE_WRAPPER',
                         'CARGO_BUILD_RUSTC_WRAPPER', 'CARGO_BUILD_RUSTC_WORKSPACE_WRAPPER'):
                self.assertEqual(environment[name], '')
            target = Path(environment['CARGO_TARGET_DIR'])
            intermediate = Path(environment['CARGO_BUILD_BUILD_DIR'])
            self.assertFalse(target.exists())
            self.assertFalse(intermediate.exists())
            self.assertEqual(target.parent, intermediate.parent)
            self.assertNotIn(RESOURCES.ROOT.resolve(), target.resolve().parents)
            self.assertNotIn(RESOURCES.ROOT.resolve(), intermediate.resolve().parents)
            self.assertIn('build.rustc-wrapper=""', command)
            self.assertIn('build.rustc-workspace-wrapper=""', command)
            self.assertIn(f'build.build-dir="{intermediate}"', command)
            target.mkdir()
            binary = target / 'purrassist'
            binary.write_bytes(b'uncached executable')
            event = {'reason': 'compiler-artifact', 'fresh': False,
                     'target': {'name': 'purrassist'}, 'executable': str(binary)}
            return SimpleNamespace(stdout=json.dumps(event), stderr='', check_returncode=lambda: None)
        with tempfile.TemporaryDirectory() as directory, \
             patch.object(RESOURCES, 'ROOT', Path(directory)), \
             patch.object(RESOURCES, 'toolchain_cargo', return_value='/toolchain/cargo'), \
             patch.object(RESOURCES.subprocess, 'run', side_effect=fake_build):
            result = RESOURCES.cold_build()
        self.assertEqual(result['fresh_artifacts'], 0)
        self.assertEqual(result['built_artifacts'], 1)
        self.assertTrue(result['compiler_cache_disabled'])
        self.assertTrue(result['fresh_build_directory'])
        self.assertNotIn('/toolchain', result['method'])

    def test_cold_build_rejects_a_reused_compiler_artifact(self):
        reused = {'reason': 'compiler-artifact', 'fresh': True}
        response = SimpleNamespace(stdout=json.dumps(reused), stderr='', check_returncode=lambda: None)
        with tempfile.TemporaryDirectory() as directory, \
             patch.object(RESOURCES, 'ROOT', Path(directory)), \
             patch.object(RESOURCES, 'toolchain_cargo', return_value='/toolchain/cargo'), \
             patch.object(RESOURCES.subprocess, 'run', return_value=response):
            with self.assertRaisesRegex(ValueError, 'reused'):
                RESOURCES.cold_build()

    def test_engine_identity_tracks_executable_inputs_and_excludes_generated_reports(self):
        with tempfile.TemporaryDirectory() as directory, \
             patch.object(RESOURCES, 'ROOT', Path(directory)):
            root = Path(directory)
            for name in ('Cargo.toml', 'Cargo.lock', 'rust-toolchain.toml',
                         'Makefile', 'scripts/measure-resources.py',
                         'scripts/check-resource-limits.py', 'scripts/test-resource-report.py',
                         'crates/core/Cargo.toml', 'crates/core/src/lib.rs',
                         'crates/core/src/schema/metaschemas/schema.json'):
                path = root / name
                path.parent.mkdir(parents=True, exist_ok=True)
                path.write_text('original\n')
            baseline = RESOURCES.engine_source_identity()
            report = root / 'docs/qualification/resources-linux-x86_64.json'
            report.parent.mkdir(parents=True)
            report.write_text('generated evidence\n')
            self.assertEqual(RESOURCES.engine_source_identity(), baseline)
            (root / 'crates/core/src/schema/metaschemas/schema.json').write_text('{"type":"string"}\n')
            changed_data = RESOURCES.engine_source_identity()
            self.assertNotEqual(changed_data, baseline)
            (root / 'crates/core/src/lib.rs').write_text('changed implementation\n')
            changed_engine = RESOURCES.engine_source_identity()
            self.assertNotEqual(changed_engine, changed_data)
            (root / 'scripts/measure-resources.py').write_text('changed measurement method\n')
            self.assertNotEqual(RESOURCES.engine_source_identity(), changed_engine)


LIMIT_SPEC = importlib.util.spec_from_file_location('limits', Path(__file__).with_name('check-resource-limits.py'))
if LIMIT_SPEC is None or LIMIT_SPEC.loader is None:
    raise ImportError('could not load resource checker')
LIMITS = importlib.util.module_from_spec(LIMIT_SPEC)
LIMIT_SPEC.loader.exec_module(LIMITS)


class ResourceGate(unittest.TestCase):
    def setUp(self):
        # Exercise real committed measurements; missing fields must fail.
        self.report = json.loads((RESOURCES.ROOT / 'docs/qualification/resources-linux-x86_64.json').read_text())

    def test_complete_evidence_schema(self):
        LIMITS.check_schema(self.report)

    def test_incomplete_coverage_duplicates_and_invalid_measurements_fail(self):
        mutations = (
            lambda r: r['latency_workloads'].pop(),
            lambda r: r['syntax_latency_workloads'].append(copy.deepcopy(r['syntax_latency_workloads'][0])),
            lambda r: r['workloads'][0].update(process_peak_rss_bytes=0),
            lambda r: r['latency_workloads'][0].update(median_ns=-1),
            lambda r: r['latency_workloads'][0].update(allocation_calls=True),
            lambda r: r['latency_workloads'][0].update(peak_live_bytes=float('nan')),
            lambda r: r['latency_workloads'][0].update(p95_ns=0),
            lambda r: r['artifact_sizes']['native_cli'].update(sha256='unverified'),
            lambda r: r['source'].update(engine_source_tree_sha256='unverified'),
            lambda r: r['builds'][0].update(command='cargo build'),
            lambda r: r['builds'][0].update(executable_bytes=True),
            lambda r: r['platform'].update(libc='glibc'),
            lambda r: r['cold_build'].update(compiler_cache_disabled=False),
            lambda r: r['cold_build'].update(fresh_artifacts=1),
            lambda r: r['recovery_benchmark'][0].update(correct=199),
            lambda r: r['recovery_benchmark'][0].update(wrong_corrections=1),
        )
        for mutation in mutations:
            report = copy.deepcopy(self.report)
            mutation(report)
            with self.subTest(mutation=mutation), self.assertRaises(SystemExit):
                LIMITS.check_schema(report)

    def test_limit_violation_is_separate_from_evidence_schema(self):
        limits = LIMITS.load_limits()
        self.report['cold_build']['elapsed_ns'] = limits['cold_build_elapsed_ns'] + 1
        LIMITS.check_schema(self.report)
        self.assertTrue(any('cold_build.elapsed_ns' in violation
                            for violation in LIMITS.check_limits(self.report, limits)))


if __name__ == '__main__':
    unittest.main()
