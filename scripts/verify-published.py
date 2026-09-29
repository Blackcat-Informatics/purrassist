#!/usr/bin/env python3
# SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
# SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0
"""Verify a published PurRAssist release from the outside.

Installs `purrassist-cli` at an exact version into a fresh, throwaway
`CARGO_HOME` (never the caller's own registry cache or credentials), replays
fixed CLI and MCP stdio transcripts against the installed binary and compares
them byte-for-byte with committed fixtures, then downloads each of the five
release crates' `.crate` archives and diffs their contents against a fresh
`cargo package` of the same source. Never publishes anything; every network
or registry access here is read-only.

`--registry-index` redirects installation and archive download away from the
real crates.io at a Cargo `local-registry` directory (see
https://doc.rust-lang.org/cargo/reference/source-replacement.html#local-registry-sources),
so this script is fully exercisable offline; `scripts/test-verify-published.py`
does exactly that against a throwaway fixture workspace.

`--capture` regenerates the committed `scripts/data/verify-published`
response fixtures from the current workspace CLI instead of verifying a
publication (used when the tool registry changes, e.g. Task 10's tool
census). `scripts/test-verify-published.py` runs the ordinary (non-capture)
transcript replay against the current workspace CLI as a freshness check, so
a stale fixture can never be committed silently: regenerate with

    python3 scripts/verify-published.py --capture

and review the diff before committing.
"""
import argparse
import importlib.util
import io
import json
import os
from pathlib import Path
import subprocess
import tarfile
import tempfile
import urllib.error
import urllib.request

ROOT = Path(__file__).resolve().parent.parent
USER_AGENT = 'purrassist-release (https://github.com/Blackcat-Informatics/purrassist)'
BIN_NAME = 'purrassist' + ('.exe' if os.name == 'nt' else '')
DEFAULT_FIXTURES = ROOT / 'scripts/data/verify-published'
MCP_TIMEOUT_SECONDS = 30


def _load_module(path: Path, name: str):
    spec = importlib.util.spec_from_file_location(name, path)
    if spec is None or spec.loader is None:
        raise SystemExit(f'could not load module spec for {path}')
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def _load_release_order():
    """Reuse the one true release/dependency order instead of duplicating it."""
    return _load_module(ROOT / 'scripts/check-release.py', 'purrassist_check_release').RELEASE_ORDER


RELEASE_ORDER = _load_release_order()


def extract(archive: tarfile.TarFile, member: tarfile.TarInfo, identity: str) -> bytes:
    """`TarFile.extractfile` returns `None` for non-regular members; fail
    with a precise, member-naming report instead of an `AttributeError`."""
    handle = archive.extractfile(member)
    if handle is None:
        raise SystemExit(f'{identity}: archive member {member.name} is not a regular file')
    return handle.read()


def normalize_registry_index(value: str) -> str:
    """Accept a bare filesystem path or a `file://` URL; return an absolute path."""
    if value.startswith('file://'):
        value = value[len('file://'):]
    return str(Path(value).resolve())


def configure_cargo_home(cargo_home: Path, registry_index):
    """Write a fresh CARGO_HOME with no credentials and, optionally, a
    `local-registry` source replacing crates-io for fully offline testing.
    Defense in depth only: `registry_config_args` below is what this script
    actually relies on, since some environments run `cargo` through a
    wrapper that unconditionally forces its own `CARGO_HOME` (discarding
    this one) before invoking the real binary."""
    cargo_home.mkdir(parents=True, exist_ok=True)
    if registry_index is None:
        return
    (cargo_home / 'config.toml').write_text(
        '[source.verify-published-local]\n'
        f'local-registry = "{registry_index}"\n'
        '\n'
        '[source.crates-io]\n'
        'replace-with = "verify-published-local"\n'
    )


def registry_config_args(registry_index):
    """`--config` CLI overrides for a local-registry source replacement,
    applied directly on the command line rather than through a config file.
    Cargo's own precedence rules put CLI `--config` above every config file
    (including one this script writes into its own scratch `CARGO_HOME`),
    so this keeps working even under a wrapper that replaces `CARGO_HOME`
    outright before running the real `cargo` (measured on this workstation:
    `/home/paudley/stage/scripts/cargo` does exactly that, but explicitly
    forwards `--config`)."""
    if registry_index is None:
        return []
    return [
        '--config', f'source.verify-published-local.local-registry="{registry_index}"',
        '--config', 'source.crates-io.replace-with="verify-published-local"',
    ]


