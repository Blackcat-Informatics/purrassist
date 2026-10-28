#!/usr/bin/env python3
# SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
# SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0
"""Check measurement parsing and fail-closed behavior without invoking profilers."""
import copy
import hashlib
import io
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
    def test_failed_profiler_retains_output_and_surfaces_startup_or_memory_error(self):
        for code in (1, 97):
            with self.subTest(exit_code=code), tempfile.TemporaryDirectory() as temporary:
                directory = Path(temporary)
                detail = 'Fatal startup redirection failure' if code == 1 else 'Invalid read of size 8'

                def failed(command, **kwargs):
                    self.assertFalse(kwargs['check'])
                    self.assertIn('--leak-check=full', command)
                    self.assertIn('--error-exitcode=97', command)
                    self.assertIn('--errors-for-leak-kinds=definite,indirect,possible', command)
                    (directory / 'empty.memcheck.log').write_text(detail)
                    return SimpleNamespace(returncode=code, stdout=b'partial workload output\n',
                                           stderr=b'profiler stderr diagnostic\n')

                with patch.object(RESOURCES, 'native', return_value={}), \
                     patch.object(RESOURCES.subprocess, 'run', side_effect=failed):
                    with self.assertRaises(RuntimeError) as error:
                        RESOURCES.profile(['/benchmark'], 'empty', directory, {})
                message = str(error.exception)
                for expected in (f'exited {code}', detail, 'partial workload output',
                                 'profiler stderr diagnostic', 'empty.memcheck.log'):
                    self.assertIn(expected, message)
                self.assertEqual((directory / 'empty.memcheck.stdout').read_bytes(), b'partial workload output\n')
                self.assertEqual((directory / 'empty.memcheck.stderr').read_bytes(), b'profiler stderr diagnostic\n')

    def test_failed_profiler_reports_missing_tool_log_and_success_still_checks_output(self):
        with tempfile.TemporaryDirectory() as temporary:
            directory = Path(temporary)
            failed = SimpleNamespace(returncode=1, stdout=b'', stderr=b'cannot start tool')
            with patch.object(RESOURCES.subprocess, 'run', return_value=failed):
                with self.assertRaisesRegex(RuntimeError, 'cannot start tool') as error:
                    RESOURCES.run_profiler(['valgrind'], ['/benchmark'], 'empty', 'memcheck', directory, {})
            self.assertIn('(not created)', str(error.exception))
            wrong = SimpleNamespace(returncode=0, stdout=b'ok other\n', stderr=b'')
            with patch.object(RESOURCES.subprocess, 'run', return_value=wrong):
                with self.assertRaisesRegex(ValueError, 'unexpected workload output'):
                    RESOURCES.run_profiler(['valgrind'], ['/benchmark'], 'empty', 'memcheck', directory, {})
            self.assertEqual((directory / 'empty.memcheck.stdout').read_bytes(), b'ok other\n')

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
    """Latency/recovery bench capture,
    and cold-build cost. Only the pure, no-subprocess pieces are exercised
    here (this file's own contract is parsing/fail-closed behavior without
    invoking profilers, builders, or the network); the full pipeline is
    exercised by actually running `make resources`."""

    def test_identity_reports_digest(self):
        with tempfile.NamedTemporaryFile() as handle:
            handle.write(b'purrassist')
            handle.flush()
            report = RESOURCES.identity(Path(handle.name))
        self.assertEqual(report, {
            'sha256': hashlib.sha256(b'purrassist').hexdigest(),
        })

    def test_portable_loader_debug_symbols_are_verified_and_recorded(self):
        with tempfile.TemporaryDirectory() as directory, \
             patch.object(RESOURCES, 'ROOT', Path(directory)), \
             patch.object(RESOURCES.platform, 'machine', return_value='x86_64'), \
             patch.object(RESOURCES.subprocess, 'run'), \
             patch.object(RESOURCES.subprocess, 'check_output', return_value='bsdtar test version'), \
             patch.object(RESOURCES.urllib.request, 'urlopen', side_effect=AssertionError('cached inputs need no network')):
            root = Path(directory)
            cache = root / '.cache/profile-runtime'
            manifest = {
                'architecture': 'x86_64', 'name': 'test runtime', 'version': '1',
                'sha256': hashlib.sha256(b'archive').hexdigest(),
                'members': ['usr/lib/ld-linux-x86-64.so.2', 'usr/lib/libc.so.6'],
                'loader_debuginfo': {
                    'url': 'https://example.invalid/loader.debug',
                    'sha256': hashlib.sha256(b'verified debug symbols').hexdigest(),
                    'member': 'usr/lib/.debug/ld-linux-x86-64.so.2.debug',
                },
            }
            (root / 'vendor').mkdir()
            (root / 'vendor/profiling-runtime.json').write_text(json.dumps(manifest))
            for name in manifest['members']:
                path = cache / name
                path.parent.mkdir(parents=True, exist_ok=True)
                path.write_bytes(b'runtime file')
            (cache / 'runtime.pkg.tar.zst').write_bytes(b'archive')
            name = manifest['loader_debuginfo']['member']
            debug = cache / name
            debug.parent.mkdir()
            debug.write_bytes(b'verified debug symbols')
            _, report = RESOURCES.runtime(Path('/benchmark'), True)
            self.assertEqual(report['files_sha256'][name], manifest['loader_debuginfo']['sha256'])
            debug.write_bytes(b'corrupted symbols')
            with self.assertRaisesRegex(ValueError, 'loader debuginfo digest mismatch'):
                RESOURCES.runtime(Path('/benchmark'), True)
            debug.unlink()
            with patch.object(RESOURCES.urllib.request, 'urlopen', return_value=io.BytesIO(b'verified debug symbols')) as fetch:
                _, report = RESOURCES.runtime(Path('/benchmark'), True)
            fetch.assert_called_once_with(manifest['loader_debuginfo']['url'])
            self.assertEqual(debug.read_bytes(), b'verified debug symbols')
            self.assertEqual(report['files_sha256'][name], manifest['loader_debuginfo']['sha256'])
            debug.unlink()
            debug.mkdir()
            with self.assertRaisesRegex(ValueError, 'loader debuginfo must be a regular file'):
                RESOURCES.runtime(Path('/benchmark'), True)

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
            lambda r: r['cold_build']['artifact'].update(sha256='unverified'),
            lambda r: r['source'].update(engine_source_tree_sha256='unverified'),
            lambda r: r['builds'][0].update(command='cargo build'),
            lambda r: r['builds'][0].update(executable_sha256='unverified'),
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
