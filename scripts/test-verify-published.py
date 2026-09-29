#!/usr/bin/env python3
# SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
# SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0
"""Offline unit/integration tests for scripts/verify-published.py.

Builds a throwaway five-crate fixture workspace (`purrassist-core` ->
`purrassist-syntax` -> `purrassist` -> `purrassist-host` -> `purrassist-cli`,
the same names and dependency order as the real release set, but a fixture
version and trivial bodies), packages it with `cargo package --workspace`,
and turns that packaged output into a Cargo `local-registry` fixture
directory (see
https://doc.rust-lang.org/cargo/reference/source-replacement.html#local-registry-sources).
`scripts/verify-published.py --registry-index <that directory>` is then run
against it exactly as it would run against the real crates.io, exercising:
install-from-registry, CLI/MCP transcript comparison, and archive diffing,
including a negative case where a tampered `.crate` in the registry must be
detected and rejected.

Never touches the real crates.io and never modifies this checkout.
"""
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import textwrap
import unittest

ROOT = Path(__file__).resolve().parent.parent
VERIFY_PUBLISHED = ROOT / 'scripts/verify-published.py'


def _load_verify_published():
    spec = importlib.util.spec_from_file_location('purrassist_verify_published', VERIFY_PUBLISHED)
    if spec is None or spec.loader is None:
        raise SystemExit(f'could not load module spec for {VERIFY_PUBLISHED}')
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


VP = _load_verify_published()
FIXTURE_VERSION = '9.9.9'
CRATE_NAMES = ('purrassist-core', 'purrassist-syntax', 'purrassist', 'purrassist-host', 'purrassist-cli')
LIB_NAMES = {
    'purrassist-core': 'purrassist_core',
    'purrassist-syntax': 'purrassist_syntax',
    'purrassist': 'purrassist',
    'purrassist-host': 'purrassist_host',
}


def index_path(name: str) -> str:
    """Cargo's registry-index nesting: 1/2/3-char names get a flat bucket,
    longer names are split into two 2-character directory levels."""
    lower = name.lower()
    if len(lower) == 1:
        return f'1/{lower}'
    if len(lower) == 2:
        return f'2/{lower}'
    if len(lower) == 3:
        return f'3/{lower[0]}/{lower}'
    return f'{lower[0:2]}/{lower[2:4]}/{lower}'


def write_license_triple(directory: Path):
    for name in ('LICENSE-MIT', 'LICENSE-APACHE', 'LICENSE-MULAN'):
        (directory / name).write_text(f'{name} fixture text.\n')