def caller_bin_candidates():
    """Locations a `cargo install` that ignored `--root` could write a
    `purrassist` binary into: the default `~/.cargo/bin`, and the ambient
    `CARGO_HOME` this process inherited (if different) -- covering a
    wrapper that forces its own `CARGO_HOME` regardless of what this script
    sets for its own subprocess calls."""
    candidates = {Path.home() / '.cargo' / 'bin' / BIN_NAME}
    ambient = os.environ.get('CARGO_HOME')
    if ambient:
        candidates.add(Path(ambient) / 'bin' / BIN_NAME)
    return candidates


def snapshot_caller_bins():
    snapshot = {}
    for path in caller_bin_candidates():
        try:
            stat = path.stat()
            snapshot[path] = (stat.st_mtime_ns, stat.st_size)
        except FileNotFoundError:
            snapshot[path] = None
    return snapshot


def assert_hermetic_install(before, after, install_root: Path, scratch: Path):
    """Fail loudly, naming exactly what happened, if installation was not
    fully contained in `scratch` -- the caller's own toolchain state (e.g.
    `~/.cargo/bin/purrassist`) must be byte-for-byte unaffected by a run of
    this script."""
    changed = sorted(str(path) for path in before if before[path] != after.get(path))
    if changed:
        raise SystemExit(
            f'hermeticity violation: cargo install changed {changed} outside --root {install_root}; '
            'refusing to trust this result')
    if scratch.resolve() not in install_root.resolve().parents and install_root.resolve() != scratch.resolve():
        raise SystemExit(f'hermeticity violation: install root {install_root} is not under the scratch '
                          f'directory {scratch}')


def install(cargo_home: Path, version: str, registry_index, scratch: Path):
    """Install `purrassist-cli` at `version` into a scratch root under
    `cargo_home`, using a scratch, non-workspace current directory so
    installation never sees this checkout's own path dependencies. Passes
    the local-registry replacement (if any) both as `--config` (relied on)
    and via `cargo_home`'s own config file (defense in depth; see
    `configure_cargo_home`), and verifies afterwards that nothing outside
    `scratch`/`--root` was touched.

    # Errors
    Raises `SystemExit` if `cargo install` fails, does not produce the
    expected binary, or the hermeticity check fails.
    """
    configure_cargo_home(cargo_home, registry_index)
    install_root = cargo_home / 'install'
    workdir = cargo_home / 'install-cwd'
    workdir.mkdir(parents=True, exist_ok=True)
    environment = dict(os.environ, CARGO_HOME=str(cargo_home))
    command = [
        'cargo', *registry_config_args(registry_index),
        'install', 'purrassist-cli', '--version', version, '--locked',
        '--root', str(install_root),
    ]
    if registry_index is not None:
        # Hermeticity check: a local-registry replacement never needs the
        # network, so if cargo tried to reach one anyway this fails loudly
        # instead of silently succeeding by accidental connectivity.
        command.append('--offline')
    before = snapshot_caller_bins()
    result = subprocess.run(command, cwd=workdir, env=environment,
                             capture_output=True, text=True, check=False)
    after = snapshot_caller_bins()
    assert_hermetic_install(before, after, install_root, scratch)
    if result.returncode != 0:
        raise SystemExit(
            f'cargo install purrassist-cli --version {version} failed '
            f'(exit {result.returncode}):\n{result.stdout}\n{result.stderr}')
    binary = install_root / 'bin' / BIN_NAME
    if not binary.is_file():
        raise SystemExit(f'cargo install did not produce the expected binary at {binary}')
    return binary


def build_workspace_cli(workspace: Path, cargo_home: Path | None = None, target_dir: Path | None = None) -> Path:
    """Build `purrassist-cli` from `workspace` and return the produced
    binary's path, read directly from `--message-format=json`'s own report
    (accurate regardless of whether `--target-dir` was honored or a wrapper
    silently redirected it elsewhere -- this never trusts a *separate*
    lookup of where the build should have landed). Uses the ambient
    `CARGO_HOME`/build cache unless `cargo_home` is given, so a maintainer
    running `--capture` locally gets an ordinary incremental build rather
    than a from-scratch one.

    # Errors
    Raises `SystemExit` if the build fails or produces no `purrassist` binary.
    """
    environment = os.environ.copy()
    if cargo_home is not None:
        cargo_home.mkdir(parents=True, exist_ok=True)
        environment['CARGO_HOME'] = str(cargo_home)
    command = ['cargo', 'build', '-p', 'purrassist-cli', '--locked', '--message-format=json']
    if target_dir is not None:
        target_dir.mkdir(parents=True, exist_ok=True)
        command += ['--target-dir', str(target_dir)]
    result = subprocess.run(command, cwd=workspace, env=environment, capture_output=True, text=True, check=False)
    if result.returncode != 0:
        raise SystemExit(f'cargo build -p purrassist-cli failed:\n{result.stdout}\n{result.stderr}')
    for line in result.stdout.splitlines():
        event = json.loads(line)
        if (event.get('reason') == 'compiler-artifact' and event.get('executable')
                and event['target']['name'] == 'purrassist'):
            return Path(event['executable'])
    raise SystemExit(f'cargo build -p purrassist-cli did not produce a purrassist binary from {workspace}')


