#!/usr/bin/env python3
# SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
# SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0
"""Linux process/heap qualification; no instrumentation is linked into the core."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import platform
import re
import shutil
import subprocess
import sys
import tempfile
import time
import urllib.request

ROOT = Path(__file__).resolve().parent.parent


def memcheck(text) -> dict[str, int]:
    match = re.search(r'total heap usage: ([\d,]+) allocs, ([\d,]+) frees, ([\d,]+) bytes allocated', text)
    if not match:
        raise ValueError('missing Memcheck allocation summary')
    errors = re.search(r'ERROR SUMMARY: ([\d,]+) errors', text)
    if not errors or int(errors[1].replace(',', '')):
        raise ValueError('missing or nonzero Memcheck error summary')
    keys: tuple[str, str, str] = ('allocation_calls', 'free_calls', 'total_allocated_bytes')
    values = (int(value.replace(',', '')) for value in match.groups())
    return dict(zip(keys, values))


def massif(text):
    samples = []
    for block in text.split('snapshot=')[1:]:
        fields = dict(re.findall(r'^(mem_heap_B|mem_heap_extra_B|mem_stacks_B)=(\d+)$', block, re.M))
        if len(fields) != 3:
            raise ValueError('incomplete Massif snapshot')
        samples.append({key: int(value) for key, value in fields.items()})
    if not samples or 'heap_tree=peak' not in text:
        raise ValueError('missing Massif snapshots or peak record')
    return {
        'observed_peak_requested_heap_bytes': max(row['mem_heap_B'] for row in samples),
        'observed_peak_heap_plus_estimated_overhead_bytes': max(row['mem_heap_B'] + row['mem_heap_extra_B'] for row in samples),
        'snapshots': len(samples),
    }


def build(command, target, directory):
    start = time.perf_counter_ns()
    result = subprocess.run(command + ['--message-format=json'], cwd=ROOT, text=True,
                            stdout=subprocess.PIPE, stderr=subprocess.PIPE, check=False)
    (directory / f'build-{target}.log').write_text(result.stderr)
    result.check_returncode()
    events = [json.loads(line) for line in result.stdout.splitlines()]
    artifacts = [event for event in events if event.get('reason') == 'compiler-artifact']
    selected = [event for event in artifacts if event['target']['name'] == target and event.get('executable')]
    if len(selected) != 1:
        raise ValueError(f'expected one executable for {target}')
    binary = Path(selected[0]['executable'])
    return binary, {
        'target': target, 'command': command, 'elapsed_ns': time.perf_counter_ns() - start,
        'cache_state': 'not cleaned; fresh artifact counts are observed Cargo events',
        'fresh_artifacts': sum(bool(event['fresh']) for event in artifacts),
        'built_artifacts': sum(not event['fresh'] for event in artifacts),
        'profile': selected[0]['profile'],
        'executable_sha256': hashlib.sha256(binary.read_bytes()).hexdigest(),
    }


def native(command, case, environment):
    start = time.perf_counter_ns()
    result = subprocess.run(command + [case, '--rss'], cwd=ROOT, env=environment,
                            stdout=subprocess.PIPE, stderr=subprocess.PIPE, check=True)
    elapsed = time.perf_counter_ns() - start
    return {'process_elapsed_ns': elapsed, 'process_peak_rss_bytes': rss(result.stdout, case)}


def rss(output, case):
    lines = output.splitlines(keepends=True)
    if len(lines) != 2:
        raise ValueError('missing native RSS probe output')
    verify_output(lines[0], case)
    match = re.fullmatch(rb'rss_bytes ([0-9]+)\n', lines[1])
    if not match or int(match[1]) == 0:
        raise ValueError('invalid native RSS probe output')
    return int(match[1])


def verify_output(output, case):
    if output != f'ok {case}\n'.encode():
        raise ValueError(f'unexpected workload output for {case}')


def run_profiler(command, executable, case, tool, directory, environment):
    """Retain every diagnostic before rejecting a profiler's failed execution."""
    result = subprocess.run(command + executable + [case], cwd=ROOT, env=environment,
                            stdout=subprocess.PIPE, stderr=subprocess.PIPE, check=False)
    (directory / f'{case}.{tool}.stdout').write_bytes(result.stdout)
    (directory / f'{case}.{tool}.stderr').write_bytes(result.stderr)
    tool_log = directory / f'{case}.{tool}.log'
    if result.returncode != 0:
        log = tool_log.read_text(errors='replace') if tool_log.exists() else '(not created)'
        raise RuntimeError(
            f'{tool} workload {case} exited {result.returncode}; raw logs retained in {directory}\n'
            f'stdout:\n{result.stdout.decode(errors="replace")}\n'
            f'stderr:\n{result.stderr.decode(errors="replace")}\n'
            f'tool log ({tool_log.name}):\n{log}'
        )
    verify_output(result.stdout, case)


