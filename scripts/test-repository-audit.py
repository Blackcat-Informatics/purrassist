#!/usr/bin/env python3
# SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
# SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0
"""Verify the dependency-free, network-free, ambient-free audit in
`scripts/check-repository.py` against synthetic metadata and source fixtures."""
import hashlib
import importlib.util
import json
import shutil
import subprocess
import tempfile
import unittest
from pathlib import Path


def _load_module(path: Path, name: str):
    spec = importlib.util.spec_from_file_location(name, path)
    if spec is None or spec.loader is None:
        raise SystemExit(f'could not load module spec for {path}')
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


AUDIT = _load_module(Path(__file__).with_name('check-repository.py'), 'check_repository')


def package(name, version='1.0.0', source=None, publish=None):
    return {'id': f'{name} {version}', 'name': name, 'version': version,
            'source': source, 'publish': publish}


def metadata(packages, edges, members):
    """edges maps a package name to a list of (dependency name, kind) pairs."""
    ids = {p['name']: p['id'] for p in packages}
    nodes = [{'id': p['id'], 'deps': [
        {'pkg': ids[dep], 'dep_kinds': [{'kind': kind, 'target': None}]}
        for dep, kind in edges.get(p['name'], [])]} for p in packages]
    return {'packages': packages, 'workspace_members': [ids[m] for m in members],
            'resolve': {'nodes': nodes}}


REGISTRY = 'registry+https://github.com/rust-lang/crates.io-index'


def clean():
    return metadata([package('purrassist-core'), package('purrassist', publish=None)],
                    {'purrassist': [('purrassist-core', None)]}, ['purrassist-core', 'purrassist'])


class Closure(unittest.TestCase):
    def test_empty_core_closure_passes(self):
        self.assertEqual(AUDIT.check_core_closure(clean()), [])
        self.assertEqual(AUDIT.check_network_denylist(clean()), [])

    def test_any_core_dependency_fails_with_path(self):
        for kind in (None, 'build', 'dev'):
            data = metadata([package('purrassist-core'), package('mid', source=REGISTRY),
                             package('leaf', source=REGISTRY)],
                            {'purrassist-core': [('mid', kind)], 'mid': [('leaf', None)]},
                            ['purrassist-core'])
            errors = AUDIT.check_core_closure(data)
            self.assertEqual(len(errors), 2, kind)
            self.assertIn('purrassist-core -> mid -> leaf', ' '.join(errors))

    def derive_data(self, edges):
        packages = [package('purrassist-core'), package('purrassist-derive'),
                    package('helper', source=REGISTRY), package('leaf', source=REGISTRY)]
        return metadata(packages, edges, ['purrassist-core', 'purrassist-derive'])

    def test_derive_without_dependencies_passes(self):
        self.assertEqual(AUDIT.check_derive_closure(self.derive_data({})), [])

    def test_derive_normal_dependency_fails_with_path(self):
        errors = AUDIT.check_derive_closure(self.derive_data(
            {'purrassist-derive': [('helper', None)], 'helper': [('leaf', None)]}))
        self.assertEqual(len(errors), 2)
        self.assertIn('purrassist-derive -> helper -> leaf', ' '.join(errors))
        self.assertIn('normal and build', errors[0])

    def test_derive_normal_or_build_edge_to_core_fails(self):
        for kind in (None, 'build'):
            errors = AUDIT.check_derive_closure(self.derive_data({'purrassist-derive': [('purrassist-core', kind)]}))
            self.assertEqual(len(errors), 1, kind)
            self.assertIn('purrassist-derive -> purrassist-core', errors[0])

    def test_derive_build_dependency_fails(self):
        errors = AUDIT.check_derive_closure(self.derive_data({'purrassist-derive': [('helper', 'build')]}))
        self.assertEqual(len(errors), 1)
        self.assertIn('purrassist-derive -> helper', errors[0])

    def test_derive_dev_dependency_on_core_passes(self):
        self.assertEqual(AUDIT.check_derive_closure(
            self.derive_data({'purrassist-derive': [('purrassist-core', 'dev')]})), [])

    def test_derive_dev_dependency_on_third_party_fails(self):
        errors = AUDIT.check_derive_closure(self.derive_data(
            {'purrassist-derive': [('purrassist-core', 'dev'), ('helper', 'dev')]}))
        self.assertEqual(len(errors), 1)
        self.assertIn('dev dependencies may reach only purrassist-core', errors[0])
        self.assertIn('purrassist-derive -> helper', errors[0])

    def test_missing_derive_package_fails(self):
        self.assertEqual(len(AUDIT.check_derive_closure(clean())), 1)

    def test_tokio_in_publishable_closure_fails(self):
        data = metadata([package('purrassist-core'), package('purrassist-cli'),
                         package('client', source=REGISTRY), package('tokio', source=REGISTRY)],
                        {'purrassist-cli': [('client', None)], 'client': [('tokio', None)]},
                        ['purrassist-core', 'purrassist-cli'])
        errors = AUDIT.check_network_denylist(data)
        self.assertEqual(len(errors), 1)
        self.assertIn('purrassist-cli', errors[0])
        self.assertIn('tokio', errors[0])
        self.assertIn('purrassist-cli -> client -> tokio', errors[0])

    def test_unpublished_crate_and_dev_edges_are_exempt_from_denylist(self):
        data = metadata([package('tool', publish=[]), package('lib'), package('hyper-util', source=REGISTRY),
                         package('reqwest', source=REGISTRY)],
                        {'tool': [('hyper-util', None)], 'lib': [('reqwest', 'dev')]}, ['tool', 'lib'])
        self.assertEqual(AUDIT.check_network_denylist(data), [])

    def test_denylist_patterns(self):
        for name in ['hyper', 'hyper-util', 'h3-quinn', 'http', 'http-body-util', 'tokio-rustls',
                     'mio', 'openssl-sys', 'curl-sys', 'actix-web', 'quinn-proto', 'warp', 'axum']:
            self.assertTrue(AUDIT.NETWORK_DENYLIST.fullmatch(name), name)
        for name in ['httparse', 'serde', 'regex', 'smolstr']:
            self.assertFalse(AUDIT.NETWORK_DENYLIST.fullmatch(name), name)