def read_lines(path: Path):
    return [line for line in path.read_text().splitlines() if line.strip()]


def replay_mcp(binary: Path, root: Path, requests):
    """Send `requests` (JSON-RPC lines) to a fresh `purrassist mcp` session
    one at a time and return the response line for every id'd request, in
    request order. Strictly sequential, well within the session's bounded
    concurrent-call admission (never floods past it, unlike sending every
    request before reading any response).

    # Errors
    Raises `SystemExit` if the subprocess does not expose its stdio pipes,
    produces no response for an id'd request, or exits non-zero.
    """
    # `with process:` closes stdin/stdout/stderr on every exit path (normal,
    # exception, or timeout), instead of leaking the pipe file objects.
    with subprocess.Popen(
        [str(binary), 'mcp', '--root', str(root)],
        stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
        text=True, bufsize=1,
    ) as process:
        if process.stdin is None or process.stdout is None or process.stderr is None:
            raise SystemExit('purrassist mcp subprocess did not provide stdio pipes')
        stdin, stdout, stderr = process.stdin, process.stdout, process.stderr
        actual = []
        returncode = None
        try:
            for line in requests:
                request = json.loads(line)
                stdin.write(line + '\n')
                stdin.flush()
                if request.get('id') is None:
                    continue  # A notification never gets a response.
                response = stdout.readline()
                if not response:
                    raise SystemExit(f'purrassist mcp produced no response for {line!r}: {stderr.read()}')
                actual.append(response.rstrip('\n'))
            stdin.close()
            returncode = process.wait(timeout=MCP_TIMEOUT_SECONDS)
        finally:
            stderr_tail = stderr.read() if process.poll() is not None else ''
            if process.poll() is None:
                process.kill()
        if returncode != 0:
            raise SystemExit(f'purrassist mcp exited {returncode}: {stderr_tail}')
        return actual


def run_mcp_transcript(binary: Path, root: Path, fixtures: Path):
    """Replay the committed MCP stdio transcript against `binary` and return
    a list of precise mismatch descriptions (empty on an exact match)."""
    requests = read_lines(fixtures / 'mcp-requests.jsonl')
    expected = read_lines(fixtures / 'mcp-responses.jsonl')
    actual = replay_mcp(binary, root, requests)
    return diff_transcript('mcp', expected, actual)


def run_call_transcript(binary: Path, root: Path, fixtures: Path):
    """Replay the committed `call` transcript and return mismatch descriptions."""
    request = json.loads((fixtures / 'call-request.json').read_text())
    expected = (fixtures / 'call-response.json').read_text().rstrip('\n')
    result = subprocess.run(
        [str(binary), 'call', request['tool'], '--root', str(root)],
        input=json.dumps(request['arguments']), capture_output=True, text=True,
        timeout=MCP_TIMEOUT_SECONDS, check=False)
    errors = []
    if result.returncode != 0:
        errors.append(f'purrassist call {request["tool"]} exited {result.returncode}: {result.stderr}')
    actual = result.stdout.rstrip('\n')
    if actual != expected:
        errors.append(
            f'call {request["tool"]} transcript differs from scripts/data/verify-published/call-response.json:\n'
            f'  expected: {expected}\n  actual:   {actual}')
    return errors


def diff_transcript(kind, expected, actual):
    if expected == actual:
        return []
    errors = [f'{kind} transcript has {len(actual)} response(s); expected {len(expected)}'] \
        if len(expected) != len(actual) else []
    for index, (want, got) in enumerate(zip(expected, actual)):
        if want != got:
            errors.append(f'{kind} transcript response {index} differs:\n  expected: {want}\n  actual:   {got}')
    return errors