def build_fixture_workspace(root: Path, version: str):
    """A minimal, real (not mocked) five-crate dependency chain: each crate
    actually depends on and calls into the one before it, so a successful
    build genuinely proves the dependency-ordered install works, not just
    that files with the right names exist."""
    root.mkdir(parents=True, exist_ok=True)
    members = ['core', 'syntax', 'mid', 'host', 'cli']
    (root / 'Cargo.toml').write_text(
        '[workspace]\n'
        'resolver = "2"\n'
        f'members = {json.dumps(members)}\n'
    )
    chain = [
        ('core', 'purrassist-core', None),
        ('syntax', 'purrassist-syntax', 'purrassist-core'),
        ('mid', 'purrassist', 'purrassist-syntax'),
        ('host', 'purrassist-host', 'purrassist'),
    ]
    directory_of_name = {name: directory for directory, name, _ in chain}
    previous_lib = None
    for directory, name, dependency in chain:
        crate_dir = root / directory
        (crate_dir / 'src').mkdir(parents=True, exist_ok=True)
        write_license_triple(crate_dir)
        (crate_dir / 'README.md').write_text(f'{name} fixture.\n')
        lib_name = LIB_NAMES[name]
        deps_toml = ''
        marker_body = f'"{directory}".to_string()'
        if dependency is not None:
            dependency_directory = directory_of_name[dependency]
            deps_toml = (
                '\n[dependencies]\n'
                f'{dependency} = {{ path = "../{dependency_directory}", version = "{version}" }}\n'
            )
            marker_body = f'format!("{{}}+{{}}", "{directory}", {previous_lib}::marker())'
        (crate_dir / 'Cargo.toml').write_text(
            '[package]\n'
            f'name = "{name}"\n'
            f'version = "{version}"\n'
            'edition = "2021"\n'
            'license = "MIT OR Apache-2.0 OR MulanPSL-2.0"\n'
            'description = "verify-published.py fixture crate; never published"\n'
            f'{deps_toml}'
        )
        (crate_dir / 'src/lib.rs').write_text(
            f'pub fn marker() -> String {{ {marker_body} }}\n'
        )
        previous_lib = lib_name

    cli_dir = root / 'cli'
    (cli_dir / 'src').mkdir(parents=True, exist_ok=True)
    write_license_triple(cli_dir)
    (cli_dir / 'README.md').write_text('purrassist-cli fixture.\n')
    (cli_dir / 'Cargo.toml').write_text(
        '[package]\n'
        'name = "purrassist-cli"\n'
        f'version = "{version}"\n'
        'edition = "2021"\n'
        'license = "MIT OR Apache-2.0 OR MulanPSL-2.0"\n'
        'description = "verify-published.py fixture CLI; never published"\n'
        '\n[dependencies]\n'
        f'purrassist-host = {{ path = "../host", version = "{version}" }}\n'
        '\n[[bin]]\n'
        'name = "purrassist"\n'
        'path = "src/main.rs"\n'
    )
    (cli_dir / 'src/main.rs').write_text(textwrap.dedent(r'''
        use std::io::{self, BufRead, Read, Write};

        fn extract_id(line: &str) -> Option<&str> {
            let start = line.find("\"id\":")? + 5;
            let rest = &line[start..];
            let end = rest.find(|c: char| c == ',' || c == '}').unwrap_or(rest.len());
            Some(&rest[..end])
        }

        fn main() {
            let args: Vec<String> = std::env::args().skip(1).collect();
            match args.first().map(String::as_str) {
                Some("call") => {
                    let mut input = String::new();
                    io::stdin().read_to_string(&mut input).unwrap();
                    println!(
                        "{{\"tool\":\"{}\",\"marker\":\"{}\",\"echo\":{}}}",
                        args.get(1).map(String::as_str).unwrap_or(""),
                        purrassist_host::marker(),
                        input.trim()
                    );
                }
                Some("mcp") => {
                    let stdin = io::stdin();
                    let mut out = io::stdout();
                    for line in stdin.lock().lines() {
                        let line = line.unwrap();
                        if line.trim().is_empty() {
                            continue;
                        }
                        if let Some(id) = extract_id(&line) {
                            writeln!(
                                out,
                                "{{\"jsonrpc\":\"2.0\",\"id\":{id},\"result\":{{\"marker\":\"{}\"}}}}",
                                purrassist_host::marker()
                            )
                            .unwrap();
                        }
                    }
                }
                _ => std::process::exit(2),
            }
        }
    '''))


def build_local_registry(package_directory: Path, registry: Path, version: str):
    registry.mkdir(parents=True, exist_ok=True)
    (registry / 'index').mkdir()
    dependency_of = {
        'purrassist-core': None,
        'purrassist-syntax': 'purrassist-core',
        'purrassist': 'purrassist-syntax',
        'purrassist-host': 'purrassist',
        'purrassist-cli': 'purrassist-host',
    }
    for name in CRATE_NAMES:
        identity = f'{name}-{version}'
        crate = package_directory / f'{identity}.crate'
        if not crate.is_file():
            raise SystemExit(f'fixture packaging did not produce {crate}')
        data = crate.read_bytes()
        shutil.copy(crate, registry / f'{identity}.crate')
        dependency = dependency_of[name]
        deps = [] if dependency is None else [{
            'name': dependency, 'req': f'^{version}', 'features': [], 'optional': False,
            'default_features': True, 'target': None, 'kind': 'normal', 'registry': None, 'package': None,
        }]
        entry = {
            'name': name, 'vers': version, 'deps': deps,
            'cksum': hashlib.sha256(data).hexdigest(), 'features': {}, 'yanked': False, 'links': None,
        }
        path = registry / 'index' / index_path(name)
        path.parent.mkdir(parents=True, exist_ok=True)
        with open(path, 'a') as handle:
            handle.write(json.dumps(entry) + '\n')


def cargo_environment(cargo_home: Path):
    return dict(os.environ, CARGO_HOME=str(cargo_home))


def package_workspace(workspace: Path, cargo_home: Path, target_dir: Path) -> Path:
    """`--target-dir` is a best-effort hint (some environments run `cargo`
    through a wrapper that strips it and substitutes a directory of its
    own); the returned path is always read back from `cargo metadata` in
    the same environment, so it matches wherever `cargo package` actually
    wrote its archives either way."""
    target_dir.mkdir(parents=True, exist_ok=True)
    subprocess.run(['cargo', 'generate-lockfile'], cwd=workspace, env=cargo_environment(cargo_home),
                    check=True, capture_output=True, text=True)
    metadata = json.loads(subprocess.check_output(
        ['cargo', 'metadata', '--no-deps', '--format-version', '1'],
        cwd=workspace, env=cargo_environment(cargo_home), text=True))
    subprocess.run(['cargo', 'package', '--workspace', '--locked', '--target-dir', str(target_dir)],
                    cwd=workspace, env=cargo_environment(cargo_home), check=True, capture_output=True, text=True)
    names = [f"{package['name']}-{package['version']}.crate" for package in metadata['packages']]
    return VP.locate_package_directory(target_dir, metadata, names)