class Ambient(unittest.TestCase):
    def scan(self, text):
        return AUDIT.scan_ambient_source('crates/core/src/x.rs', text)

    def test_net_import_fails_with_location(self):
        errors = self.scan('fn a() {}\nuse std::net::TcpStream;\n')
        self.assertTrue(errors)
        self.assertTrue(errors[0].startswith('crates/core/src/x.rs:2:'))

    def test_hashmap_fails(self):
        self.assertTrue(self.scan('fn a() {\n    let m = HashMap::new();\n}\n'))

    def test_grouped_import_fails(self):
        self.assertTrue(self.scan('use std::{fmt, fs};\n'))

    def test_every_token_fails(self):
        for token in ['std::net::X', 'TcpStream', 'TcpListener', 'UdpSocket', 'ToSocketAddrs',
                      'std::process::exit', 'std::fs::read', 'std::env::var', 'SystemTime', 'Instant::now',
                      'RandomState', 'HashMap', 'HashSet', 'DefaultHasher', 'std::thread::spawn',
                      'stdin()', 'stdout()', 'stderr()']:
            self.assertTrue(self.scan(f'let _ = {token};\n'), token)

    def test_comments_pass(self):
        text = ('/// uses Instant\n//! HashMap docs\n// std::fs\n/* std::env\n   TcpStream /* nested */ SystemTime */\n'
                'let x = 1; // HashSet trailing\nlet y = 2;\n')
        self.assertEqual(self.scan(text), [])

    def test_code_after_block_comment_and_strings(self):
        self.assertTrue(self.scan('/* c */ let m = HashMap::new();\n'))
        self.assertTrue(self.scan('let s = "http://x"; let m = HashSet::new();\n'))
        self.assertEqual(self.scan('let s = r#"//"#; let t = Instantiate;\n'), [])

    def test_derive_source_is_scanned_under_the_derive_label(self):
        root = Path(tempfile.mkdtemp())
        self.addCleanup(shutil.rmtree, root)
        for source in ('crates/core/src', 'crates/derive/src'):
            (root / source).mkdir(parents=True)
            (root / source / 'lib.rs').write_text('fn ok() {}\n')
        self.assertEqual(AUDIT.check_ambient_access(root), [])
        (root / 'crates/derive/src/parse.rs').write_text('fn a() {\n    let _ = std::env::var("X");\n}\n')
        errors = AUDIT.check_ambient_access(root)
        self.assertEqual(len(errors), 1)
        self.assertTrue(errors[0].startswith('crates/derive/src/parse.rs:2:'))
        self.assertIn('forbidden in purrassist-derive', errors[0])
        self.assertNotIn('purrassist-core', errors[0])

    def test_core_source_keeps_the_core_label(self):
        errors = AUDIT.scan_ambient_source('crates/core/src/x.rs', 'use std::fs;\n')
        self.assertIn('forbidden in purrassist-core', errors[0])

    def test_current_core_source_is_clean(self):
        self.assertEqual(AUDIT.check_ambient_access(AUDIT.ROOT), [])
        self.assertTrue(list((AUDIT.ROOT / 'crates/derive/src').rglob('*.rs')))