def capture_fixtures(binary: Path, root: Path, fixtures: Path):
    """Regenerate the committed `mcp-responses.jsonl`/`call-response.json`
    fixtures deterministically, by replaying the existing, hand-curated
    request fixtures (`mcp-requests.jsonl`/`call-request.json`, left
    untouched) against `binary`.

    # Errors
    Raises `SystemExit` if the `call` replay exits non-zero.
    """
    requests = read_lines(fixtures / 'mcp-requests.jsonl')
    responses = replay_mcp(binary, root, requests)
    (fixtures / 'mcp-responses.jsonl').write_text('\n'.join(responses) + '\n')

    call_request = json.loads((fixtures / 'call-request.json').read_text())
    result = subprocess.run(
        [str(binary), 'call', call_request['tool'], '--root', str(root)],
        input=json.dumps(call_request['arguments']), capture_output=True, text=True,
        timeout=MCP_TIMEOUT_SECONDS, check=False)
    if result.returncode != 0:
        raise SystemExit(
            f'purrassist call {call_request["tool"]} exited {result.returncode}: {result.stderr}')
    (fixtures / 'call-response.json').write_text(result.stdout)


def crate_bytes_from_registry(registry_index: Path, identity: str) -> bytes:
    path = registry_index / f'{identity}.crate'
    if not path.is_file():
        raise SystemExit(f'local registry is missing {path}')
    return path.read_bytes()


def crate_bytes_from_crates_io(name: str, version: str) -> bytes:
    url = f'https://crates.io/api/v1/crates/{name}/{version}/download'
    request = urllib.request.Request(url, headers={'User-Agent': USER_AGENT})
    try:
        with urllib.request.urlopen(request, timeout=60) as response:
            return response.read()
    except urllib.error.HTTPError as error:
        raise SystemExit(f'downloading {name} {version} failed: HTTP {error.code}') from error


def package_fresh(workspace: Path, cargo_home: Path, target_dir: Path) -> Path:
    """Run `cargo package --locked` for exactly the publishable release
    crates in `workspace` under a scratch `CARGO_HOME`, and return the
    directory holding the produced `.crate` archives. Packages only the
    publishable set (mirroring `check-release.py`'s filter) rather than
    `--workspace`, since the latter also tries to package and verify-build
    `publish = false` tooling crates that are not part of the release this
    script audits and may target an entirely different build (e.g.
    `crates/wasm-harness`, wasm32-unknown-unknown only).

    `--target-dir` is passed as a best-effort hint (some environments run
    `cargo` through a wrapper that silently strips `--target-dir` and
    substitutes a directory of its own choosing); either way, the returned
    path is read back from `cargo metadata` run in the exact same
    environment immediately afterwards, so it always matches wherever
    `cargo package` actually wrote its archives, honored hint or not."""
    cargo_home.mkdir(parents=True, exist_ok=True)
    target_dir.mkdir(parents=True, exist_ok=True)
    environment = dict(os.environ, CARGO_HOME=str(cargo_home))
    # `--locked` needs a lockfile. Generate one only when the workspace has
    # none (a synthetic fixture); regenerating an existing one would rewrite
    # the committed dependency pins this verification is meant to hold fixed.
    if not (workspace / 'Cargo.lock').is_file():
        subprocess.run(['cargo', 'generate-lockfile'], cwd=workspace,
                        env=environment, check=True, capture_output=True, text=True)
    metadata = json.loads(subprocess.check_output(
        ['cargo', 'metadata', '--no-deps', '--format-version', '1'],
        cwd=workspace, env=environment, text=True))
    publishable = sorted(
        package['name'] for package in metadata['packages'] if package.get('publish') != [])
    package_args = [flag for name in publishable for flag in ('-p', name)]
    result = subprocess.run(
        ['cargo', 'package', *package_args, '--locked', '--target-dir', str(target_dir)],
        cwd=workspace, env=environment, capture_output=True, text=True, check=False)
    if result.returncode != 0:
        raise SystemExit(f'cargo package failed:\n{result.stdout}\n{result.stderr}')
    versions = {package['name']: package['version'] for package in metadata['packages']}
    return locate_package_directory(
        target_dir, metadata, [f'{name}-{versions[name]}.crate' for name in publishable])


def locate_package_directory(target_dir: Path, metadata: dict, names) -> Path:
    """Return the `package` directory that actually holds the expected
    `.crate` archives. Plain cargo honours `--target-dir`; a wrapper may strip
    it and write under the target directory `cargo metadata` reports. Checking
    both, in that order, is correct under either, and anything else is an
    explicit error naming both candidates."""
    candidates = [target_dir / 'package', Path(metadata['target_directory']) / 'package']
    for candidate in candidates:
        if all((candidate / name).is_file() for name in names):
            return candidate
    raise SystemExit('cargo package produced no archives in either candidate directory: '
                     + ', '.join(str(candidate) for candidate in candidates)
                     + f' (expected {", ".join(names)})')