REQUESTS = [
    {'jsonrpc': '2.0', 'id': 1, 'method': 'initialize'},
    {'jsonrpc': '2.0', 'id': 2, 'method': 'tools/list'},
]


def generate_expected_fixtures(binary: Path, fixtures: Path, root: Path):
    fixtures.mkdir(parents=True, exist_ok=True)
    request_lines = [json.dumps(request) for request in REQUESTS]
    (fixtures / 'mcp-requests.jsonl').write_text('\n'.join(request_lines) + '\n')
    process = subprocess.run([str(binary), 'mcp', '--root', str(root)],
                              input='\n'.join(request_lines) + '\n',
                              capture_output=True, text=True, timeout=30, check=True)
    responses = [line for line in process.stdout.splitlines() if line.strip()]
    if len(responses) != len(REQUESTS):
        raise SystemExit(f'fixture CLI produced {len(responses)} responses for {len(REQUESTS)} requests')
    (fixtures / 'mcp-responses.jsonl').write_text('\n'.join(responses) + '\n')

    call_request = {'tool': 'demo_tool', 'arguments': {'hello': 'world'}}
    (fixtures / 'call-request.json').write_text(json.dumps(call_request) + '\n')
    call = subprocess.run([str(binary), 'call', call_request['tool'], '--root', str(root)],
                           input=json.dumps(call_request['arguments']),
                           capture_output=True, text=True, timeout=30, check=True)
    (fixtures / 'call-response.json').write_text(call.stdout)


class VerifyPublishedFixture:
    """Builds the fixture workspace, registry, and expected transcripts once
    per test, in a fresh temporary directory removed on cleanup."""

    def __init__(self):
        self.scratch = Path(tempfile.mkdtemp(prefix='test-verify-published-'))
        self.workspace = self.scratch / 'workspace'
        self.fixtures = self.scratch / 'fixtures'
        self.mcp_root = self.scratch / 'mcp-root'
        self.mcp_root.mkdir()
        build_fixture_workspace(self.workspace, FIXTURE_VERSION)
        build_cargo_home = self.scratch / 'build-cargo-home'
        package_directory = package_workspace(self.workspace, build_cargo_home, self.scratch / 'package-target')
        self.binary = VP.build_workspace_cli(self.workspace, build_cargo_home, self.scratch / 'cli-target')
        generate_expected_fixtures(self.binary, self.fixtures, self.mcp_root)
        self.registry = self.scratch / 'registry'
        build_local_registry(package_directory, self.registry, FIXTURE_VERSION)

    def close(self):
        shutil.rmtree(self.scratch, ignore_errors=True)

    def run_verify_published(self, extra_args=()):
        command = [
            sys.executable, str(VERIFY_PUBLISHED),
            '--version', FIXTURE_VERSION,
            '--registry-index', str(self.registry),
            '--workspace', str(self.workspace),
            '--fixtures', str(self.fixtures),
            *extra_args,
        ]
        return subprocess.run(command, capture_output=True, text=True, timeout=300, check=False)

    def run_capture(self, fixtures_dir: Path):
        command = [
            sys.executable, str(VERIFY_PUBLISHED),
            '--capture', '--binary', str(self.binary), '--fixtures', str(fixtures_dir),
        ]
        return subprocess.run(command, capture_output=True, text=True, timeout=120, check=False)


class IndexPathTests(unittest.TestCase):
    def test_bucket_nesting_matches_cargo_registry_layout(self):
        self.assertEqual(index_path('a'), '1/a')
        self.assertEqual(index_path('ab'), '2/ab')
        self.assertEqual(index_path('abc'), '3/a/abc')
        self.assertEqual(index_path('purrassist-cli'), 'pu/rr/purrassist-cli')
        self.assertEqual(index_path('Purrassist-CLI'), 'pu/rr/purrassist-cli')


