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
import subprocess
import sys
import tempfile
import time
import urllib.request

ROOT = Path(__file__).resolve().parent.parent


def memcheck(text):
    match = re.search(r'total heap usage: ([\d,]+) allocs, ([\d,]+) frees, ([\d,]+) bytes allocated', text)
    if not match:
        raise ValueError('missing Memcheck allocation summary')
    errors = re.search(r'ERROR SUMMARY: ([\d,]+) errors', text)
    if not errors or int(errors[1].replace(',', '')):
        raise ValueError('missing or nonzero Memcheck error summary')
    return dict(zip(('allocation_calls', 'free_calls', 'total_allocated_bytes'),
                    (int(value.replace(',', '')) for value in match.groups())))


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
        'executable_bytes': binary.stat().st_size,
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
    for command in commands:
        result = subprocess.run(command + executable + [case], cwd=ROOT, env=environment,
                                stdout=subprocess.PIPE, stderr=subprocess.PIPE, check=True)
        verify_output(result.stdout, case)
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
    loader = cache / 'usr/lib/ld-linux-x86-64.so.2'
    return [str(loader), '--library-path', str(cache / 'usr/lib'), str(binary)], {
        'kind': 'isolated pinned loader and libc; other libraries supplied by host',
        'name': manifest['name'], 'version': manifest['version'],
        'archive_sha256': manifest['sha256'], 'files_sha256': files,
        'extractor': subprocess.check_output(['bsdtar', '--version'], text=True).strip(),
    }


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--output', type=Path, default=ROOT / '.cache/resource-report.json')
    parser.add_argument('--portable-runtime', action='store_true', help='use the pinned isolated x86_64 glibc runtime for profiler compatibility')
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
    environment = dict(os.environ, LC_ALL='C')
    binary, benchmark_build = build(['cargo', 'bench', '-p', 'purrassist-core', '--bench', 'resources', '--no-run', '--locked', '--config', 'profile.bench.debug=1', '--config', 'profile.bench.strip=false'], 'resources', directory)
    _, cli_build = build(['cargo', 'build', '-p', 'purrassist-cli', '--release', '--locked'], 'purrassist', directory)
    executable, runtime_identity = runtime(binary, args.portable_runtime)
    cases = subprocess.check_output([str(binary), '--list'], text=True).splitlines()
    if not cases or len(set(cases)) != len(cases) or any(not re.fullmatch(r'[a-z_]+', case) for case in cases):
        raise ValueError('invalid workload inventory')
    report = {
        'format': 'purrassist.resources.v1', 'source': source_identity(),
        'platform': {'system': 'Linux', 'architecture': platform.machine(), 'libc': platform.libc_ver()},
        'tools': {'valgrind': version, 'rustc': subprocess.check_output(['rustc', '--version'], text=True).strip()},
        'scope': 'One process per workload and profiler; includes startup, construction, assertions, output, and teardown. Native elapsed time is not per-operation latency. Heap excludes stacks and mappings; Massif overhead is estimated. RSS is the native workload VmHWM, including a small procfs read used to obtain it. These observations are not hard memory bounds or representative quality evaluation.',
        'rss_observation': 'VmHWM after workload and before final teardown; includes native-only procfs read',
        'runtime': runtime_identity, 'builds': [benchmark_build, cli_build], 'workloads': [],
    }
    for case in cases:
        print(f'Profiling {case}', flush=True)
        report['workloads'].append(profile(executable, case, directory, environment))
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(report, indent=2) + '\n')
    print(f'Wrote {args.output}; raw logs retained in {directory}', flush=True)


if __name__ == '__main__':
    main()