def archive_members(data: bytes, identity: str):
    with tarfile.open(fileobj=io.BytesIO(data)) as archive:
        return {
            member.name: extract(archive, member, identity)
            for member in archive.getmembers() if member.isfile()
        }


def diff_crate(identity: str, downloaded: bytes, local: bytes):
    """Precise, member-level comparison instead of a single opaque byte
    mismatch, so a real content difference is distinguishable from
    incidental archive metadata (which is still reported, never ignored)."""
    if downloaded == local:
        return []
    left, right = archive_members(downloaded, identity), archive_members(local, identity)
    errors = []
    missing = sorted(set(right) - set(left))
    extra = sorted(set(left) - set(right))
    if missing:
        errors.append(f'{identity}: downloaded archive is missing {missing}')
    if extra:
        errors.append(f'{identity}: downloaded archive has unexpected extra member(s) {extra}')
    for name in sorted(set(left) & set(right)):
        if left[name] != right[name]:
            errors.append(f'{identity}: member {name} differs between the downloaded and freshly packaged archive')
    if not errors:
        errors.append(
            f'{identity}: archive bytes differ but extracted contents are identical '
            '(packaging metadata only, e.g. timestamps); treated as a difference')
    return errors


def main():
    parser = argparse.ArgumentParser(description=__doc__,
                                      formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument('--version', default=None,
                         help='the published version to verify, e.g. 0.1.0 (required unless --capture)')
    parser.add_argument('--registry-index', default=None,
                         help='local-registry directory path or file:// URL for offline verification; '
                              'default installs from and downloads from the real crates.io')
    parser.add_argument('--workspace', type=Path, default=ROOT,
                         help='workspace to freshly `cargo package`/build for comparison (default: this checkout)')
    parser.add_argument('--fixtures', type=Path, default=DEFAULT_FIXTURES,
                         help='directory holding the committed call/mcp transcript fixtures')
    parser.add_argument('--capture', action='store_true',
                         help='regenerate the committed response fixtures from the current workspace CLI '
                              '(or --binary) instead of verifying a publication')
    parser.add_argument('--binary', type=Path, default=None,
                         help='with --capture, an already-built purrassist binary to capture from instead of '
                              'building one from --workspace')
    args = parser.parse_args()

    if args.capture:
        if args.registry_index is not None or args.version is not None:
            parser.error('--capture does not take --version or --registry-index')
        if args.binary is not None and not args.binary.is_file():
            parser.error(f'--binary {args.binary} is not a file')
        with tempfile.TemporaryDirectory(prefix='purrassist-verify-published-capture-') as scratch_name:
            scratch = Path(scratch_name)
            binary = args.binary or build_workspace_cli(
                args.workspace, scratch / 'cargo-home', scratch / 'target')
            root = scratch / 'mcp-root'
            root.mkdir()
            capture_fixtures(binary, root, args.fixtures)
        print(f'Captured {args.fixtures} from {binary}. Review the diff before committing.')
        return

    if args.version is None:
        parser.error('--version is required unless --capture is given')

    registry_index = normalize_registry_index(args.registry_index) if args.registry_index else None
    errors = []
    with tempfile.TemporaryDirectory(prefix='purrassist-verify-published-') as scratch_name:
        scratch = Path(scratch_name)
        binary = install(scratch / 'cargo-home', args.version, registry_index, scratch)

        root = scratch / 'mcp-root'
        root.mkdir()
        errors += run_mcp_transcript(binary, root, args.fixtures)
        errors += run_call_transcript(binary, root, args.fixtures)

        package_directory = package_fresh(
            args.workspace, scratch / 'package-cargo-home', scratch / 'package-target')
        for name in RELEASE_ORDER:
            identity = f'{name}-{args.version}'
            local_path = package_directory / f'{identity}.crate'
            if not local_path.is_file():
                errors.append(f'{identity}: freshly packaged archive not found at {local_path}')
                continue
            if registry_index is not None:
                downloaded = crate_bytes_from_registry(Path(registry_index), identity)
            else:
                downloaded = crate_bytes_from_crates_io(name, args.version)
            errors += diff_crate(identity, downloaded, local_path.read_bytes())

    if errors:
        raise SystemExit('\n'.join(errors))
    print(f'Verified purrassist-cli {args.version}: installed CLI and MCP transcripts match the committed '
          f'fixtures, and all {len(RELEASE_ORDER)} downloaded archives match freshly packaged copies of '
          f'{args.workspace}.')


if __name__ == '__main__':
    main()