def profile(executable, case, directory, environment):
    row = {'case': case, **native(executable, case, environment)}
    memory = directory / f'{case}.memcheck.log'
    heap = directory / f'{case}.massif'
    commands = [
        ['valgrind', '--tool=memcheck', '--leak-check=full', '--track-origins=yes',
         '--show-leak-kinds=all', '--errors-for-leak-kinds=definite,indirect,possible',
         '--error-exitcode=97', f'--log-file={memory}'],
        ['valgrind', '--tool=massif', '--time-unit=B', '--peak-inaccuracy=0.0',
         '--max-snapshots=1000', '--detailed-freq=1', f'--massif-out-file={heap}',
         f'--log-file={directory / (case + ".massif.log")}'],
    ]
    for tool, command in zip(('memcheck', 'massif'), commands):
        run_profiler(command, executable, case, tool, directory, environment)
    row.update(memcheck(memory.read_text()))
    row.update(massif(heap.read_text()))
    return row


def source_identity():
    names = subprocess.check_output(['git', 'ls-files', '-co', '--exclude-standard', '-z'], cwd=ROOT).split(b'\0')
    digest = hashlib.sha256()
    for name in sorted(set(names) - {b''}):
        path = ROOT / os.fsdecode(name)
        if path.is_file():
            digest.update(name + b'\0' + hashlib.sha256(path.read_bytes()).digest())
    return {
        'commit': subprocess.check_output(['git', 'rev-parse', 'HEAD'], cwd=ROOT, text=True).strip(),
        'dirty': bool(subprocess.check_output(['git', 'status', '--porcelain'], cwd=ROOT)),
        'nonignored_file_tree_sha256': digest.hexdigest(),
    }


def engine_source_identity():
    """Digest executable source inputs, excluding reports and documentation.

    Record before building and reject mutations before writing evidence. This
    binds measurements to stable engines even when documentation is edited in
    parallel. Cargo.lock binds fetched grammar/dependency versions.
    """
    names = {'Cargo.toml', 'Cargo.lock', 'rust-toolchain.toml', 'Makefile',
             'scripts/measure-resources.py', 'scripts/check-resource-limits.py',
             'scripts/test-resource-report.py'}
    for path in (ROOT / 'crates').rglob('*'):
        relative = path.relative_to(ROOT)
        if path.is_file() and ('tests' not in relative.parts) and (
            path.suffix in ('.rs', '.c', '.h', '.json') or path.name == 'Cargo.toml'
        ):
            names.add(relative.as_posix())
    for base in ('.cargo', 'vendor'):
        directory = ROOT / base
        if directory.exists():
            names.update(path.relative_to(ROOT).as_posix() for path in directory.rglob('*')
                         if path.is_file() and path.suffix in ('.toml', '.json'))
    digest = hashlib.sha256()
    for name in sorted(names):
        path = ROOT / name
        if not path.is_file():
            raise ValueError(f'missing engine source input {name}')
        digest.update(name.encode() + b'\0' + hashlib.sha256(path.read_bytes()).digest())
    return digest.hexdigest()