class Inventory(unittest.TestCase):
    def test_core_dev_dependency_fails(self):
        self.assertEqual(AUDIT.check_core_dev_dependencies({'dependencies': {}}), [])
        self.assertTrue(AUDIT.check_core_dev_dependencies({'dev-dependencies': {'serde': '1'}}))
        self.assertTrue(AUDIT.check_core_dev_dependencies(
            {'target': {'cfg(unix)': {'dev-dependencies': {'serde': '1'}}}}))

    def test_extra_third_party_package_fails(self):
        data = metadata([package('purrassist-core'), package('serde', '1.0.0', REGISTRY),
                         package('extra', '0.1.0', REGISTRY)], {}, ['purrassist-core'])
        self.assertEqual(AUDIT.check_dependency_baseline(data, ['serde@1.0.0', 'extra@0.1.0']), [])
        errors = AUDIT.check_dependency_baseline(data, ['serde@1.0.0'])
        self.assertEqual(len(errors), 1)
        self.assertIn('extra@0.1.0', errors[0])
        self.assertIn('maintainer approval', errors[0])


class DeriveExample(unittest.TestCase):
    """`check_derive_example` over a synthetic pair of READMEs."""

    BLOCK = '```rust\nlet x = 1;\n```\n'

    def setUp(self):
        self.root = Path(tempfile.mkdtemp())
        self.addCleanup(shutil.rmtree, self.root)
        (self.root / 'crates/purrassist').mkdir(parents=True)
        self.write('README.md', self.BLOCK)
        self.write('crates/purrassist/README.md', self.BLOCK)

    def write(self, name, block, begin='<!-- derive-example:begin -->\n',
              end='<!-- derive-example:end -->\n'):
        (self.root / name).write_text(f'# title\n\n{begin}{block}{end}\ntext\n')

    def test_identical_copies_pass(self):
        self.assertEqual(AUDIT.check_derive_example(self.root), [])

    def test_drifted_root_copy_fails_naming_both_files(self):
        self.write('README.md', self.BLOCK.replace('1', '2'))
        errors = AUDIT.check_derive_example(self.root)
        self.assertEqual(len(errors), 1)
        self.assertTrue(errors[0].startswith('README.md: derive example differs'))
        self.assertIn('crates/purrassist/README.md', errors[0])

    def test_one_byte_of_whitespace_fails(self):
        self.write('README.md', self.BLOCK.replace('let x', 'let  x'))
        self.assertEqual(len(AUDIT.check_derive_example(self.root)), 1)

    def test_missing_marker_fails(self):
        self.write('README.md', self.BLOCK, begin='')
        errors = AUDIT.check_derive_example(self.root)
        self.assertEqual(len(errors), 1)
        self.assertTrue(errors[0].startswith('README.md: needs exactly one'))

    def test_missing_file_fails(self):
        (self.root / 'README.md').unlink()
        self.assertEqual(AUDIT.check_derive_example(self.root),
                         ['README.md: derive example copy is missing'])


class MarkdownAttribution(unittest.TestCase):
    def setUp(self):
        self.directory = tempfile.TemporaryDirectory()
        self.addCleanup(self.directory.cleanup)
        self.root = Path(self.directory.name)

    def write(self, name, text):
        path = self.root / name
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(text)

    HEADER = ('<!-- SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc. -->\n'
              '<!-- SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0 -->\n')

    def test_headed_markdown_passes(self):
        self.write('docs/A.md', self.HEADER + '# A\n')
        self.assertEqual(AUDIT.check_markdown_attribution(self.root, ['docs/A.md']), [])

    def test_unheaded_markdown_fails_naming_the_file(self):
        self.write('docs/B.md', '# B\n')
        self.assertEqual(AUDIT.check_markdown_attribution(self.root, ['docs/B.md']),
                         ['docs/B.md: missing attribution'])

    def test_one_missing_tag_fails(self):
        self.write('C.md', '<!-- SPDX-License-Identifier: MIT -->\n# C\n')
        self.assertEqual(len(AUDIT.check_markdown_attribution(self.root, ['C.md'])), 1)

    def test_tracked_markdown_is_the_git_index_minus_test_data(self):
        subprocess.run(['git', 'init', '--quiet'], cwd=self.root, check=True)
        for name in ['README.md', 'docs/A.md', 'crates/core/tests/data/x/README.md', 'untracked.md']:
            self.write(name, self.HEADER)
        subprocess.run(['git', 'add', 'README.md', 'docs/A.md', 'crates/core/tests/data/x/README.md'],
                       cwd=self.root, check=True)
        self.assertEqual(AUDIT.tracked_markdown(self.root), ['README.md', 'docs/A.md'])

    def test_current_repository_markdown_is_attributed(self):
        root = Path(__file__).resolve().parent.parent
        names = AUDIT.tracked_markdown(root)
        self.assertIn('README.md', names)
        self.assertEqual(AUDIT.check_markdown_attribution(root, names), [])


