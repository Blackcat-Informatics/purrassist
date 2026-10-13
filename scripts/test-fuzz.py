#!/usr/bin/env python3
# SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
# SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0
"""Verify coverage novelty boundaries, differential-oracle normalization, and
fail-closed campaign behavior for `scripts/fuzz.py`."""
import contextlib
import importlib.util
import io
import json
import shutil
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

def _load_module(path: Path, name: str):
    spec = importlib.util.spec_from_file_location(name, path)
    if spec is None or spec.loader is None:
        raise SystemExit(f'could not load module spec for {path}')
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


FUZZ = _load_module(Path(__file__).with_name('fuzz.py'), 'fuzz')


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

    def test_repeated_and_reordered_filenames_keep_each_region_mapping(self):
        document = self.document()
        core = str(FUZZ.ROOT / 'crates/core/src/json/parser.rs')
        other = str(FUZZ.ROOT / 'crates/core/src/markdown.rs')
        noncore = '/rustc/stdlib.rs'
        document['data'][0]['functions'] = [
            {'filenames': [core, noncore, core],
             'regions': [[1, 1, 2, 1, 1, 0, 0, 0], [1, 1, 2, 1, 9, 2, 0, 0]]},
            {'filenames': [noncore, other, core],
             'regions': [[1, 1, 2, 1, 7, 2, 0, 0], [3, 2, 4, 5, 2, 1, 0, 0],
                         [9, 1, 9, 2, 1, 0, 0, 0]]},
        ]
        self.assertEqual(FUZZ.covered_regions(document), {
            ('crates/core/src/json/parser.rs', 1, 1, 2, 1),
            ('crates/core/src/markdown.rs', 3, 2, 4, 5)})
        document['data'][0]['functions'].reverse()
        self.assertEqual(FUZZ.covered_regions(document), {
            ('crates/core/src/json/parser.rs', 1, 1, 2, 1),
            ('crates/core/src/markdown.rs', 3, 2, 4, 5)})

    def test_noncore_regions_remain_validated_after_cached_classification(self):
        for region in ([1] * 7, [1, 1, 2, 1, -1, 0, 0, 0],
                       [1, 1, 2, 1, 1, 1, 0, 0], [1, 1, 2, 1, True, 0, 0, 0]):
            document = self.document()
            document['data'][0]['functions'].append({
                'filenames': ['/rustc/stdlib.rs'], 'regions': [region]})
            with self.assertRaises(ValueError):
                FUZZ.covered_regions(document)

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
                    FUZZ.probe(Path('unused-probe'), b'broken input', directory, 1, 'json', {}, {})
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
                        FUZZ.probe(Path('unused-probe'), b'null', Path(name), 1, 'json', {}, {})
                    self.assertEqual(run.call_count, 1)

    def test_an_unexpected_differential_record_fails_a_non_differential_target(self):
        result = subprocess.CompletedProcess([], 0, b'ok\n{"kind":"pattern"}\n', b'')
        with tempfile.TemporaryDirectory() as name:
            with patch.object(FUZZ.subprocess, 'run', return_value=result):
                with self.assertRaises(ValueError):
                    FUZZ.probe(Path('unused-probe'), b'null', Path(name), 1, 'json', {}, {})

    def test_mutations_reproduce_and_respect_probe_bound(self):
        for target, config in FUZZ.TARGETS.items():
            left, right = FUZZ.Random(123), FUZZ.Random(123)
            for seed in [*config['seeds'], b'x' * 4096]:
                for _ in range(50):
                    mutated = FUZZ.mutate(seed, left, config['tokens'])
                    self.assertEqual(
                        mutated, FUZZ.mutate(seed, right, config['tokens']),
                        f'{target}: mutation is not reproducible for the same seed',
                    )
                    self.assertLessEqual(len(mutated), 4096, f'{target}: mutation exceeded the probe bound')

    def test_build_identity_tracks_sources_without_documentation_noise(self):
        with tempfile.TemporaryDirectory() as name:
            root = Path(name)
            (root / 'crates/core/src').mkdir(parents=True)
            (root / 'crates/core/examples').mkdir()
            for filename in ('Cargo.toml', 'Cargo.lock', 'rust-toolchain.toml', 'crates/core/Cargo.toml', 'crates/core/README.md'):
                (root / filename).write_text('fixed input', encoding='utf-8')
            source = root / 'crates/core/src/lib.rs'
            source.write_text('first engine', encoding='utf-8')
            harness = root / 'crates/core/examples/fuzz_patch.rs'
            harness.write_text('first harness', encoding='utf-8')
            with patch.object(FUZZ, 'ROOT', root):
                initial = FUZZ.input_identity('patch')
                (root / 'README.md').write_text('new documentation', encoding='utf-8')
                self.assertEqual(FUZZ.input_identity('patch'), initial)
                source.write_text('second engine', encoding='utf-8')
                changed = FUZZ.input_identity('patch')
                self.assertNotEqual(changed['engine_source_sha256'], initial['engine_source_sha256'])
                self.assertEqual(changed['harness_source_sha256'], initial['harness_source_sha256'])
                harness.write_text('second harness', encoding='utf-8')
                self.assertNotEqual(FUZZ.input_identity('patch')['harness_source_sha256'], initial['harness_source_sha256'])

    def test_every_target_seed_is_within_the_probe_bound(self):
        for target, config in FUZZ.TARGETS.items():
            for seed in config['seeds']:
                self.assertLessEqual(len(seed), 4096, f'{target}: a committed seed exceeds 4096 bytes')

    def test_every_target_has_a_registered_harness_source(self):
        manifest = (FUZZ.ROOT / 'crates/core/Cargo.toml').read_text(encoding='utf-8')
        for target in FUZZ.TARGETS:
            self.assertTrue(FUZZ.harness_source(target).is_file(), f'{target}: no probe source')
            self.assertIn(f'name = "fuzz-{FUZZ.example_name(target)}"', manifest, f'{target}: no [[example]] entry')
        self.assertEqual(FUZZ.harness_source('tool-call').name, 'fuzz_tool_call.rs')
        self.assertEqual(FUZZ.harness_source('tool-call-ref').name, 'fuzz_tool_call.rs')

    def test_tool_call_seeds_cover_every_conformance_fixture_and_raw_bytes(self):
        markup = FUZZ._tool_call_conformance_markup()
        self.assertEqual(len(markup), len(list((FUZZ.ROOT / 'crates/core/tests/data/tool-calls/conformance').glob('*/*.json'))))
        turns = [seed[2:] for seed in FUZZ.TARGETS['tool-call']['seeds']]
        for fixture in markup:
            self.assertIn(fixture, turns)
        self.assertTrue(any(b'\xff' in turn for turn in turns))
        self.assertTrue(FUZZ.admits_bytes('tool-call'))
        self.assertTrue(FUZZ.admits_bytes('tool-call-ref'))
        self.assertFalse(any(FUZZ.admits_bytes(target) for target in FUZZ.TARGETS
                             if target not in ('tool-call', 'tool-call-ref')))

    def test_strict_tool_call_seeds_draw_what_their_comments_claim(self):
        """The strict seeds' control and trailer bytes, decoded as the probe
        header documents (`fuzz_tool_call.rs`): bits 4-5 of the control byte
        pick the reasoning policy (`10` is "as content"), bits 6-7 `11` force
        `read_file`; trailer bit 2 forbids reasoning. A strict no-reasoning
        choice under "as content" is a request error, so the seeds that test
        no-reasoning must not draw it, and one seed must."""
        seeds = FUZZ.TOOL_CALL_STRICT
        for seed in seeds:
            self.assertEqual(seed[0] >> 6, 3, 'strict seeds force read_file')
            self.assertGreaterEqual(seed[-1], 0xF8)
        decoded = [((seed[0] >> 4) & 3, seed[-1] & 4 != 0) for seed in seeds]
        as_content, separate = 2, (0, 3)
        for index in (3, 4):
            reasoning, no_reasoning = decoded[index]
            self.assertIn(reasoning, separate, f'seed {index} must test no-reasoning under separate reasoning')
            self.assertTrue(no_reasoning)
        self.assertEqual(decoded[5], (as_content, True), 'seed 5 draws the request-error combination')
        for index in (0, 1, 2, 6, 7):
            self.assertFalse(decoded[index][1] and decoded[index][0] == as_content, f'seed {index}')

    def test_strict_tool_call_seeds_run_ok_in_the_built_probe(self):
        """Every strict seed, the request-error seed included, completes with
        `ok` in the real probe."""
        root = FUZZ.ROOT
        build = subprocess.run(
            ['cargo', 'build', '-q', '-p', 'purrassist-core', '--example', 'fuzz-tool-call'],
            cwd=root, capture_output=True, encoding='utf-8')
        self.assertEqual(build.returncode, 0, build.stderr)
        metadata = subprocess.run(
            ['cargo', 'metadata', '--format-version', '1', '--no-deps'],
            cwd=root, capture_output=True, encoding='utf-8', check=True)
        probe = Path(json.loads(metadata.stdout)['target_directory']) / 'debug/examples/fuzz-tool-call'
        self.assertTrue(probe.is_file(), probe)
        for index, seed in enumerate(FUZZ.TOOL_CALL_STRICT):
            run = subprocess.run([str(probe)], input=seed, capture_output=True, cwd=root)
            self.assertEqual((run.returncode, run.stdout), (0, b'ok\n'), (index, run.stderr.decode(errors='replace')))

    def test_forced_header_seeds_with_a_trailer_byte_are_registered_strict_seeds(self):
        """G9: the probe consumes a last byte >= 0xF8 as the strict-forced
        trailer only under the forced choice (control bits 6-7 `11`). Every
        seed of a tool-call target with that header and such a last byte must
        therefore be one of its registered strict seeds (and so be reviewed
        for what its trailer draws); a seed that is not registered would be
        silently misread as carrying options."""
        for target in ('tool-call', 'tool-call-ref'):
            config = FUZZ.TARGETS[target]
            strict = config['strict']
            self.assertTrue(strict, target)
            forced = [seed for seed in config['seeds']
                      if len(seed) > 2 and seed[0] >> 6 == 3 and seed[-1] >= 0xF8]
            self.assertTrue(forced, target)
            for seed in forced:
                self.assertIn(seed, strict, f'{target}: an unregistered forced-header trailer seed {seed[:40]!r}')
            for seed in strict:
                self.assertIn(seed, config['seeds'], target)
                self.assertGreaterEqual(seed[-1], 0xF8)
                self.assertEqual(seed[0] >> 6, 3)

    def test_the_tool_call_seed_set_is_unchanged_by_the_ref_target(self):
        """The existing `tool-call` seeds are byte-for-byte what they were
        (modulo the R4 seed-4 correction), not extended by `tool-call-ref`."""
        self.assertNotIn('probe_args', FUZZ.TARGETS['tool-call'])
        self.assertEqual(FUZZ.probe_arguments('tool-call'), [])
        self.assertEqual(FUZZ.example_name('tool-call'), 'tool-call')
        self.assertFalse(set(FUZZ.TOOL_CALL_REF_SEEDS) & set(FUZZ.TOOL_CALL_SEEDS))

    def test_tool_call_ref_target_is_registered_with_the_ref_tool_set(self):
        config = FUZZ.TARGETS['tool-call-ref']
        self.assertEqual(FUZZ.example_name('tool-call-ref'), 'tool-call')
        self.assertEqual(FUZZ.probe_arguments('tool-call-ref'), ['--tools', 'ref'])
        self.assertEqual(config['input'], 'bytes')
        makefile = (FUZZ.ROOT / 'Makefile').read_text(encoding='utf-8')
        loop = next(line for line in makefile.splitlines() if 'for target in' in line)
        self.assertIn('tool-call-ref', loop.replace(';', ' ').split())
        self.assertNotIn('continue-on-known', ''.join(
            line for line in makefile.splitlines() if 'scripts/fuzz.py' in line))
        documentation = (FUZZ.ROOT / 'docs/FUZZING.md').read_text(encoding='utf-8')
        for needle in ('tool-call-ref', '--tools ref', '--continue-on-known'):
            self.assertIn(needle, documentation)

    def test_tool_call_ref_seeds_cover_every_nested_shape(self):
        turns = [seed[2:] for seed in FUZZ.TOOL_CALL_REF_SEEDS if seed[0] >> 6 != 3]
        joined = b'\n'.join(turns)
        # Each tool of the ref set is called, and each shape is present.
        for tool in (b'list_records', b'batch_refs', b'any_items', b'tree_walk', b'free_items', b'root_ref'):
            self.assertIn(b'<function=' + tool + b'>', joined, tool)
        shapes = {
            'top level repeated items': FUZZ._qwen_call('list_records', FUZZ._REF_ITEM_A + FUZZ._REF_ITEM_B),
            'in-object array': b'<parameter=children>',
            'mixed JSON and nested': b'<parameter=records>\n[{"name": "A", "uid": 1}]',
            'unclosed last item': b'<parameter=name>\nA\n</function>',
            'unclosed item before a sibling': b'<parameter=name>\nA\n<parameter=owner>',
            'same-name member': b'<parameter=records>\nr\n</parameter>\n<parameter=name>',
            'string items': b'<parameter=tags>\na\n</parameter>\n<parameter=tags>\nb',
            'ref items': b'<function=batch_refs>\n<parameter=items>',
            'colliding names': b'<parameter=name>\ninner\n</parameter>\n<parameter=name>\nouter',
            'recursive tree': b'<parameter=children>\n<parameter=label>\nb\n</parameter>\n<parameter=children>',
            'anyOf items': b'<function=any_items>',
            'opaque item': b'<parameter=anything>',
            'root ref call': b'<function=root_ref>\n<parameter=path>',
        }
        for shape, needle in shapes.items():
            self.assertIn(needle, joined, shape)

    def test_tool_call_ref_seeds_run_ok_in_the_built_probe_under_both_tool_sets(self):
        root = FUZZ.ROOT
        build = subprocess.run(
            ['cargo', 'build', '-q', '-p', 'purrassist-core', '--example', 'fuzz-tool-call'],
            cwd=root, capture_output=True, encoding='utf-8')
        self.assertEqual(build.returncode, 0, build.stderr)
        metadata = subprocess.run(
            ['cargo', 'metadata', '--format-version', '1', '--no-deps'],
            cwd=root, capture_output=True, encoding='utf-8', check=True)
        probe = Path(json.loads(metadata.stdout)['target_directory']) / 'debug/examples/fuzz-tool-call'
        for arguments in (FUZZ.probe_arguments('tool-call-ref'), []):
            for index, seed in enumerate(FUZZ.TARGETS['tool-call-ref']['seeds']):
                run = subprocess.run([str(probe), *arguments], input=seed, capture_output=True, cwd=root)
                self.assertEqual((run.returncode, run.stdout), (0, b'ok\n'),
                                 (arguments, index, run.stderr.decode(errors='replace')))
        usage = subprocess.run([str(probe), '--tools', 'bogus'], input=b'\x00\x00', capture_output=True, cwd=root)
        self.assertNotEqual(usage.returncode, 0)

    def test_probe_runs_the_selected_tool_set(self):
        seen = []

        def fake(command, **_):
            seen.append(command)
            raise subprocess.TimeoutExpired(command, 1)

        for target, expected in (('tool-call', ['probe']), ('tool-call-ref', ['probe', '--tools', 'ref'])):
            with tempfile.TemporaryDirectory() as directory:
                with patch.object(FUZZ.subprocess, 'run', side_effect=fake):
                    with self.assertRaises(subprocess.TimeoutExpired):
                        FUZZ.probe(Path('probe'), b'\x00\x00', Path(directory), 1, target, {}, {})
            self.assertEqual(seen[-1], expected)

    def test_known_failure_signatures_are_only_the_issue_6_panics(self):
        runs_past = ('thread \'main\' (24424) panicked at crates/core/examples/fuzz_tool_call.rs:1156:9:\n'
                     'rejected attempt 1..190 runs past where it failed (39) without its closer: "x"\n')
        beyond = ('thread \'main\' (154968) panicked at crates/core/examples/fuzz_tool_call.rs:495:9:\n'
                  'rejected attempt 218..322 fails at 159, beyond a literal past its end\n')
        self.assertEqual(FUZZ.known_failure(runs_past), 'issue-6-rejected-span-runs-past-failure')
        self.assertEqual(FUZZ.known_failure(beyond), 'issue-6-failure-beyond-literal-past-its-end')
        echoed = ('input: rejected attempt 1..2 runs past where it failed (3)\n'
                  'thread \'main\' panicked at crates/core/examples/fuzz_tool_call.rs:7:9:\n'
                  'assertion failed: reasoning blocks overlap\n')
        for other in ('assertion failed: reasoning blocks overlap',
                      'a rejected attempt\'s raw bytes are not its range',
                      'rejected attempt without either phrase', '',
                      'rejected attempt 1..190 runs past where it failed (39) without a panic line',
                      echoed):
            self.assertIsNone(FUZZ.known_failure(other), other)

    def _campaign(self, arguments, outcomes):
        """Run `FUZZ.main` over a two-seed fake target whose probe gives
        `outcomes` (a stderr string for a crash, `None` for a pass)."""
        directory = Path(tempfile.mkdtemp())
        self.addCleanup(lambda: shutil.rmtree(directory, ignore_errors=True))
        outcomes = list(outcomes)
        seeds = [bytes([index]) * 3 for index in range(len(outcomes))]

        def fake_probe(binary, data, where, timeout, target, oracle, tool_paths, counts=None):
            stderr = outcomes[seeds.index(data)]
            (where / 'probe.stderr').write_text(stderr or '', encoding='utf-8')
            if stderr is not None:
                counts['crashes'] += 1
                raise subprocess.CalledProcessError(101, ['probe'])
            return {('crates/core/src/x.rs', len(data), 1, 2, 3)}, False, 0

        config = dict(FUZZ.TARGETS['tool-call'], seeds=seeds)
        with patch.object(FUZZ, 'TARGETS', {'tool-call': config}), \
             patch.object(FUZZ, 'probe', fake_probe), \
             patch.object(FUZZ, 'tool_identity', return_value=({}, {}, {})), \
             patch.object(FUZZ, 'build', return_value=(Path('probe'), {})), \
             patch.object(FUZZ.tempfile, 'mkdtemp', return_value=str(directory)), \
             patch.object(FUZZ.subprocess, 'check_output', return_value=''), \
             patch.object(FUZZ, 'source_digest', return_value='digest'), \
             patch('sys.argv', ['fuzz.py', '--target', 'tool-call', '--iterations', '0', *arguments]), \
             contextlib.ExitStack() as quiet:
            # A mocked campaign's crash and known-panic lines would read as real
            # findings in a CI log; keep them out unless a test captures them.
            if sys.stdout is sys.__stdout__:
                quiet.enter_context(contextlib.redirect_stdout(io.StringIO()))
            if sys.stderr is sys.__stderr__:
                quiet.enter_context(contextlib.redirect_stderr(io.StringIO()))
            try:
                FUZZ.main()
                error = None
            except subprocess.CalledProcessError as caught:
                error = caught
        return directory, error

    KNOWN = ('thread \'main\' (1) panicked at crates/core/examples/fuzz_tool_call.rs:1156:9:\n'
             'rejected attempt 0..3 runs past where it failed (3) without its closer: ""\n')

    def test_continue_on_known_retains_every_issue_6_input_and_continues(self):
        directory, error = self._campaign(['--continue-on-known'], [self.KNOWN, None, self.KNOWN])
        self.assertIsNone(error)
        report = json.loads((directory / 'report.json').read_text(encoding='utf-8'))
        self.assertEqual((report['status'], report['executions'], report['crashes']), ('passed', 3, 0))
        self.assertTrue(report['continue_on_known'])
        self.assertEqual([entry['execution'] for entry in report['known_failures']], [0, 2])
        for entry in report['known_failures']:
            self.assertTrue((directory / 'known-6' / f'{entry["sha256"]}.bin').is_file())
            self.assertIn('runs past where it failed',
                          (directory / 'known-6' / f'{entry["sha256"]}.stderr').read_text(encoding='utf-8'))

    def test_without_continue_on_known_the_first_issue_6_panic_stops_the_campaign(self):
        directory, error = self._campaign([], [self.KNOWN, None])
        self.assertIsInstance(error, subprocess.CalledProcessError)
        report = json.loads((directory / 'report.json').read_text(encoding='utf-8'))
        self.assertEqual((report['status'], report['crashes'], report['known_failures']), ('failed', 1, []))
        self.assertFalse((directory / 'known-6').exists())

    def test_continue_on_known_still_stops_at_any_other_failure(self):
        directory, error = self._campaign(['--continue-on-known'], [self.KNOWN, 'assertion failed: overlap'])
        self.assertIsInstance(error, subprocess.CalledProcessError)
        report = json.loads((directory / 'report.json').read_text(encoding='utf-8'))
        self.assertEqual((report['status'], report['crashes']), ('failed', 1))
        self.assertEqual(len(report['known_failures']), 1)

    def test_a_crash_prints_its_panic_and_classification_to_the_log(self):
        stderr = io.StringIO()
        with patch('sys.stderr', stderr):
            _, error = self._campaign([], [self.KNOWN, None])
        self.assertIsInstance(error, subprocess.CalledProcessError)
        self.assertIn('Probe crashed at execution 0; known issue signature: '
                      'issue-6-rejected-span-runs-past-failure', stderr.getvalue())
        self.assertIn('runs past where it failed', stderr.getvalue())

    def test_an_unknown_crash_is_classified_as_none_in_the_log(self):
        stderr = io.StringIO()
        with patch('sys.stderr', stderr):
            _, error = self._campaign(['--continue-on-known'], ['assertion failed: overlap'])
        self.assertIsInstance(error, subprocess.CalledProcessError)
        self.assertIn('known issue signature: none', stderr.getvalue())
        self.assertIn('assertion failed: overlap', stderr.getvalue())

    def test_a_retained_known_panic_is_named_in_the_log(self):
        stdout = io.StringIO()
        with patch('sys.stdout', stdout):
            _, error = self._campaign(['--continue-on-known'], [self.KNOWN, None])
        self.assertIsNone(error)
        self.assertIn('Known issue #6 panic at execution 0 (issue-6-rejected-span-runs-past-failure)',
                      stdout.getvalue())

    def test_byte_input_probe_completes_with_ok_on_invalid_utf8(self):
        document = Coverage().document()
        results = [subprocess.CompletedProcess([], 0, b'ok\n', b''),
                   subprocess.CompletedProcess([], 0, b'', b''),
                   subprocess.CompletedProcess([], 0, json.dumps(document).encode(), b'')]
        counts = {}
        tools = {'llvm-profdata': Path('profdata'), 'llvm-cov': Path('cov')}
        with tempfile.TemporaryDirectory() as directory:
            with patch.object(FUZZ.subprocess, 'run', side_effect=results):
                regions, invalid, checks = FUZZ.probe(
                    Path('probe'), b'\x00\x00\xff', Path(directory), 1, 'tool-call', {}, tools, counts)
        self.assertFalse(invalid)
        self.assertEqual((len(regions), checks, counts['non_utf8_admitted']), (1, 0, 1))
        for output in (b'invalid-utf8\n', b'ok\n{"kind":"pattern"}\n'):
            with tempfile.TemporaryDirectory() as directory:
                with patch.object(FUZZ.subprocess, 'run', return_value=subprocess.CompletedProcess([], 0, output, b'')):
                    with self.assertRaises(ValueError):
                        FUZZ.probe(Path('probe'), b'\x00\x00\xff', Path(directory), 1, 'tool-call', {}, tools)