def runtime(binary, portable):
    if not portable:
        return [str(binary)], {'kind': 'system loader and libraries'}
    manifest = json.loads((ROOT / 'vendor/profiling-runtime.json').read_text())
    if platform.machine() != manifest['architecture']:
        raise ValueError('portable profiling runtime architecture mismatch')
    cache = ROOT / '.cache/profile-runtime'
    cache.mkdir(parents=True, exist_ok=True)
    archive = cache / 'runtime.pkg.tar.zst'
    if not archive.exists():
        with urllib.request.urlopen(manifest['url']) as response:
            archive.write_bytes(response.read())
    if hashlib.sha256(archive.read_bytes()).hexdigest() != manifest['sha256']:
        raise ValueError('profiling runtime archive digest mismatch')
    subprocess.run(['bsdtar', '-xf', str(archive), '-C', str(cache), *manifest['members']], check=True)
    files = {}
    for name in manifest['members']:
        path = cache / name
        if path.is_symlink() or not path.is_file() or cache.resolve() not in path.resolve().parents:
            raise ValueError('invalid profiling runtime member')
        files[name] = hashlib.sha256(path.read_bytes()).hexdigest()
    # The stripped loader needs these exact symbols for mandatory Memcheck
    # redirections. A verified adjacent GNU debuglink file avoids depending
    # on the caller's debuginfod configuration or system debug packages.
    debug = manifest['loader_debuginfo']
    debug_file = cache / debug['member']
    if debug_file.is_symlink() or cache.resolve() not in debug_file.resolve().parents:
        raise ValueError('invalid profiling loader debuginfo path')
    if debug_file.exists() and not debug_file.is_file():
        raise ValueError('profiling loader debuginfo must be a regular file')
    debug_file.parent.mkdir(parents=True, exist_ok=True)
    if not debug_file.exists():
        with urllib.request.urlopen(debug['url']) as response:
            debug_file.write_bytes(response.read())
    debug_digest = hashlib.sha256(debug_file.read_bytes()).hexdigest()
    if debug_digest != debug['sha256']:
        raise ValueError('profiling loader debuginfo digest mismatch')
    files[debug['member']] = debug_digest
    loader = cache / 'usr/lib/ld-linux-x86-64.so.2'
    return [str(loader), '--library-path', str(cache / 'usr/lib'), str(binary)], {
        'kind': 'isolated pinned loader and libc; other libraries supplied by host',
        'name': manifest['name'], 'version': manifest['version'],
        'archive_sha256': manifest['sha256'], 'files_sha256': files,
        'extractor': subprocess.check_output(['bsdtar', '--version'], text=True).strip(),
    }


def identity(path):
    return {'sha256': hashlib.sha256(path.read_bytes()).hexdigest()}


def bench_executable(package, bench, directory):
    """Build one `harness = false` bench binary and return its path; a
    leaner sibling of `build()` above (no fresh/rebuilt artifact bookkeeping,
    which only matters for the profiled `resources` bench)."""
    log = directory / f'{package}-{bench}-build.log'
    command = ['cargo', 'bench', '-p', package, '--bench', bench, '--no-run', '--locked',
               '--message-format=json']
    result = subprocess.run(command, cwd=ROOT, text=True, stdout=subprocess.PIPE,
                            stderr=subprocess.PIPE, check=False)
    log.write_text(result.stderr)
    if result.returncode != 0:
        raise SystemExit(f'building {package}::{bench}: {result.stderr}')
    events = [json.loads(line) for line in result.stdout.splitlines()]
    selected = [event for event in events if event.get('reason') == 'compiler-artifact'
                and event.get('executable') and event['target']['name'] == bench]
    if len(selected) != 1:
        raise ValueError(f'expected one executable for {package}::{bench}')
    return Path(selected[0]['executable'])


def run_latency_bench(package, directory):
    """Run every workload `crates/{core,syntax}/benches/latency.rs` measures
    (median/p95 latency, real allocation calls, real peak live bytes) and
    return its JSON array verbatim."""
    binary = bench_executable(package, 'latency', directory)
    cases = subprocess.check_output([str(binary), '--list'], cwd=ROOT, text=True).splitlines()
    if not cases or len(cases) != len(set(cases)):
        raise ValueError('empty or duplicate latency workload inventory')
    rows = []
    for case in cases:
        if not re.fullmatch(r'[a-z_]+', case):
            raise ValueError('invalid latency workload name')
        row = json.loads(subprocess.check_output([str(binary), case], cwd=ROOT, text=True))
        if row.get('case') != case:
            raise ValueError('latency workload output does not match inventory')
        rows.append(row)
    return rows


def run_recovery_bench(directory):
    """Run `crates/core/benches/recovery.rs`'s six deterministic recovery
    cases (quality assertions plus latency) and return its JSON array."""
    binary = bench_executable('purrassist-core', 'recovery', directory)
    output = subprocess.check_output([str(binary)], cwd=ROOT, text=True)
    return json.loads(output)