class EndToEndTests(unittest.TestCase):
    """One fixture build backs every case in this class; cargo builds are
    too slow to redo per test method."""

    @classmethod
    def setUpClass(cls):
        cls.fixture = VerifyPublishedFixture()

    @classmethod
    def tearDownClass(cls):
        cls.fixture.close()

    def test_install_transcripts_and_archives_match(self):
        result = self.fixture.run_verify_published()
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        self.assertIn('Verified purrassist-cli', result.stdout)

    def test_tampered_registry_archive_is_detected(self):
        target = self.fixture.registry / f'purrassist-core-{FIXTURE_VERSION}.crate'
        original = target.read_bytes()
        tampered = bytearray(original)
        tampered[-1] ^= 0xFF
        target.write_bytes(bytes(tampered))
        try:
            result = self.fixture.run_verify_published()
            self.assertNotEqual(result.returncode, 0)
            self.assertIn('purrassist-core', result.stdout + result.stderr)
        finally:
            target.write_bytes(original)

    def test_mismatched_mcp_transcript_fixture_is_detected(self):
        responses_path = self.fixture.fixtures / 'mcp-responses.jsonl'
        original = responses_path.read_text()
        responses_path.write_text(original.replace('"marker"', '"tampered-marker"'))
        try:
            result = self.fixture.run_verify_published()
            self.assertNotEqual(result.returncode, 0)
            self.assertIn('mcp transcript', result.stdout + result.stderr)
        finally:
            responses_path.write_text(original)

    def test_missing_version_in_registry_fails_closed(self):
        result = self.fixture.run_verify_published(extra_args=['--version', '0.0.1'])
        self.assertNotEqual(result.returncode, 0)

    def test_capture_regenerates_tampered_fixtures_deterministically(self):
        """`--capture` must reproduce byte-identical fixtures from the same
        binary and request files, so a maintainer can regenerate stale
        fixtures (e.g. after a tool-registry change) with confidence."""
        copy_dir = Path(tempfile.mkdtemp(prefix='test-verify-published-capture-'))
        try:
            shutil.copytree(self.fixture.fixtures, copy_dir, dirs_exist_ok=True)
            responses_path = copy_dir / 'mcp-responses.jsonl'
            call_path = copy_dir / 'call-response.json'
            responses_path.write_text(responses_path.read_text().replace('"marker"', '"tampered-marker"'))
            call_path.write_text(call_path.read_text().replace('"marker"', '"tampered-marker"'))

            capture_result = self.fixture.run_capture(copy_dir)
            self.assertEqual(capture_result.returncode, 0, capture_result.stdout + capture_result.stderr)
            self.assertEqual(responses_path.read_text(), (self.fixture.fixtures / 'mcp-responses.jsonl').read_text())
            self.assertEqual(call_path.read_text(), (self.fixture.fixtures / 'call-response.json').read_text())
            # The request files are hand-curated inputs; capture must never touch them.
            self.assertEqual((copy_dir / 'mcp-requests.jsonl').read_text(),
                              (self.fixture.fixtures / 'mcp-requests.jsonl').read_text())
        finally:
            shutil.rmtree(copy_dir, ignore_errors=True)


class CommittedFixtureFreshnessTests(unittest.TestCase):
    """The check-mode counterpart to `--capture`: builds the *real* current
    workspace CLI (not the throwaway fixture one) and replays the committed
    `scripts/data/verify-published` transcripts against it. Wired into
    `make hygiene` (this script is), so a tool-registry change that makes the
    committed fixtures stale (e.g. Task 10's registry replacement, or a new
    tool such as `generate_json_witness`) fails the gate instead of being
    committed silently.
    """

    @classmethod
    def setUpClass(cls):
        cls.scratch = Path(tempfile.mkdtemp(prefix='verify-published-freshness-'))
        cls.binary = VP.build_workspace_cli(
            VP.ROOT, cls.scratch / 'build-cargo-home', cls.scratch / 'build-target')
        cls.root = cls.scratch / 'mcp-root'
        cls.root.mkdir()

    @classmethod
    def tearDownClass(cls):
        shutil.rmtree(cls.scratch, ignore_errors=True)

    def test_committed_fixtures_match_the_current_workspace_cli(self):
        errors = VP.run_mcp_transcript(self.binary, self.root, VP.DEFAULT_FIXTURES)
        errors += VP.run_call_transcript(self.binary, self.root, VP.DEFAULT_FIXTURES)
        self.assertEqual(
            errors, [],
            'committed scripts/data/verify-published fixtures are stale relative to the current '
            'workspace CLI; regenerate with `python3 scripts/verify-published.py --capture` and '
            'review the diff before committing:\n' + '\n'.join(errors))


if __name__ == '__main__':
    unittest.main()