class DifferentialNormalization(unittest.TestCase):
    """One test per differential probe's comparison normalization (both
    probes: `pattern` against Node, `markdown` against `cmark-gfm`)."""

    def test_normalize_html_ignores_only_line_endings_and_trailing_newlines(self):
        self.assertEqual(FUZZ.normalize_html('<p>tea</p>\n'), '<p>tea</p>\n')
        self.assertEqual(FUZZ.normalize_html('<p>tea</p>'), '<p>tea</p>\n')
        self.assertEqual(FUZZ.normalize_html('<p>tea</p>\r\n'), '<p>tea</p>\n')
        self.assertEqual(FUZZ.normalize_html('<p>tea</p>\n\n\n'), '<p>tea</p>\n')
        # Real content differences are never normalized away.
        self.assertNotEqual(FUZZ.normalize_html('<p>tea</p>\n'), FUZZ.normalize_html('<p>Tea</p>\n'))

    def test_normalizer_preserves_raw_rcdata_and_foreign_content(self):
        first = '<a z="2" a="1">'
        second = '<a a="1" z="2">'
        for name in ('script', 'style', 'xmp', 'iframe', 'noembed', 'noframes', 'title', 'textarea'):
            for opener in ('<' + name + '>', '<' + name.upper() + " data='>'>"):
                left = opener + first + '</' + name + '>'
                right = opener + second + '</' + name + '>'
                self.assertNotEqual(FUZZ.normalize_html(left), FUZZ.normalize_html(right), name)
                self.assertIn(first, FUZZ.normalize_html(left))
                for tail in ('', '\n', '\n\n'):
                    self.assertEqual(FUZZ.normalize_html(opener + first + tail), opener + first + tail)
            after = '<' + name + '>body</' + name + '>'
            self.assertEqual(FUZZ.normalize_html(after + first), FUZZ.normalize_html(after + second))
        for name in ('svg', 'math'):
            for body in ('<input/>x', '<' + name + '><input/>x</' + name + '><input/>y'):
                left = '<' + name + '>' + body + '</' + name + '>'
                right = left.replace('<input/>', '<input>')
                self.assertNotEqual(FUZZ.normalize_html(left), FUZZ.normalize_html(right))
                self.assertIn(body, FUZZ.normalize_html(left))
            for tail in ('', '\n', '\n\n'):
                source = '<' + name + '>' + first + tail
                self.assertEqual(FUZZ.normalize_html(source), source)
        for tail in ('', '\n', '\n\n'):
            source = '<plaintext>' + first + '</plaintext>' + tail
            self.assertEqual(FUZZ.normalize_html(source), source)
        for opener in ('<!--', '<![CDATA['):
            for tail in ('', '\n', '\n\n'):
                source = opener + first + tail
                self.assertEqual(FUZZ.normalize_html(source), source)
            self.assertNotEqual(FUZZ.normalize_html(opener + 'x\n'), FUZZ.normalize_html(opener + 'x\n\n'))
        # Quoted attributes/comments cannot start a raw-text region.
        ordinary = '<p title="<script>" z="2" a="1">x</p>'
        self.assertEqual(FUZZ.normalize_html(ordinary), '<p a="1" title="<script>" z="2">x</p>\n')
        self.assertEqual(FUZZ.normalize_html('<!--<script>-->' + first),
                         FUZZ.normalize_html('<!--<script>-->' + second))
        self.assertNotEqual(FUZZ.normalize_html('<textarea>x\n'), FUZZ.normalize_html('<textarea>x\n\n'))

    def test_normalizer_never_searches_inside_unrecognized_outer_markup(self):
        from html.parser import HTMLParser
        class Attributes(HTMLParser):
            def __init__(self):
                super().__init__()
                self.attributes = []
            def handle_starttag(self, tag, attributes):
                self.attributes.append((tag, attributes))
        left = '<div data-x=foo<br/>\n'
        right = '<div data-x=foo<br>\n'
        parsed_left, parsed_right = Attributes(), Attributes()
        parsed_left.feed(left)
        parsed_right.feed(right)
        self.assertEqual(parsed_left.attributes, [('div', [('data-x', 'foo<br/')])])
        self.assertEqual(parsed_right.attributes, [('div', [('data-x', 'foo<br')])])
        self.assertNotEqual(FUZZ.normalize_html(left), FUZZ.normalize_html(right))
        for left, right in (
            ('<custom-tag data-x=foo<br/>', '<custom-tag data-x=foo<br>'),
            ('<!bogus <input b="2" a="1" />>', '<!bogus <input a="1" b="2">>'),
            ('<?xml "<br/>"?>', '<?xml "<br>"?>'),
            ('<svg><div data-x=foo<br/>', '<svg><div data-x=foo<br>'),
            ('<svg><div data-x=foo</svg>\n', '<svg><div data-x=foo</svg>\n\n')):
            self.assertEqual(FUZZ.normalize_html(left), left if left.startswith('<svg>') else left + '\n')
            self.assertEqual(FUZZ.normalize_html(right), right if right.startswith('<svg>') else right + '\n')
            self.assertNotEqual(FUZZ.normalize_html(left), FUZZ.normalize_html(right))
        for prefix in ('<?pi?>', '<!DOCTYPE html>', '<!DOCTYPE html PUBLIC \"literal > text\">', '<!bogus <tag>>', '<div data-x=foo<br/>'):
            after = '<input z=\"2\" a=\"1\" />'
            equivalent = '<input a=\"1\" z=\"2\">'
            self.assertEqual(FUZZ.normalize_html(prefix + after), FUZZ.normalize_html(prefix + equivalent))
        for whitespace in ('\u00a0', '\v'):
            source = '<p a=\"1\"' + whitespace + 'b=\"2\">x</p>'
            self.assertEqual(FUZZ.normalize_html(source), source + '\n')
            self.assertNotEqual(FUZZ.normalize_html(source), FUZZ.normalize_html(source.replace(whitespace, ' ')))
        for source in ('<?pi <br/', '<!DOCTYPE html PUBLIC \"<br/>', '<!bogus <br'):
            self.assertEqual(FUZZ.normalize_html(source), source)
        ordinary = '<my-widget z="2" a="1"><input disabled="" type="checkbox" /></my-widget>'
        equivalent = '<my-widget a="1" z="2"><input type="checkbox" disabled=""></my-widget>'
        self.assertEqual(FUZZ.normalize_html(ordinary), FUZZ.normalize_html(equivalent))
        self.assertNotEqual(FUZZ.normalize_html('<my-widget/>'), FUZZ.normalize_html('<my-widget>'))

    def test_normalize_html_ignores_attribute_order_and_void_self_closing(self):
        # cmark-gfm's CLI self-closes void elements and orders attributes
        # differently from the official GFM specification fixture (see
        # crates/core/tests/data/markdown/gfm-0.29.0.gfm.13.json example 279);
        # both spellings are the same element per the WHATWG HTML spec.
        ours = '<ul>\n<li><input disabled="" type="checkbox"> todo</li>\n</ul>\n'
        theirs = '<ul>\n<li><input type="checkbox" disabled="" /> todo</li>\n</ul>\n'
        self.assertEqual(FUZZ.normalize_html(ours), FUZZ.normalize_html(theirs))
        # A genuinely different attribute value is never normalized away.
        checked = '<input checked="" disabled="" type="checkbox">'
        unchecked = '<input disabled="" type="checkbox">'
        self.assertNotEqual(FUZZ.normalize_html(checked), FUZZ.normalize_html(unchecked))
        self.assertNotEqual(FUZZ.normalize_html('<custom />'), FUZZ.normalize_html('<custom>'))

    def test_normalization_preserves_comment_and_cdata_text(self):
        for opaque in ('<!-- <input b="2" a="1" /> -->', '<![CDATA[<input b="2" a="1" />]]>'):
            self.assertEqual(FUZZ.normalize_html(opaque), opaque + '\n')
        first = '<!-- <input b="2" a="1" /> -->'
        second = '<!-- <input a="1" b="2"> -->'
        self.assertNotEqual(FUZZ.normalize_html(first), FUZZ.normalize_html(second))

    def test_normalization_preserves_duplicate_attribute_precedence(self):
        first = '<p a="2" a="1">tea</p>'
        second = '<p a="1" a="2">tea</p>'
        self.assertNotEqual(FUZZ.normalize_html(first), FUZZ.normalize_html(second))
        self.assertIn('a="2" a="1"', FUZZ.normalize_html(first))
        case_duplicate = '<p a="2" A="1">tea</p>'
        self.assertIn('a="2" A="1"', FUZZ.normalize_html(case_duplicate))
        self.assertNotEqual(FUZZ.normalize_html(case_duplicate), FUZZ.normalize_html('<p A="1" a="2">tea</p>'))
        self.assertEqual(FUZZ.normalize_html('<p a="1" b="2">tea</p>'), FUZZ.normalize_html('<p b="2" a="1">tea</p>'))

    def test_node_pattern_test_normalizes_trailing_stdout_whitespace(self):
        with patch.object(
            FUZZ.subprocess, 'run',
            return_value=subprocess.CompletedProcess([], 0, 'true\n', ''),
        ):
            self.assertIs(FUZZ.node_pattern_test('a+', 'aaa', 5), True)
        with patch.object(
            FUZZ.subprocess, 'run',
            return_value=subprocess.CompletedProcess([], 0, ' false \n', ''),
        ):
            self.assertIs(FUZZ.node_pattern_test('a+', 'bbb', 5), False)
        with patch.object(
            FUZZ.subprocess, 'run',
            return_value=subprocess.CompletedProcess([], 0, 'unsupported', ''),
        ):
            self.assertIsNone(FUZZ.node_pattern_test('(a', 'a', 5))
        # A reference-tool timeout (catastrophic V8 backtracking on an
        # adversarial pattern this project's derivative engine is specifically
        # designed to avoid) is skipped, never raised as a mismatch.
        with patch.object(
            FUZZ.subprocess, 'run',
            side_effect=subprocess.TimeoutExpired('node', 5),
        ):
            self.assertIsNone(FUZZ.node_pattern_test('(a+)+$', 'a' * 40, 5))

    def test_pattern_differential_mismatch_is_reported_precisely(self):
        record = {'kind': 'pattern', 'pattern': 'a+', 'subject': 'aaa', 'expected': True}
        with patch.object(FUZZ, 'node_pattern_test', return_value=False):
            with self.assertRaisesRegex(ValueError, r'pattern differential mismatch.*a\+.*aaa'):
                FUZZ.check_pattern_differential(record, 5)

    def test_reference_rejection_and_timeout_are_counted_separately(self):
        counts = {'reference_rejections': 0, 'reference_timeouts': 0}
        with patch.object(FUZZ.subprocess, 'run', return_value=subprocess.CompletedProcess([], 0, 'unsupported', '')):
            self.assertIsNone(FUZZ.node_pattern_test('(a', 'a', 5, counts))
        with patch.object(FUZZ.subprocess, 'run', side_effect=subprocess.TimeoutExpired('node', 5)):
            self.assertIsNone(FUZZ.node_pattern_test('(a+)+$', 'a' * 40, 5, counts))
        self.assertEqual(counts, {'reference_rejections': 1, 'reference_timeouts': 1})

    def test_tasklist_reference_profile_is_selected_from_raw_input(self):
        for source in ('- - [x] d\n', '> - [ ] d\n', '1. - [X]\td\r\n', '- > - [x] d\n'):
            self.assertTrue(FUZZ.tasklist_reference_limited(source), source)
        for source in ('- [x] d\n', '- parent\n  - [x] d\n', '    - - [x] d\n', '- - [x]word\n'):
            self.assertFalse(FUZZ.tasklist_reference_limited(source), source)

    def test_tasklist_raw_profile_uses_horizontal_ascii_whitespace_and_first_marker(self):
        for separator in (' ', '\t', '\v', '\f'):
            self.assertTrue(FUZZ.tasklist_reference_limited('- - [x]' + separator + 'd'))
            self.assertTrue(FUZZ.tasklist_reference_limited('- [ ]' + separator + 'literal [x]'))
            self.assertFalse(FUZZ.tasklist_reference_limited('- [x]' + separator + 'literal [ ]'))
            self.assertFalse(FUZZ.tasklist_reference_limited('- [ ]' + separator + 'ordinary'))
        for ending in ('\n', '\r', '\r\n'):
            self.assertFalse(FUZZ.tasklist_reference_limited('- [ ] ordinary' + ending + '[x]'))
        self.assertFalse(FUZZ.tasklist_reference_limited('- [ ]word [x]'))
        self.assertFalse(FUZZ.tasklist_reference_limited('    - [ ] done [x]'))
        reasons = FUZZ.tasklist_reference_limit_reasons('- - [ ] literal [X]')
        self.assertEqual(len(reasons), 2)

    def test_tasklist_projection_preserves_later_marker_and_vertical_whitespace(self):
        for separator in (' ', '\t', '\v', '\f'):
            plain = '<ul>\n<li>[ ]' + separator + 'literal [x]</li>\n</ul>\n'
            retained = separator if separator in '\v\f' else ''
            full = '<ul>\n<li><input disabled="" type="checkbox"> ' + retained + 'literal [x]</li>\n</ul>\n'
            self.assertTrue(FUZZ.check_tasklist_projection(plain, full))
            for wrong in (full.replace(' disabled=', ' checked="" disabled='),
                          full.replace('literal [x]', 'literal'), plain):
                with self.assertRaises(FUZZ.DifferentialMismatch):
                    FUZZ.check_tasklist_projection(plain, wrong)
        source = '- [ ] - [ ( [x]['
        self.assertTrue(FUZZ.tasklist_reference_limited(source))
        self.assertIn('later literal checked marker', FUZZ.tasklist_reference_limit_reasons(source)[0])

    def test_tasklist_profile_and_projection_follow_paragraph_whitespace(self):
        cases = (
            ('- [ ] \ntodo\n- done\n', '<ul>\n<li>[ ]\ntodo</li>\n<li>done</li>\n</ul>\n', '<ul>\n<li><input disabled="" type="checkbox"> todo</li>\n<li>done</li>\n</ul>\n'),
            ('- [x]\n  todo\n', '<ul>\n<li>[x]\ntodo</li>\n</ul>\n', '<ul>\n<li><input checked="" disabled="" type="checkbox"> todo</li>\n</ul>\n'),
            ('- [\n  ] todo\n', '<ul>\n<li>[\n] todo</li>\n</ul>\n', '<ul>\n<li><input disabled="" type="checkbox"> todo</li>\n</ul>\n'),
        )
        for source, plain, full in cases:
            for ending in ('\n', '\r\n', '\r'):
                self.assertTrue(FUZZ.tasklist_reference_limited(source.replace('\n', ending)))
            self.assertTrue(FUZZ.check_tasklist_projection(plain, full))
            with self.assertRaises(FUZZ.DifferentialMismatch):
                FUZZ.check_tasklist_projection(plain, full.replace('todo', 'lost'))
        for middle in ('\t', '\v', '\f'):
            source = '- [' + middle + '] todo'
            plain = '<ul>\n<li>[' + middle + '] todo</li>\n</ul>\n'
            full = '<ul>\n<li><input disabled="" type="checkbox"> todo</li>\n</ul>\n'
            self.assertTrue(FUZZ.tasklist_reference_limited(source))
            self.assertTrue(FUZZ.check_tasklist_projection(plain, full))
            with self.assertRaises(FUZZ.DifferentialMismatch):
                FUZZ.check_tasklist_projection(plain, full.replace(' disabled=', ' checked="" disabled='))
        self.assertFalse(FUZZ.tasklist_reference_limited('- [x]'))
        self.assertFalse(FUZZ.tasklist_reference_limited('- [\n  ]word'))
        self.assertTrue(FUZZ.check_tasklist_projection('<ul><li>[x]</li></ul>', '<ul><li>[x]</li></ul>'))

    def test_raw_reference_profile_prefix_scan_is_bounded_on_deep_quotes(self):
        self.assertFalse(FUZZ.tasklist_reference_limited('> ' * 2000 + 'end'))
        self.assertTrue(FUZZ.tasklist_reference_limited('> ' * 1000 + '- [x] done'))

    def test_tasklist_projection_checks_semantics_without_erasing_content(self):
        plain = '<ul>\n<li>[X] done</li>\n</ul>\n'
        full = '<ul>\n<li><input checked="" disabled="" type="checkbox"> done</li>\n</ul>\n'
        self.assertTrue(FUZZ.check_tasklist_projection(plain, full))
        for wrong in (full.replace(' checked=""', ''), full.replace('done', 'lost'), plain):
            with self.assertRaises(FUZZ.DifferentialMismatch):
                FUZZ.check_tasklist_projection(plain, wrong)
        self.assertFalse(FUZZ.check_tasklist_projection('<ul><li><a href="/x">x</a> done</li></ul>', full,
                         '- [x] done\n\n[x]: /x\n'))

    def test_task_projection_cannot_invent_markers_from_neutral_html(self):
        plain = '<ul>\n<li>[\n] todo</li>\n<li>[xx] do&lt;</li>\n</ul>\n'
        for source in ('- [\n ] todo\n- [xx] do<', '- [\n  ] todo\n',
                       '- [ \n ] todo\n', '- [\r\n\t] todo', '- [\r] todo'):
            self.assertIn('neutral softbreak HTML loses split-marker indentation and whitespace',
                          FUZZ.tasklist_projection_limit_reasons(source))
            self.assertFalse(FUZZ.check_tasklist_projection(plain, plain, source))
        for source in ('- [&#32;] todo\n- - [x] done\n', '- \\[ ] todo\n- - [x] done\n',
                       '<ul><li>[ ] todo</li></ul>\n- - [x] done\n'):
            self.assertTrue(FUZZ.tasklist_projection_limit_reasons(source))
            self.assertFalse(FUZZ.check_tasklist_projection('<ul><li>[ ] todo</li></ul>',
                '<ul><li>[ ] todo</li></ul>', source))

    def test_task_projection_unavailable_is_selected_from_source_before_oracle(self):
        source = '- [\n ] todo\n- [xx] do<'
        plain = '<ul>\n<li>[\n] todo</li>\n<li>[xx] do&lt;</li>\n</ul>\n'
        record = {'kind': 'markdown', 'profile': 'tasklist-reference-limited',
                  'source': source, 'html': plain, 'full_html': plain}
        counts = {'reference_profile_exclusions': 0, 'tasklist_projection_checks': 0,
                  'tasklist_projection_unavailable': 0, 'profile_exclusions': []}
        with patch.object(FUZZ, 'cmark_gfm_render', return_value=plain):
            self.assertTrue(FUZZ.check_markdown_differential(record, Path('oracle'), 5, counts))
        self.assertEqual(counts['tasklist_projection_checks'], 0)
        self.assertEqual(counts['tasklist_projection_unavailable'], 1)
        self.assertFalse(counts['profile_exclusions'][0]['projection_checked'])
        self.assertTrue(counts['profile_exclusions'][0]['projection_unavailable_reasons'])
        record['html'] = plain.replace('todo', 'lost')
        with patch.object(FUZZ, 'cmark_gfm_render', return_value=plain):
            with self.assertRaises(FUZZ.DifferentialMismatch):
                FUZZ.check_markdown_differential(record, Path('oracle'), 5)
        record['html'] = plain
        record['tasklist_projection_limits'] = []
        with patch.object(FUZZ, 'cmark_gfm_render') as oracle:
            with self.assertRaises(ValueError):
                FUZZ.check_markdown_differential(record, Path('oracle'), 5)
            oracle.assert_not_called()

    def test_task_projection_extra_checkbox_without_source_certificate_fails(self):
        plain = '<ul>\n<li>[ ] todo</li>\n</ul>\n'
        checked = '<input disabled="" type="checkbox"> '
        full = plain.replace('[ ] ', checked)
        source = '- [ ] todo\n'
        self.assertEqual(FUZZ.tasklist_projection_limit_reasons(source), [])
        self.assertTrue(FUZZ.check_tasklist_projection(plain, full, source))
        for wrong in (full.replace('todo', checked + 'todo'),
                      full.replace('</li>', '</li>\n<li>' + checked + 'invented</li>'),
                      full.replace('todo', 'lost'), plain):
            with self.assertRaises(FUZZ.DifferentialMismatch):
                FUZZ.check_tasklist_projection(plain, wrong, source)
        source = '- - [x] done\n'
        plain = '<ul>\n<li>\n<ul>\n<li>[x] done</li>\n</ul>\n</li>\n</ul>\n'
        full = plain.replace('[x] ', '<input checked="" disabled="" type="checkbox"> ')
        full = full.replace('done', checked + 'invented done')
        record = {'kind': 'markdown', 'profile': 'tasklist-reference-limited',
                  'source': source, 'html': plain, 'full_html': full}
        with patch.object(FUZZ, 'cmark_gfm_render') as oracle:
            with self.assertRaises(FUZZ.DifferentialMismatch):
                FUZZ.check_markdown_differential(record, Path('oracle'), 5)
            oracle.assert_not_called()

    def test_empty_task_separator_loss_is_certified_from_raw_prefixes(self):
        reason = 'neutral empty-task HTML loses the required trailing separator'
        for newline in ['\n', '\r\n', '\r']:
            for source in (f'- [x] {newline}', f'> - [ ]\t{newline}',
                           f'- [ ] todo{newline}- [x] {newline}- [x] done{newline}'):
                self.assertIn(reason, FUZZ.tasklist_projection_limit_reasons(source))
            for source in (f'- [x]{newline}', f'- [x]{newline}  todo{newline}',
                           f'- literal [x] {newline}', f'- [xx] {newline}',
                           f'- [x]\v{newline}', f'- [x]\f{newline}'):
                self.assertNotIn(reason, FUZZ.tasklist_projection_limit_reasons(source))
        source = '- [ ] todo\n- [x] \n- [x] done\n'
        plain = '<ul>\n<li>[ ] todo</li>\n<li>[x]</li>\n<li>[x] done</li>\n</ul>\n'
        full = ('<ul>\n<li><input disabled="" type="checkbox"> todo</li>\n'
                '<li><input checked="" disabled="" type="checkbox"> </li>\n'
                '<li><input checked="" disabled="" type="checkbox"> done</li>\n</ul>\n')
        record = {'kind': 'markdown', 'profile': 'tasklist-reference-limited',
                  'source': source, 'html': plain, 'full_html': full}
        counts = {'reference_profile_exclusions': 0, 'tasklist_projection_checks': 0,
                  'tasklist_projection_unavailable': 0, 'profile_exclusions': []}
        with patch.object(FUZZ, 'cmark_gfm_render', return_value=plain):
            self.assertTrue(FUZZ.check_markdown_differential(record, Path('oracle'), 5, counts))
        self.assertEqual(counts['tasklist_projection_unavailable'], 1)
        self.assertEqual(counts['tasklist_projection_checks'], 0)
        self.assertIn(reason, counts['profile_exclusions'][0]['projection_unavailable_reasons'])
        for changed in (plain.replace('todo', 'lost'), plain.replace('<li>[x]</li>\n', '')):
            record['html'] = changed
            with patch.object(FUZZ, 'cmark_gfm_render', return_value=plain):
                with self.assertRaises(FUZZ.DifferentialMismatch):
                    FUZZ.check_markdown_differential(record, Path('oracle'), 5)

    def test_limited_tasklist_profile_retains_all_other_extensions(self):
        with patch.object(FUZZ.subprocess, 'run', return_value=subprocess.CompletedProcess([], 0, b'', b'')) as run:
            FUZZ.cmark_gfm_render(Path('cmark-gfm'), '- - [x] d\n', 5, tasklists=False)
        command = run.call_args.args[0]
        self.assertNotIn('tasklist', command)
        for extension in ('table', 'strikethrough', 'autolink', 'tagfilter'):
            self.assertIn(extension, command)

    def test_limited_profile_is_counted_and_keeps_exact_evidence(self):
        source = '- - [x] done\n'
        plain = '<ul>\n<li>\n<ul>\n<li>[x] done</li>\n</ul>\n</li>\n</ul>\n'
        full = plain.replace('[x] ', '<input checked="" disabled="" type="checkbox"> ')
        record = {'kind': 'markdown', 'profile': 'tasklist-reference-limited',
                  'source': source, 'html': plain, 'full_html': full}
        counts = {'reference_profile_exclusions': 0, 'tasklist_projection_checks': 0,
                  'tasklist_projection_unavailable': 0, 'profile_exclusions': []}
        with patch.object(FUZZ, 'cmark_gfm_render', return_value=plain) as oracle:
            self.assertTrue(FUZZ.check_markdown_differential(record, Path('cmark-gfm'), 5, counts))
        self.assertFalse(oracle.call_args_list[0].kwargs['tasklists'])
        self.assertEqual(oracle.call_count, 2)
        self.assertEqual(counts['reference_profile_exclusions'], 1)
        self.assertEqual(counts['tasklist_projection_checks'], 1)
        self.assertEqual(counts['profile_exclusions'][0]['source'], source)
        self.assertTrue(counts['profile_exclusions'][0]['projection_checked'])
        record['profile'] = 'gfm'
        with patch.object(FUZZ, 'cmark_gfm_render') as oracle:
            with self.assertRaises(ValueError):
                FUZZ.check_markdown_differential(record, Path('cmark-gfm'), 5, counts)
            oracle.assert_not_called()

    def test_cmark_uses_the_same_double_tilde_gfm_profile(self):
        with patch.object(FUZZ.subprocess, 'run', return_value=subprocess.CompletedProcess([], 0, b'<p>~tea~</p>\n', b'')) as run:
            self.assertEqual(FUZZ.cmark_gfm_render(Path('cmark-gfm'), '~tea~', 5), '<p>~tea~</p>\n')
        command = run.call_args.args[0]
        self.assertIn('--strikethrough-double-tilde', command)
        self.assertIn('--unsafe', command)
        for extension in FUZZ.CMARK_GFM_EXTENSIONS:
            self.assertIn(extension, command)

    def test_markdown_differential_mismatch_is_reported_precisely(self):
        record = {'kind': 'markdown', 'source': '# Tea\n', 'html': '<h1>Tea</h1>\n'}
        with patch.object(FUZZ, 'cmark_gfm_render', return_value='<h1>tea</h1>'):
            with self.assertRaisesRegex(ValueError, r'markdown differential mismatch'):
                FUZZ.check_markdown_differential(record, Path('unused-cmark-gfm'), 5)

    def test_emphasis_patch_rejects_missing_repeated_and_modified_preimages(self):
        source = '\n'.join(before for before, _ in FUZZ.CMARK_EMPHASIS_PATCHES)
        fixed = FUZZ.patch_reference_emphasis_floors(source)
        for before, after in FUZZ.CMARK_EMPHASIS_PATCHES:
            self.assertNotIn(before, fixed)
            self.assertIn(after, fixed)
            for invalid in (source.replace(before, '', 1), source + '\n' + before,
                            source.replace(before, before[:len(before) // 2] + 'changed' + before[len(before) // 2:], 1)):
                with self.assertRaises(ValueError):
                    FUZZ.patch_reference_emphasis_floors(invalid)
        with self.assertRaises(ValueError):
            FUZZ.patch_reference_emphasis_floors(fixed)

    def test_emphasis_build_requires_registered_digest_before_io(self):
        with tempfile.TemporaryDirectory() as name:
            root = Path(name)
            (root / 'vendor').mkdir()
            for digest in ('missing', 'wrong'):
                entry = {'name': 'cmark-gfm', 'source_sha256': FUZZ.CMARK_GFM_SOURCE_SHA256,
                         'backtick_cache_patch_sha256': FUZZ.CMARK_BACKTICK_PATCH_SHA256}
                if digest != 'missing':
                    entry['emphasis_floors_patch_sha256'] = digest
                (root / 'vendor/tooling.json').write_text(json.dumps([entry]), encoding='utf-8')
                with patch.object(FUZZ, 'ROOT', root), patch.object(FUZZ.subprocess, 'run') as run:
                    with self.assertRaises(ValueError):
                        FUZZ.ensure_cmark_gfm(corrected_backticks=True, corrected_emphasis=True)
                    run.assert_not_called()
                self.assertFalse((root / '.cache').exists())

    def test_emphasis_reference_retains_original_and_compares_complete_corrected_html(self):
        source = '*a**b** c**'
        raw = '<p>*a<strong>b</strong> c**</p>\n'
        fixed = '<p><em>a<strong>b</strong> c</em>*</p>\n'
        record = {'kind': 'markdown', 'source': source, 'html': fixed, 'full_html': fixed}
        counts = {}
        with patch.object(FUZZ, 'cmark_gfm_render', side_effect=[raw, fixed]) as oracle:
            self.assertTrue(FUZZ.check_markdown_differential(record, Path('raw'), 5, counts,
                corrected_emphasis_binary=Path('combined')))
        self.assertEqual([call.args[0] for call in oracle.call_args_list], [Path('raw'), Path('combined')])
        self.assertEqual(counts['emphasis_reference_disagreements'], 1)
        self.assertEqual(counts['emphasis_reference_cases'][0]['raw_reference_html'], raw)
        self.assertEqual(counts['emphasis_reference_cases'][0]['corrected_reference_html'], fixed)
        for wrong in (raw, fixed.replace(' c', ' lost'), fixed.replace('<strong>b</strong>', '')):
            with patch.object(FUZZ, 'cmark_gfm_render', side_effect=[raw, wrong]):
                with self.assertRaises(FUZZ.DifferentialMismatch):
                    FUZZ.check_markdown_differential(record, Path('raw'), 5,
                        corrected_emphasis_binary=Path('combined'))
        with patch.object(FUZZ, 'cmark_gfm_render') as oracle:
            with self.assertRaises(ValueError):
                FUZZ.check_markdown_differential(record, Path('raw'), 5)
            record['reference_emphasis_profile'] = 'gfm'
            with self.assertRaises(ValueError):
                FUZZ.check_markdown_differential(record, Path('raw'), 5,
                    corrected_emphasis_binary=Path('combined'))
            oracle.assert_not_called()
        with patch.object(FUZZ, 'cmark_gfm_render', return_value='<p>lost</p>\n'):
            with self.assertRaises(ValueError):
                FUZZ.verify_reference_emphasis_correction(Path('ticks'), Path('combined'))

    def test_backtick_cache_patch_requires_exact_unique_preimage(self):
        before, after = FUZZ.CMARK_BACKTICK_BEFORE, FUZZ.CMARK_BACKTICK_AFTER
        self.assertEqual(FUZZ.patch_reference_backtick_cache('prefix\n' + before + '\nsuffix'),
                         'prefix\n' + after + '\nsuffix')
        for source in ('', before + '\n' + before, before.replace('numticks', 'other'), after):
            with self.assertRaises(ValueError):
                FUZZ.patch_reference_backtick_cache(source)

    def test_backtick_build_requires_registered_source_and_correction_before_io(self):
        with tempfile.TemporaryDirectory() as name:
            root = Path(name)
            (root / 'vendor').mkdir()
            for source, correction in [('wrong', FUZZ.CMARK_BACKTICK_PATCH_SHA256),
                                       (FUZZ.CMARK_GFM_SOURCE_SHA256, 'wrong')]:
                (root / 'vendor/tooling.json').write_text(json.dumps([{
                    'name': 'cmark-gfm', 'source_sha256': source,
                    'backtick_cache_patch_sha256': correction}]), encoding='utf-8')
                with patch.object(FUZZ, 'ROOT', root), patch.object(FUZZ.subprocess, 'run') as run:
                    with self.assertRaises(ValueError):
                        FUZZ.ensure_cmark_gfm(corrected_backticks=True)
                    run.assert_not_called()
                self.assertFalse((root / '.cache').exists())

    def test_backtick_reference_is_selected_before_comparison_and_retains_raw_outputs(self):
        source = '[```a ``b`` c ``d``'
        raw = '<p>[```a <code>b</code> c ``d``</p>\n'
        fixed = '<p>[```a <code>b</code> c <code>d</code></p>\n'
        record = {'kind': 'markdown', 'source': source, 'html': fixed, 'full_html': fixed}
        counts = {}
        with patch.object(FUZZ, 'cmark_gfm_render', side_effect=[raw, fixed]) as oracle:
            self.assertTrue(FUZZ.check_markdown_differential(record, Path('raw'), 5, counts,
                corrected_backticks_binary=Path('fixed')))
        self.assertEqual([call.args[0] for call in oracle.call_args_list], [Path('raw'), Path('fixed')])
        self.assertEqual(counts['backtick_reference_checks'], 1)
        self.assertEqual(counts['backtick_reference_disagreements'], 1)
        self.assertEqual(counts['backtick_reference_cases'][0]['raw_reference_html'], raw)
        self.assertEqual(counts['backtick_reference_cases'][0]['corrected_reference_html'], fixed)
        for wrong in (raw, fixed.replace(' c ', ' lost '), fixed.replace('<code>d</code>', '')):
            with patch.object(FUZZ, 'cmark_gfm_render', side_effect=[raw, wrong]):
                with self.assertRaises(FUZZ.DifferentialMismatch):
                    FUZZ.check_markdown_differential(record, Path('raw'), 5,
                        corrected_backticks_binary=Path('fixed'))
        with patch.object(FUZZ, 'cmark_gfm_render') as oracle:
            with self.assertRaises(ValueError):
                FUZZ.check_markdown_differential(record, Path('raw'), 5)
            record['reference_backtick_profile'] = 'gfm'
            with self.assertRaises(ValueError):
                FUZZ.check_markdown_differential(record, Path('raw'), 5,
                    corrected_backticks_binary=Path('fixed'))
            oracle.assert_not_called()

    def test_reference_backtick_execution_controls_fail_on_changed_content(self):
        raw, fixed = Path('raw'), Path('fixed')
        with patch.object(FUZZ, 'cmark_gfm_render', return_value='<p>lost</p>\n'):
            with self.assertRaises(ValueError):
                FUZZ.verify_reference_backtick_correction(raw, fixed)

    def test_destination_balance_patch_is_exact_registered_and_source_selected(self):
        patched = FUZZ.patch_reference_destination_balance(FUZZ.CMARK_DESTINATION_BEFORE)
        self.assertEqual(patched, FUZZ.CMARK_DESTINATION_AFTER)
        self.assertIn('if (i >= input->len || nb_p != 0)', patched)
        for source in ('', FUZZ.CMARK_DESTINATION_BEFORE * 2,
                       FUZZ.CMARK_DESTINATION_BEFORE.replace('size_t nb_p = 0;', 'size_t nb_p = 1;')):
            with self.assertRaises(ValueError):
                FUZZ.patch_reference_destination_balance(source)
        inventory = json.loads((FUZZ.ROOT / 'vendor/tooling.json').read_text(encoding='utf-8'))
        entry = next(item for item in inventory if item['name'] == 'cmark-gfm')
        self.assertEqual(entry['destination_balance_patch_sha256'], FUZZ.CMARK_DESTINATION_PATCH_SHA256)
        for source in ('> [x]: a(b', '> > [x]: a(b)', '> [x]: a\\(b', '> [x]: <a(b>',
                       '>[te\na]: a(b\n\nother', '- [x]: a(b\n\n[x]: /valid\n\n[x]'):
            self.assertTrue(FUZZ.destination_reference_profile(source), repr(source))
        for source in ('plain (text)', '> [x]: /valid', '[x]: a(b', '[x]: a(b\n\nother'):
            self.assertFalse(FUZZ.destination_reference_profile(source), repr(source))
        bad = [dict(item, destination_balance_patch_sha256='bad') if item['name'] == 'cmark-gfm' else item for item in inventory]
        with patch.object(FUZZ.Path, 'read_text', return_value=json.dumps(bad)):
            with self.assertRaises(ValueError):
                FUZZ.ensure_cmark_gfm(corrected_destinations=True)
        record = {'kind': 'markdown', 'source': '> [x]: a(b', 'html': '<blockquote>\n<p>[x]: a(b</p>\n</blockquote>\n'}
        with patch.object(FUZZ, 'cmark_gfm_render') as oracle:
            with self.assertRaises(ValueError):
                FUZZ.check_markdown_differential(record, Path('raw'), 5)
            oracle.assert_not_called()

    def test_destination_corrected_oracle_preserves_whole_document_and_raw_answers(self):
        source = '>[te\na]: /te(a "Tea~"\n\nother\n'
        raw = '<blockquote>\n</blockquote>\n<p>other</p>\n'
        fixed = '<blockquote>\n<p>[te\na]: /te(a &quot;Tea~&quot;</p>\n</blockquote>\n<p>other</p>\n'
        record = {'kind': 'markdown', 'source': source, 'html': fixed, 'full_html': fixed}
        counts = {}
        with patch.object(FUZZ, 'cmark_gfm_render', side_effect=[raw, fixed]) as oracle:
            self.assertTrue(FUZZ.check_markdown_differential(record, Path('raw'), 5, counts,
                corrected_destinations_binary=Path('balanced')))
        self.assertEqual(oracle.call_args_list[0].args[1], source)
        self.assertEqual(oracle.call_args_list[1].args[1], source)
        self.assertEqual(counts['destination_reference_checks'], 1)
        self.assertEqual(counts['destination_reference_disagreements'], 1)
        case = counts['destination_reference_cases'][0]
        self.assertEqual(case['raw_reference_html'], raw)
        self.assertEqual(case['corrected_reference_html'], fixed)
        for wrong in (fixed.replace('other', 'lost'), raw, fixed.replace('Tea~', 'Tea')):
            record['html'] = wrong
            with patch.object(FUZZ, 'cmark_gfm_render', side_effect=[raw, fixed]):
                with self.assertRaises(FUZZ.DifferentialMismatch):
                    FUZZ.check_markdown_differential(record, Path('raw'), 5,
                        corrected_destinations_binary=Path('balanced'))
        record['reference_destination_profile'] = 'gfm'
        with patch.object(FUZZ, 'cmark_gfm_render') as oracle:
            with self.assertRaises(ValueError):
                FUZZ.check_markdown_differential(record, Path('raw'), 5,
                    corrected_destinations_binary=Path('balanced'))
            oracle.assert_not_called()

    def test_unbalanced_reference_destination_can_follow_one_physical_newline(self):
        for newline in ('\n', '\r\n', '\r'):
            for padding in ('', ' ', '\t'):
                for destination in ('(', 'a(b', 'a(b(c)'):
                    source = '[tetea]:' + newline + padding + destination
                    self.assertTrue(FUZZ.unbalanced_reference_paragraph(source), source)
                    self.assertEqual(FUZZ.reference_admission_profile(source), 'literal-unbalanced-destination')
            for destination in ('a(b)', r'a\(b', '<a(b>', 'a)b', 'plain'):
                source = '[tetea]:' + newline + destination
                self.assertFalse(FUZZ.unbalanced_reference_paragraph(source), source)
            for blank in (newline + newline, newline + ' \t' + newline):
                self.assertFalse(FUZZ.unbalanced_reference_paragraph('[tetea]:' + blank + '('))
            self.assertFalse(FUZZ.escaped_terminal_reference_title('[tea]:' + newline + '/tea "Tea\\"'))
        source = '[tetea]:\n('
        ours = '<p>[tetea]:\n(</p>\n'
        guarded = '<p>' + FUZZ._REFERENCE_GUARD + '[tetea]:\n(</p>\n'
        counts = {}
        record = {'kind': 'markdown', 'source': source, 'html': ours}
        with patch.object(FUZZ, 'cmark_gfm_render', side_effect=['', guarded]):
            self.assertTrue(FUZZ.check_markdown_differential(record, Path('raw'), 5, counts))
        case = counts['reference_admission_cases'][0]
        self.assertEqual(case['raw_reference_html'], '')
        self.assertEqual(case['guarded_reference_html'], guarded)
        self.assertEqual(counts['reference_admission_disagreements'], 1)
        for wrong in (guarded.replace('tetea', 'lost'), guarded.replace('(</p>', '</p>'),
                      guarded.replace(FUZZ._REFERENCE_GUARD, 'other')):
            with patch.object(FUZZ, 'cmark_gfm_render', side_effect=['', wrong]):
                with self.assertRaises(ValueError):
                    FUZZ.check_markdown_differential(record, Path('raw'), 5)

    def test_literal_reference_selection_requires_a_primary_grammar_certificate(self):
        for source in ('[t-\r\n]: .[(\\\"\n', '[名\n字]: a(b\n', '[t-café: -]: (.\"]!\n', '[名]: a(b\n', '[a\\]b]: a(b\n', '[t-]: .(\n\n', '[t\\-]: .(\\"]!\n\n|',
                       '[tea]: /(tea "Tea\\"', '[x]: ((a)\r\n\r\nnext',
                       '[x]: a(b\ncontinued', '[t-]:]:(<'):
            self.assertTrue(FUZZ.unbalanced_reference_paragraph(source), repr(source))
        for source in ('[x]: a(b)\n', '[x]: a\\(b\n', '[x]: <a(b>\n',
                       '[x]: a)b(\n', '[x]: a\n',
                       '[a\n\nb]: a(b\n', '[a\x01b]: a(b\n', '[a[b]: a(b\n', '[a]b]: a(b\n', '[a\0]: a(b\n', 'before\n[x]: a(b\n', ' [x]: a(b\n', '[x]: a(\0\n',
                       '[x]: ' + FUZZ._REFERENCE_GUARD + '(\n'):
            self.assertFalse(FUZZ.unbalanced_reference_paragraph(source), repr(source))

    def test_email_counter_patch_requires_registration_unique_preimage_and_raw_selection(self):
        self.assertEqual(FUZZ.patch_reference_email_counter(FUZZ.CMARK_EMAIL_BEFORE), FUZZ.CMARK_EMAIL_AFTER)
        self.assertIn('np = 0;', FUZZ.CMARK_EMAIL_AFTER)
        for source in ('', FUZZ.CMARK_EMAIL_BEFORE * 2, FUZZ.CMARK_EMAIL_BEFORE.replace('rewind = 0', 'rewind = 1')):
            with self.assertRaises(ValueError):
                FUZZ.patch_reference_email_counter(source)
        inventory = json.loads((FUZZ.ROOT / 'vendor/tooling.json').read_text(encoding='utf-8'))
        entry = next(item for item in inventory if item['name'] == 'cmark-gfm')
        self.assertEqual(entry['email_counter_patch_sha256'], FUZZ.CMARK_EMAIL_PATCH_SHA256)
        bad = [dict(item, email_counter_patch_sha256='bad') if item['name'] == 'cmark-gfm' else item for item in inventory]
        with patch.object(FUZZ.Path, 'read_text', return_value=json.dumps(bad)):
            with self.assertRaises(ValueError):
                FUZZ.ensure_cmark_gfm(corrected_emails=True)
        for source in ('a@b.c@d', 'a&#64;b.c&#64;d', 'a&#x40;b.c&#x40;d', 'a&commat;b.c&commat;d',
                       'a&commat;b.c&commat;d&period;e', '`a@b.c@d`', '[label](mailto:a@b)', '<a@b>'):
            self.assertTrue(FUZZ.email_reference_profile(source), source)
        for source in ('ordinary', '[label](target)', '<script>"text"</script>'):
            self.assertFalse(FUZZ.email_reference_profile(source), source)
        for supplied in ({}, {'corrected_emails_binary': Path('email')}):
            with patch.object(FUZZ, 'cmark_gfm_render') as oracle:
                with self.assertRaises(ValueError):
                    FUZZ.check_markdown_differential({'kind': 'markdown', 'source': 'a@b.c@d', 'html': ''}, Path('raw'), 5, **supplied)
                oracle.assert_not_called()

    def test_email_corrected_oracle_preserves_complete_answers_and_rejects_content_loss(self):
        source = '[~~strik:e~~ www.example.org\\ user@exampleww.example.orgser@exam\n]'
        raw = '<p>[<del>strik:e</del> www.example.org\\ user@<a href="mailto:exampleww.example.orgser@exam">exampleww.example.orgser@exam</a>\n]</p>\n'
        fixed = '<p>[<del>strik:e</del> www.example.org\\ user@exampleww.example.orgser@exam\n]</p>\n'
        record = {'kind': 'markdown', 'source': source, 'html': fixed, 'full_html': fixed}
        counts = {}
        with patch.object(FUZZ, 'cmark_gfm_render', side_effect=[raw, raw, fixed]):
            self.assertTrue(FUZZ.check_markdown_differential(record, Path('raw'), 5, counts,
                corrected_emphasis_binary=Path('emphasis'), corrected_destinations_binary=Path('destination'),
                corrected_emails_binary=Path('email')))
        self.assertEqual(counts['email_reference_disagreements'], 1)
        case = counts['email_reference_cases'][0]
        self.assertEqual(case['raw_reference_html'], raw)
        self.assertEqual(case['reference_html_before_correction'], raw)
        self.assertEqual(case['corrected_reference_html'], fixed)
        for wrong in (raw, fixed.replace('user@', ''), fixed.replace('strik:e', 'lost')):
            record['html'] = wrong
            with patch.object(FUZZ, 'cmark_gfm_render', side_effect=[raw, raw, fixed]):
                with self.assertRaises(FUZZ.DifferentialMismatch):
                    FUZZ.check_markdown_differential(record, Path('raw'), 5,
                        corrected_emphasis_binary=Path('emphasis'), corrected_destinations_binary=Path('destination'),
                        corrected_emails_binary=Path('email'))
        record['reference_email_profile'] = 'gfm'
        with patch.object(FUZZ, 'cmark_gfm_render') as oracle:
            with self.assertRaises(ValueError):
                FUZZ.check_markdown_differential(record, Path('raw'), 5,
                    corrected_destinations_binary=Path('destination'), corrected_emails_binary=Path('email'))
            oracle.assert_not_called()

    def test_email_attribution_uses_immediately_preceding_destination_binary(self):
        source = '[x]: a(b user@a.b'
        fixed = '<p>[x]: a(b <a href="mailto:user@a.b">user@a.b</a></p>\n'
        guarded = '<p>' + FUZZ._REFERENCE_GUARD + fixed[len('<p>'):]
        counts = {}
        with patch.object(FUZZ, 'cmark_gfm_render', side_effect=['', fixed, fixed, guarded]) as oracle:
            self.assertTrue(FUZZ.check_markdown_differential(
                {'kind': 'markdown', 'source': source, 'html': fixed}, Path('raw'), 5, counts,
                corrected_destinations_binary=Path('destination'), corrected_emails_binary=Path('email')))
        self.assertEqual(oracle.call_args_list[1].args[0], Path('destination'))
        self.assertEqual(oracle.call_args_list[2].args[0], Path('email'))
        self.assertEqual(counts['email_reference_checks'], 1)
        self.assertEqual(counts['email_reference_disagreements'], 0)
        self.assertEqual(counts['email_reference_cases'], [])
        self.assertEqual(counts['reference_admission_disagreements'], 1)

    def test_paragraph_prefix_certificate_preserves_semantic_whitespace(self):
        source = '- [d]: /x\n a\n'
        raw = '<ul>\n<li> \ta <code> b</code>  c</li>\n</ul>\n'
        projected = '<ul>\n<li>a <code> b</code>  c</li>\n</ul>\n'
        self.assertTrue(FUZZ.generated_paragraph_prefix_reference_profile(source))
        self.assertEqual(FUZZ.project_reference_paragraph_prefix(source, raw), projected)
        for excluded in ('<p> a</p>', '&#32;a', '&#9;a', '&amp;a'):
            self.assertFalse(FUZZ.generated_paragraph_prefix_reference_profile(excluded))
            with self.assertRaises(ValueError):
                FUZZ.project_reference_paragraph_prefix(excluded, '<p> a</p>')
        code = '<pre><code> a\n\tb\n</code></pre>\n'
        self.assertEqual(FUZZ.project_reference_paragraph_prefix(source, code), code)
        record = {'kind': 'markdown', 'source': source, 'html': projected}
        counts = {}
        with patch.object(FUZZ, 'cmark_gfm_render', return_value=raw):
            self.assertTrue(FUZZ.check_markdown_differential(record, Path('cmark'), 5, counts))
        self.assertEqual(counts['paragraph_prefix_reference_disagreements'], 1)
        case = counts['paragraph_prefix_reference_cases'][0]
        self.assertEqual(case['raw_reference_html'], raw)
        self.assertEqual(case['projected_reference_html'], projected)
        for wrong in (projected.replace('a ', 'lost '), projected.replace('  c', ' c'),
                      projected.replace('<code> b', '<code>b'), projected.replace('<li>', '<p>')):
            record['html'] = wrong
            with patch.object(FUZZ, 'cmark_gfm_render', return_value=raw):
                with self.assertRaises(FUZZ.DifferentialMismatch):
                    FUZZ.check_markdown_differential(record, Path('cmark'), 5)
        record['reference_paragraph_prefix_profile'] = 'gfm'
        with patch.object(FUZZ, 'cmark_gfm_render') as oracle:
            with self.assertRaises(ValueError):
                FUZZ.check_markdown_differential(record, Path('cmark'), 5)
            oracle.assert_not_called()

    def test_tasks_after_definitions_select_neutral_reference_before_execution(self):
        for ending in ('\n', '\r\n', '\r'):
            for prefix, continuation in (('- ', ' '), ('> - ', '>  '), ('- - ', '   ')):
                for marker in ('[ ]', '[x]', '[X]'):
                    source = prefix + '[d]: /x' + ending + continuation + marker + ' todo' + ending
                    self.assertIn('task marker can follow a reference definition in its first paragraph',
                                  FUZZ.tasklist_reference_limit_reasons(source))
        self.assertFalse(FUZZ.tasklist_reference_limited('- [d]: /x\n ordinary\n'))
        self.assertFalse(FUZZ.tasklist_reference_limited('- [x] todo\n'))
        source = '- [d]: /x\n [x] todo\n'
        plain = '<ul>\n<li>[x] todo</li>\n</ul>\n'
        full = '<ul>\n<li><input type="checkbox" checked="" disabled="" /> todo</li>\n</ul>\n'
        record = {'kind': 'markdown', 'source': source, 'profile': 'tasklist-reference-limited',
                  'html': plain, 'full_html': full}
        counts = {'reference_profile_exclusions': 0, 'tasklist_projection_checks': 0,
                  'tasklist_projection_unavailable': 0, 'profile_exclusions': []}
        with patch.object(FUZZ, 'cmark_gfm_render', return_value=plain):
            self.assertTrue(FUZZ.check_markdown_differential(record, Path('cmark'), 5, counts))
        self.assertEqual(counts['tasklist_projection_checks'], 1)
        record['full_html'] = full.replace('todo', 'lost')
        with patch.object(FUZZ, 'cmark_gfm_render') as oracle:
            with self.assertRaises(FUZZ.DifferentialMismatch):
                FUZZ.check_markdown_differential(record, Path('cmark'), 5)
            oracle.assert_not_called()

    def test_escaped_terminal_reference_title_certificate_is_narrow(self):
        for opener, closer in (('"', '"'), ("'", "'"), ('(', ')')):
            for ending in ('\n', '\r\n', '\r', ''):
                for slashes in (1, 3, 5):
                    source = '[tea]: /tea ' + opener + 'Tea' + '\\' * slashes + closer + ending
                    self.assertTrue(FUZZ.escaped_terminal_reference_title(source), repr(source))
                    self.assertEqual(FUZZ.reference_admission_profile(source), 'literal-escaped-title-closer')
                for slashes in (0, 2, 4):
                    source = '[tea]: /tea ' + opener + 'Tea' + '\\' * slashes + closer + ending
                    self.assertFalse(FUZZ.escaped_terminal_reference_title(source), repr(source))
            for suffix in (closer + '\n', '\n' + closer, '\nother', '\n[other]: /url', '\n> quote'):
                source = '[tea]: /tea ' + opener + 'Tea\\' + closer + suffix
                self.assertFalse(FUZZ.escaped_terminal_reference_title(source), repr(source))
        for source in ('before\n[tea]: /tea "Tea\\"\n', '[tea]: /tea\n"Tea\\"\n',
                       '[tea]: /tea "Tea\ncontinued\\"\n', '[tea]: /tea "Tea',
                       '[tea]: /tea "Tea\\" ' + FUZZ._REFERENCE_GUARD):
            self.assertFalse(FUZZ.escaped_terminal_reference_title(source), repr(source))

    def test_escaped_title_guard_retains_raw_answers_and_all_content(self):
        source = '[tea]: /tea "Tea\\"\n'
        expected = '<p>[tea]: /tea &quot;Tea&quot;</p>\n'
        record = {'kind': 'markdown', 'source': source, 'html': expected}
        guarded = '<p>' + FUZZ._REFERENCE_GUARD + '[tea]: /tea &quot;Tea&quot;</p>\n'
        counts = {}
        with patch.object(FUZZ, 'cmark_gfm_render', side_effect=['', guarded]) as oracle:
            self.assertTrue(FUZZ.check_markdown_differential(record, Path('cmark'), 5, counts))
            self.assertEqual(oracle.call_args_list[1].args[1], FUZZ._REFERENCE_GUARD + source)
        case = counts['reference_admission_cases'][0]
        self.assertEqual(case['raw_reference_html'], '')
        self.assertEqual(case['guarded_reference_html'], guarded)
        self.assertEqual(case['admission_profile'], 'literal-escaped-title-closer')
        for wrong in (guarded.replace('Tea', 'lost'), guarded.replace(FUZZ._REFERENCE_GUARD, 'other')):
            with patch.object(FUZZ, 'cmark_gfm_render', side_effect=['', wrong]):
                with self.assertRaises((ValueError, FUZZ.DifferentialMismatch)):
                    FUZZ.check_markdown_differential(record, Path('cmark'), 5)
        record['reference_admission_profile'] = 'gfm'
        with patch.object(FUZZ, 'cmark_gfm_render') as oracle:
            with self.assertRaises(ValueError):
                FUZZ.check_markdown_differential(record, Path('cmark'), 5)
            oracle.assert_not_called()

    def test_reference_certificate_accepts_literal_nonpunctuation_backslashes(self):
        for label in (r't\ea', 't\\éa', 't\\ a', 't\\\\a'):
            for ending in ('\n', '\r\n', '\r'):
                source = '[' + label + ']: /te]([tea]' + ending
                self.assertTrue(FUZZ.unbalanced_reference_paragraph(source), repr(source))
        for source in (r'[t\ea]x]: a(b', '[t\\a]: a(b', '[t\\\n\na]: a(b'):
            self.assertFalse(FUZZ.unbalanced_reference_paragraph(source), repr(source))
        self.assertFalse(FUZZ.unbalanced_reference_paragraph(r'[t\ea]: a(b)'))

    def test_loose_task_source_profile_preserves_paragraph_placement(self):
        reason = 'physical blank line can make a task list loose'
        for ending in ('\n', '\r\n', '\r'):
            source = '- [x] todo' + ending + ending + '- [xx] done'
            self.assertIn(reason, FUZZ.tasklist_reference_limit_reasons(source))
        for source in ('- [x] todo\n', '- [x] todo\n- [ ] done\n', 'ordinary\n\ntext'):
            self.assertNotIn(reason, FUZZ.tasklist_reference_limit_reasons(source))
        source = '- [x] todo\n\n- [xx] done'
        plain = '<ul>\n<li>\n<p>[x] todo</p>\n</li>\n<li>\n<p>[xx] done</p>\n</li>\n</ul>\n'
        full = plain.replace('[x]', '<input checked="" disabled="" type="checkbox">')
        self.assertTrue(FUZZ.check_tasklist_projection(plain, full, source))
        outside = full.replace('<p><input checked="" disabled="" type="checkbox"> todo</p>',
                               '<input checked="" disabled="" type="checkbox"> <p>todo</p>')
        with self.assertRaises(FUZZ.DifferentialMismatch):
            FUZZ.check_tasklist_projection(plain, outside, source)
        record = {'kind': 'markdown', 'source': source, 'profile': 'tasklist-reference-limited',
                  'html': plain, 'full_html': full}
        counts = {'reference_profile_exclusions': 0, 'tasklist_projection_checks': 0,
                  'tasklist_projection_unavailable': 0, 'profile_exclusions': []}
        with patch.object(FUZZ, 'cmark_gfm_render', side_effect=[plain, outside]) as oracle:
            self.assertTrue(FUZZ.check_markdown_differential(record, Path('cmark'), 5, counts))
        self.assertEqual(counts['tasklist_projection_checks'], 1)
        self.assertEqual(counts['profile_exclusions'][0]['raw_reference_html'], outside)

    def test_generated_hardbreak_certificate_excludes_raw_html_and_entities(self):
        for source in ('- a\\\n b\n', '> - a  \r\n>  b', 'a\\\rb'):
            self.assertTrue(FUZZ.generated_hardbreak_reference_profile(source))
        for source in ('a\nb', '- a\\\n <br> b', '- a\\\n &#32;b', '- a\\\n &#9;b'):
            self.assertFalse(FUZZ.generated_hardbreak_reference_profile(source))
            with self.assertRaises(ValueError):
                FUZZ.project_reference_hardbreaks(source, '<p>a<br>\n b</p>')

    def test_generated_hardbreak_projection_changes_only_following_physical_indent(self):
        source = '- a\\\n b\n'
        raw = '<ul>\n<li>a<br />\n \tb <code>a  b</code>  c</li>\n</ul>\n'
        expected = '<ul>\n<li>a<br>\nb <code>a  b</code>  c</li>\n</ul>\n'
        self.assertEqual(FUZZ.project_reference_hardbreaks(source, raw), expected)
        code = '<pre><code>a\\\n b\n</code></pre>\n'
        self.assertEqual(FUZZ.project_reference_hardbreaks(source, code), code)
        record = {'kind': 'markdown', 'source': source, 'html': expected, 'full_html': expected}
        counts = {}
        with patch.object(FUZZ, 'cmark_gfm_render', return_value=raw):
            self.assertTrue(FUZZ.check_markdown_differential(record, Path('cmark-gfm'), 5, counts))
        self.assertEqual(counts['hardbreak_reference_disagreements'], 1)
        self.assertEqual(counts['hardbreak_reference_cases'][0]['raw_reference_html'], raw)
        for lost in (expected.replace('b <code>', 'lost <code>'), expected.replace('  c', ' c'), expected.replace('<br>\n', '')):
            record['html'] = lost
            with patch.object(FUZZ, 'cmark_gfm_render', return_value=raw):
                with self.assertRaises(FUZZ.DifferentialMismatch):
                    FUZZ.check_markdown_differential(record, Path('cmark-gfm'), 5)
        record['html'] = expected
        record['reference_hardbreak_profile'] = 'gfm'
        with patch.object(FUZZ, 'cmark_gfm_render') as oracle:
            with self.assertRaises(ValueError):
                FUZZ.check_markdown_differential(record, Path('cmark-gfm'), 5)
            oracle.assert_not_called()

    def test_markdown_declared_stops_are_closed_and_separate_from_comparisons(self):
        counts = {}
        for stage in ('parse', 'render', 'neutral-render'):
            for reason, resource in (('BudgetExceeded', 'Work'), ('BudgetExceeded', 'ScratchBytes'), ('Cancelled', None)):
                record = {'kind': 'resource-stop', 'source': 'source', 'stage': stage,
                          'reason': reason, 'resource': resource}
                FUZZ.check_markdown_stop(record, 'source', counts)
        self.assertEqual(counts['resource_non_results'], 9)
        self.assertEqual(sum(counts['resource_non_result_reasons'].values()), 9)
        self.assertNotIn('differential_checks', counts)
        valid = {'kind': 'resource-stop', 'source': 'source', 'stage': 'parse',
                 'reason': 'BudgetExceeded', 'resource': 'Work'}
        for wrong in ({**valid, 'html': 'invented success'}, {**valid, 'reason': 'InvalidInput'},
                      {**valid, 'reason': 'NoCandidate'}, {**valid, 'resource': 'unknown'}, {**valid, 'stage': []},
                      {**valid, 'reason': []}, {**valid, 'resource': []},
                      {**valid, 'source': []},
                      {**valid, 'stage': 'unknown'}, {**valid, 'source': 'different'},
                      {**valid, 'reason': 'Cancelled'}, {key: value for key, value in valid.items() if key != 'resource'}):
            with self.assertRaises(ValueError):
                FUZZ.check_markdown_stop(wrong, 'source', counts)
        self.assertEqual(counts['resource_non_results'], 9)

    def test_markdown_protocol_rejects_missing_duplicate_and_contradictory_records(self):
        stop = {'kind': 'resource-stop', 'source': 'source', 'stage': 'render',
                'reason': 'Cancelled', 'resource': None}
        html = {'kind': 'markdown', 'source': 'source', 'profile': 'gfm',
                'html': '<p>source</p>', 'full_html': '<p>source</p>'}
        for records in ([], [stop, stop], [stop, html], [html, stop], [html, html]):
            output = b'ok\n' + b''.join(json.dumps(record).encode() + b'\n' for record in records)
            result = subprocess.CompletedProcess([], 0, output, b'')
            with tempfile.TemporaryDirectory() as directory:
                with patch.object(FUZZ.subprocess, 'run', return_value=result):
                    with patch.object(FUZZ, 'check_differential') as compare:
                        with self.assertRaises(ValueError):
                            FUZZ.probe(Path('probe'), b'source', Path(directory), 1, 'markdown', {}, {})
                        compare.assert_not_called()

    def test_markdown_stop_protocol_still_extracts_coverage_without_a_false_check(self):
        record = {'kind': 'resource-stop', 'source': 'source', 'stage': 'parse',
                  'reason': 'BudgetExceeded', 'resource': 'Depth'}
        output = b'ok\n' + json.dumps(record).encode() + b'\n'
        document = Coverage().document()
        results = [subprocess.CompletedProcess([], 0, output, b''),
                   subprocess.CompletedProcess([], 0, b'', b''),
                   subprocess.CompletedProcess([], 0, json.dumps(document).encode(), b'')]
        counts = {}
        with tempfile.TemporaryDirectory() as directory:
            with patch.object(FUZZ.subprocess, 'run', side_effect=results):
                with patch.object(FUZZ, 'check_differential') as compare:
                    regions, invalid, checks = FUZZ.probe(Path('probe'), b'source', Path(directory), 1,
                        'markdown', {}, {'llvm-profdata': Path('profdata'), 'llvm-cov': Path('cov')}, counts)
                    compare.assert_not_called()
        self.assertFalse(invalid)
        self.assertEqual(checks, 0)
        self.assertEqual(len(regions), 1)
        self.assertEqual(counts['resource_non_results'], 1)
        self.assertEqual(counts['resource_non_result_cases'][0]['source'], 'source')

    def test_probe_rejects_a_markdown_record_for_a_different_input(self):
        wrong = {'kind': 'markdown', 'profile': 'gfm', 'source': 'changed',
                 'html': '<p>changed</p>\n', 'full_html': '<p>changed</p>\n'}
        result = subprocess.CompletedProcess([], 0, b'ok\n' + json.dumps(wrong).encode() + b'\n', b'')
        with tempfile.TemporaryDirectory() as directory:
            with patch.object(FUZZ.subprocess, 'run', return_value=result):
                with patch.object(FUZZ, 'check_differential') as compare:
                    with self.assertRaisesRegex(ValueError, 'exact admitted input'):
                        FUZZ.probe(Path('probe'), b'original', Path(directory), 5, 'markdown', {}, {})
                    compare.assert_not_called()

    def test_literal_reference_profile_certificate_rejects_misclassified_records(self):
        record = {'kind': 'markdown', 'source': '[x]: a(b\n', 'html': '<p>[x]: a(b</p>\n',
                  'reference_admission_profile': 'gfm'}
        with patch.object(FUZZ, 'cmark_gfm_render') as oracle:
            with self.assertRaises(ValueError):
                FUZZ.check_markdown_differential(record, Path('cmark-gfm'), 5)
            oracle.assert_not_called()
        record['source'] = '[x]: a(b)\n'
        record['reference_admission_profile'] = 'literal-unbalanced-destination'
        with patch.object(FUZZ, 'cmark_gfm_render') as oracle:
            with self.assertRaises(ValueError):
                FUZZ.check_markdown_differential(record, Path('cmark-gfm'), 5)
            oracle.assert_not_called()

    def test_literal_reference_oracle_compares_all_content_and_retains_raw_disagreement(self):
        source = '[t\\-]: .(\\"]!\n\n|\n'
        literal = '<p>[t-]: .(&quot;]!</p>\n<p>|</p>\n'
        guarded = '<p>' + FUZZ._REFERENCE_GUARD + literal[len('<p>'):]
        record = {'kind': 'markdown', 'source': source, 'html': literal}
        counts = {}
        with patch.object(FUZZ, 'cmark_gfm_render', side_effect=['<p>|</p>\n', guarded]) as oracle:
            self.assertTrue(FUZZ.check_markdown_differential(record, Path('cmark-gfm'), 5, counts))
        self.assertEqual(oracle.call_args_list[0].args[1], source)
        self.assertEqual(oracle.call_args_list[1].args[1], FUZZ._REFERENCE_GUARD + source)
        self.assertEqual(counts['reference_admission_projections'], 1)
        self.assertEqual(counts['reference_admission_disagreements'], 1)
        self.assertEqual(counts['reference_admission_cases'][0]['source'], source)
        self.assertEqual(counts['reference_admission_cases'][0]['guarded_reference_html'], guarded)
        self.assertEqual(counts['reference_admission_cases'][0]['raw_reference_html'], '<p>|</p>\n')
        for wrong in (guarded.replace('&quot;', 'changed'), guarded.replace('<p>|</p>', ''),
                      guarded.replace(']!', 'lost')):
            with patch.object(FUZZ, 'cmark_gfm_render', side_effect=['', wrong]):
                with self.assertRaises(FUZZ.DifferentialMismatch):
                    FUZZ.check_markdown_differential(record, Path('cmark-gfm'), 5)
        with patch.object(FUZZ, 'cmark_gfm_render', side_effect=['', literal]):
            with self.assertRaises(ValueError):
                FUZZ.check_markdown_differential(record, Path('cmark-gfm'), 5)

    def test_literal_reference_guard_preserves_heading_table_and_following_blocks(self):
        for source, html, start in (
            ('[x]: a(b\ncontinued\n', '<p>[x]: a(b\ncontinued</p>\n', '<p>'),
            ('[x]: a(b\n---\n', '<h2>[x]: a(b</h2>\n', '<h2>'),
            ('[x]: a(b|c\n|-|-|\n', '<table>\n<thead>\n<tr>\n<th>[x]: a(b</th>\n<th>c</th>\n</tr>\n</thead>\n</table>\n', '<table>\n<thead>\n<tr>\n<th>'),
        ):
            guarded = start + FUZZ._REFERENCE_GUARD + html[len(start):]
            with patch.object(FUZZ, 'cmark_gfm_render', side_effect=['', guarded]):
                self.assertTrue(FUZZ.check_markdown_differential(
                    {'kind': 'markdown', 'source': source, 'html': html}, Path('cmark-gfm'), 5))

    def test_a_record_kind_mismatched_with_its_target_is_rejected(self):
        with self.assertRaises(ValueError):
            FUZZ.check_pattern_differential({'kind': 'markdown'}, 5)
        with self.assertRaises(ValueError):
            FUZZ.check_markdown_differential({'kind': 'pattern'}, Path('unused-cmark-gfm'), 5)
        with self.assertRaises(ValueError):
            FUZZ.check_differential('json', {'kind': 'pattern'}, {}, 5)




if __name__ == '__main__':
    unittest.main()