class EvaluationManifest(unittest.TestCase):
    """`check_evaluation_manifest` over a synthetic repository tree."""

    def setUp(self):
        self.root = Path(tempfile.mkdtemp())
        self.addCleanup(shutil.rmtree, self.root)
        evaluation = self.root / 'crates/core/tests/data/evaluation'
        evaluation.mkdir(parents=True)
        (self.root / 'input.txt').write_text('input\n')
        (evaluation / 'generate.py').write_text('# generator\n')
        (evaluation / 'cases.json').write_text(json.dumps([{'id': 'a', 'query': 'x'}]))
        source = self.root / 'crates/core/examples/corpus.rs'
        source.parent.mkdir(parents=True)
        source.write_text('fn main() {}\n')
        lines = [{'id': 'a', 'category': 'c', 'input': 'x', 'truth': 'Unresolved'},
                 {'id': 'b', 'category': 'd', 'input': 'x', 'truth': []},
                 {'id': 'c', 'category': 'c', 'input': 'y', 'truth': []}]
        (evaluation / 'corpus.jsonl').write_text(''.join(json.dumps(line) + '\n' for line in lines))
        digest = lambda path: hashlib.sha256(path.read_bytes()).hexdigest()
        self.manifest = {
            'generator': 'crates/core/tests/data/evaluation/generate.py',
            'generator_sha256': digest(evaluation / 'generate.py'),
            'inputs': {'text': 'input.txt', 'text_sha256': digest(self.root / 'input.txt')},
            'outputs': {'cases.json': {'cases': 1, 'sha256': digest(evaluation / 'cases.json')}},
            'generators': {'corpus.jsonl': {
                'sources': {'crates/core/examples/corpus.rs': digest(source)},
                'cases': 3, 'distinct_requests': 2,
                'sha256': digest(evaluation / 'corpus.jsonl')}},
        }
        self.write()

    def write(self):
        path = self.root / 'crates/core/tests/data/evaluation/manifest.json'
        path.write_text(json.dumps(self.manifest, indent=2) + '\n')

    def test_consistent_manifest_passes(self):
        self.assertEqual(AUDIT.check_evaluation_manifest(self.root), [])

    def test_changed_corpus_fails(self):
        path = self.root / 'crates/core/tests/data/evaluation/corpus.jsonl'
        path.write_text(path.read_text().replace('"y"', '"z"'))
        errors = AUDIT.check_evaluation_manifest(self.root)
        self.assertEqual(errors, ['evaluation/corpus.jsonl: corpus digest or census mismatch'])

    def test_census_mismatch_fails(self):
        self.manifest['generators']['corpus.jsonl']['distinct_requests'] = 3
        self.write()
        self.assertEqual(AUDIT.check_evaluation_manifest(self.root),
                         ['evaluation/corpus.jsonl: corpus digest or census mismatch'])

    def test_changed_generator_source_fails(self):
        (self.root / 'crates/core/examples/corpus.rs').write_text('fn main() { changed() }\n')
        self.assertEqual(AUDIT.check_evaluation_manifest(self.root), [
            'evaluation/corpus.jsonl: generator source digest mismatch: crates/core/examples/corpus.rs'])

    def test_missing_corpus_and_sources_fail(self):
        (self.root / 'crates/core/tests/data/evaluation/corpus.jsonl').unlink()
        self.assertEqual(AUDIT.check_evaluation_manifest(self.root),
                         ['evaluation/corpus.jsonl: registered corpus missing'])

    def test_current_repository_manifest_passes(self):
        self.assertEqual(AUDIT.check_evaluation_manifest(AUDIT.ROOT), [])


if __name__ == '__main__':
    unittest.main()