def toolchain_cargo() -> str:
    """Resolve Cargo through rustup, bypassing host Cargo launch wrappers."""
    if not shutil.which('rustup'):
        raise ValueError('rustup is required to resolve the real toolchain Cargo')
    return subprocess.check_output(['rustup', 'which', 'cargo'], cwd=ROOT, text=True).strip()


def cold_build():
    """Build the release CLI in empty target AND intermediate directories.

    Clear both compiler wrapper layers, including config overrides. Cargo's
    package download cache remains available; OS page caches are not flushed.
    """
    cargo = toolchain_cargo()
    environment = os.environ.copy()
    for name in ('RUSTC_WRAPPER', 'RUSTC_WORKSPACE_WRAPPER',
                 'CARGO_BUILD_RUSTC_WRAPPER', 'CARGO_BUILD_RUSTC_WORKSPACE_WRAPPER'):
        environment[name] = ''
    cache = ROOT / '.cache'
    cache.mkdir(exist_ok=True)
    # Compile outside the source tree: hosts may enforce that no generated
    # Cargo target/build tree exists inside a checkout, even in ignored caches.
    with tempfile.TemporaryDirectory(prefix='purrassist-cold-build-', dir='/tmp') as directory:
        target = Path(directory) / 'target'
        intermediate = Path(directory) / 'build'
        environment['CARGO_TARGET_DIR'] = str(target)
        environment['CARGO_BUILD_BUILD_DIR'] = str(intermediate)
        command = [cargo, 'build', '-p', 'purrassist-cli', '--release', '--locked',
                   '--target-dir', str(target), '--config', f'build.build-dir="{intermediate}"',
                   '--config', 'build.rustc-wrapper=""', '--config', 'build.rustc-workspace-wrapper=""',
                   '--message-format=json']
        start = time.perf_counter_ns()
        result = subprocess.run(command, cwd=ROOT, env=environment, check=False,
                                text=True, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
        elapsed = time.perf_counter_ns() - start
        (cache / 'cold-build.log').write_text(result.stderr)
        (cache / 'cold-build.events.jsonl').write_text(result.stdout)
        result.check_returncode()
        artifacts = [event for event in map(json.loads, result.stdout.splitlines())
                     if event.get('reason') == 'compiler-artifact']
        fresh = sum(bool(event['fresh']) for event in artifacts)
        if not artifacts or fresh:
            raise ValueError('cold build reused compiler artifacts or produced none')
        binaries = [event['executable'] for event in artifacts
                    if event['target']['name'] == 'purrassist' and event.get('executable')]
        if len(binaries) != 1:
            raise ValueError('cold build did not produce exactly one CLI')
        artifact = identity(Path(binaries[0]))
    return {
        'method': ('Real toolchain Cargo resolved through rustup; brand-new empty target and '
                   'intermediate build directories under /tmp outside the checkout; '
                   'both compiler wrapper layers disabled. '
                   'Downloaded package sources and OS page cache are retained.'),
        'fresh_target_directory': True,
        'fresh_build_directory': True,
        'compiler_cache_disabled': True,
        'fresh_artifacts': fresh,
        'built_artifacts': len(artifacts),
        'artifact': artifact,
        'elapsed_ns': elapsed,
    }


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--output', type=Path, default=ROOT / '.cache/resource-report.json')
    parser.add_argument('--portable-runtime', action='store_true', help='use the pinned isolated x86_64 glibc runtime for profiler compatibility')
    parser.add_argument('--check-tools', action='store_true', help='run strict native/Memcheck/Massif startup and parser checks, without writing a measurement report')
    args = parser.parse_args()
    if sys.platform != 'linux':
        raise SystemExit('this measurement runner qualifies Linux only (procfs RSS probe)')
    version = subprocess.check_output(['valgrind', '--version'], text=True).strip()
    inventory = json.loads((ROOT / 'vendor/tooling.json').read_text())
    expected = next(item['version'] for item in inventory if item['name'] == 'valgrind')
    if version != 'valgrind-' + expected:
        raise SystemExit('Valgrind version differs from the qualification inventory')
    cache = ROOT / '.cache'
    cache.mkdir(exist_ok=True)
    directory = Path(tempfile.mkdtemp(prefix='resource-logs-', dir=cache))
    print(f'Raw resource logs: {directory}', flush=True)
    environment = dict(os.environ, LC_ALL='C')
    engine_digest = engine_source_identity()
    # An explicit target-cpu opts this one build out of a host-tuning cargo
    # wrapper that otherwise appends `-C target-cpu=native` for any build
    # whose target is this host; Valgrind's JIT cannot decode every
    # instruction that flag can select on a modern CPU. x86-64-v2 (SSE4.2)
    # is a portable baseline every Valgrind-supported host can execute; see
    # "Loader compatibility" below for the matching loader/libc fix this
    # same SIGILL failure mode needs.
    binary, benchmark_build = build(['cargo', 'bench', '-p', 'purrassist-core', '--bench', 'resources', '--no-run', '--locked', '--config', 'profile.bench.debug=1', '--config', 'profile.bench.strip=false', '--config', 'build.rustflags=["-C", "target-cpu=x86-64-v2"]'], 'resources', directory)
    executable, runtime_identity = runtime(binary, args.portable_runtime)
    print('Checking native, Memcheck and Massif startup on the empty workload', flush=True)
    preflight = directory / 'preflight'
    preflight.mkdir()
    profile(executable, 'empty', preflight, environment)
    if args.check_tools:
        if engine_source_identity() != engine_digest:
            raise ValueError('engine source inputs changed during profiler preflight')
        print('Native, Memcheck and Massif startup/output/log checks passed; no measurement report written', flush=True)
        return
    _, cli_build = build(['cargo', 'build', '-p', 'purrassist-cli', '--release', '--locked'], 'purrassist', directory)
    cases = subprocess.check_output([str(binary), '--list'], text=True).splitlines()
    if not cases or len(set(cases)) != len(cases) or any(not re.fullmatch(r'[a-z_]+', case) for case in cases):
        raise ValueError('invalid workload inventory')
    report = {
        'format': 'purrassist.resources.v2', 'source': source_identity(),
        'platform': {'system': 'Linux', 'architecture': platform.machine(), 'libc': platform.libc_ver()},
        'tools': {'valgrind': version, 'rustc': subprocess.check_output(['rustc', '--version'], text=True).strip()},
        'scope': 'One process per workload and profiler; includes startup, construction, assertions, output, and teardown. Native elapsed time is not per-operation latency. Heap excludes stacks and mappings; Massif overhead is estimated. RSS is the native workload VmHWM, including a small procfs read used to obtain it. Latency and counting-allocator measurements cover fixture construction, correctness checks (including JSON serialization for the composed-recovery expected value), and output destruction. Each latency case is a separate process. These observations are not hard memory bounds or representative quality evaluation.',
        'rss_observation': 'VmHWM after workload and before final teardown; includes native-only procfs read',
        'runtime': runtime_identity, 'builds': [benchmark_build, cli_build], 'workloads': [],
    }
    for case in cases:
        print(f'Profiling {case}', flush=True)
        report['workloads'].append(profile(executable, case, directory, environment))
    print('Running core latency workloads', flush=True)
    report['latency_workloads'] = run_latency_bench('purrassist-core', directory)
    print('Running syntax latency workloads', flush=True)
    report['syntax_latency_workloads'] = run_latency_bench('purrassist-syntax', directory)
    print('Running the recovery benchmark', flush=True)
    recovery_output = run_recovery_bench(directory)
    report['recovery_benchmark'] = recovery_output
    recovery_path = ROOT / 'docs/qualification/recovery-bench-linux-x86_64.json'
    print('Measuring cold build cost (empty target/build directories, compiler caches disabled)', flush=True)
    report['cold_build'] = cold_build()
    if engine_source_identity() != engine_digest:
        raise ValueError('engine source inputs changed during measurement; rerun on stable sources')
    report['source'] = source_identity()
    report['source']['engine_source_tree_sha256'] = engine_digest
    recovery_path.write_text(json.dumps(recovery_output, indent=2) + '\n')
    print(f'Wrote {recovery_path}', flush=True)
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(report, indent=2) + '\n')
    print(f'Wrote {args.output}; raw logs retained in {directory}', flush=True)


if __name__ == '__main__':
    main()
