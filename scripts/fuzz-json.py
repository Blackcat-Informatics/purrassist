#!/usr/bin/env python3
# SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
# SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0
"""Deterministic source-region coverage-guided JSON qualification (LLVM 22.1.8)."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parent.parent
SEEDS = [b'', b'null', b'true', b'10', b'-1.25e+2', b'"\\uD83D\\uDE00"',
         b'{"name":"tea","items":[1,2,3]}', b'{"x":1,"x":2}',
         b'[[{"nmae":"tea","items":["1"]}]]', b'["Tea","coffee"]',
         b'```json\n{"x":1,}\n```', b'{name:\'tea\', items:[1,2',
         b'"{\\"x\\":1}"', b'\xff']
TOKENS = [b'null', b'true', b'false', b'"', b'\\', b'\n', b'\t', b':', b',',
          b'{', b'}', b'[', b']', b'0', b'-1', b'1e9999', b'\\uD800',
          b'"name"', b'"items"', b'"x"', b'"tea"', 'é'.encode(), b'\xff']


class Random:
    """Fixed LCG algorithm; reproducibility does not depend on Python random."""
    def __init__(self, seed):
        self.state = seed

    def index(self, length):
        self.state = (self.state * 6364136223846793005 + 1) & ((1 << 64) - 1)
        return (self.state >> 32) % length


def mutate(data, random):
    for _ in range(1 + random.index(3)):
        start = random.index(len(data) + 1)
        end = start + random.index(len(data) - start + 1)
        token = TOKENS[random.index(len(TOKENS))]
        operation = random.index(5)
        if operation == 0:
            data = data[:start] + token + data[start:]
        elif operation == 1:
            data = data[:start] + data[end:]
        elif operation == 2:
            data = data[:start] + token + data[end:]
        elif operation == 3:
            data = b'[' + data + b']'
        else:
            data = data[:end] + data[start:end] + data[end:]
    return data[:4096]


def covered_regions(document):
    if document.get('type') != 'llvm.coverage.json.export' or document.get('version') != '3.1.0':
        raise ValueError('unsupported LLVM coverage JSON format')
    regions = set()
    core_functions = 0
    for unit in document['data']:
        for function in unit['functions']:
            filenames = function['filenames']
            core_functions += any(Path(name).is_relative_to(ROOT / 'crates/core/src') for name in filenames)
            for region in function['regions']:
                if len(region) != 8 or any(type(value) is not int or value < 0 for value in region):
                    raise ValueError('malformed coverage region')
                line, column, end_line, end_column, count, file_id, _, kind = region
                if file_id >= len(filenames):
                    raise ValueError('coverage file index out of bounds')
                filename = Path(filenames[file_id])
                if count and kind == 0 and filename.is_relative_to(ROOT / 'crates/core/src'):
                    # Coalesce generic instantiations by source coordinates. Count only code regions.
                    regions.add((str(filename.relative_to(ROOT)), line, column, end_line, end_column))
    if not core_functions:
        raise ValueError('coverage export contains no core functions')
    return regions


def tool_identity():
    rustc = subprocess.check_output(['rustc', '-vV'], text=True)
    inventory = json.loads((ROOT / 'vendor/tooling.json').read_text())
    expected = next(entry['version'] for entry in inventory if entry['name'] == 'llvm-coverage')
    versions = {'rustc': rustc.strip()}
    for tool in ('llvm-cov', 'llvm-profdata'):
        versions[tool] = subprocess.check_output([tool, '--version'], text=True).strip()
    for tool, text in versions.items():
        match = re.search(r'LLVM version:?\s+(\S+)', text)
        if not match or match[1] != expected:
            raise ValueError(f'{tool} LLVM version differs from inventory {expected}')
    return versions


def build(directory):
    environment = dict(os.environ)
    flags = environment.get('CARGO_ENCODED_RUSTFLAGS')
    flags = flags.split('\x1f') if flags is not None else environment.get('RUSTFLAGS', '').split()
    environment['CARGO_ENCODED_RUSTFLAGS'] = '\x1f'.join(flags + ['-C', 'instrument-coverage'])
    command = ['cargo', 'build', '-p', 'purrassist-core', '--example', 'fuzz-json', '--locked',
               '--config', 'profile.dev.opt-level=1', '--config', 'profile.dev.debug=1',
               '--config', 'profile.dev.strip=false', '--config', 'profile.dev.debug-assertions=true',
               '--config', 'profile.dev.overflow-checks=true', '--message-format=json']
    result = subprocess.run(command, cwd=ROOT, env=environment, capture_output=True, text=True)
    (directory / 'build.log').write_text(result.stderr)
    result.check_returncode()
    artifacts = [json.loads(line) for line in result.stdout.splitlines()]
    binaries = [row['executable'] for row in artifacts if row.get('target', {}).get('name') == 'fuzz-json' and row.get('executable')]
    if len(binaries) != 1:
        raise ValueError('expected exactly one probe executable')
    binary = Path(binaries[0])
    return binary, {'command': command, 'sha256': hashlib.sha256(binary.read_bytes()).hexdigest(),
                    'inherited_rustflags_sha256': hashlib.sha256('\x1f'.join(flags).encode()).hexdigest(),
                    'appended_rustflags': ['-C', 'instrument-coverage']}


def probe(binary, data, directory, timeout):
    # Remove prior successful profiles so stale counters can never supply feedback.
    raw, indexed = directory / 'current.profraw', directory / 'current.profdata'
    raw.unlink(missing_ok=True)
    indexed.unlink(missing_ok=True)
    (directory / 'current.bin').write_bytes(data)
    (directory / 'probe.stderr').write_bytes(b'')
    environment = dict(os.environ, LLVM_PROFILE_FILE=str(raw), LC_ALL='C')
    result = subprocess.run([str(binary)], input=data, capture_output=True, env=environment, timeout=timeout)
    (directory / 'probe.stderr').write_bytes(result.stderr)
    result.check_returncode()
    expected = b'ok\n'
    try:
        data.decode('utf-8')
    except UnicodeDecodeError:
        expected = b'invalid-utf8\n'
    if result.stdout != expected or result.stderr:
        raise ValueError('probe output differs from its completion protocol')
    merged = subprocess.run(['llvm-profdata', 'merge', '-sparse', str(raw), '-o', str(indexed)],
                            check=True, capture_output=True, timeout=timeout)
    exported = subprocess.run(['llvm-cov', 'export', str(binary), f'-instr-profile={indexed}'],
                              check=True, capture_output=True, timeout=timeout)
    if merged.stderr or exported.stderr:
        raise ValueError('LLVM reported a coverage diagnostic')
    return covered_regions(json.loads(exported.stdout)), expected == b'invalid-utf8\n'


def source_digest():
    names = subprocess.check_output(['git', 'ls-files', '-co', '--exclude-standard', '-z'], cwd=ROOT).split(b'\0')
    digest = hashlib.sha256()
    for name in sorted(set(names) - {b''}):
        path = ROOT / os.fsdecode(name)
        if path.is_file():
            digest.update(name + b'\0' + hashlib.sha256(path.read_bytes()).digest())
    return digest.hexdigest()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--iterations', type=int, default=1000, help='mutations after all fixed seeds')
    parser.add_argument('--seed', type=int, default=0x5A1727EE614D934B)
    parser.add_argument('--timeout', type=float, default=10, help='seconds per probe or LLVM command')
    parser.add_argument('--replay', type=Path, help='execute one saved input instead of a campaign')
    args = parser.parse_args()
    if args.iterations < 0 or not 0 <= args.seed < 1 << 64 or not 0 < args.timeout <= 3600:
        parser.error('invalid iteration, seed, or timeout bound')
    cache = ROOT / '.cache'
    cache.mkdir(exist_ok=True)
    directory = Path(tempfile.mkdtemp(prefix='fuzz-json-', dir=cache))
    (directory / 'corpus').mkdir()
    print(f'Campaign artifacts: {directory}', flush=True)
    tools = tool_identity()
    binary, build_identity = build(directory)
    report = {'format': 'purrassist.fuzz-json.v1', 'status': 'running', 'tools': tools,
              'source_commit': subprocess.check_output(['git', 'rev-parse', 'HEAD'], cwd=ROOT, text=True).strip(),
              'source_dirty': bool(subprocess.check_output(['git', 'status', '--porcelain'], cwd=ROOT)),
              'nonignored_file_tree_sha256': source_digest(),
              'build': build_identity, 'seed': args.seed, 'requested_mutations': args.iterations,
              'timeout_seconds': args.timeout, 'maximum_input_bytes': 4096,
              'executions': 0, 'invalid_utf8': 0, 'retained': [], 'regions': 0}
    corpus, seen = [], set()
    random = Random(args.seed)
    seeds = [args.replay.read_bytes()] if args.replay else SEEDS
    if any(len(data) > 4096 for data in seeds):
        raise ValueError('replay exceeds probe input limit')
    mutations = 0 if args.replay else args.iterations
    try:
        for index in range(len(seeds) + mutations):
            parent = None
            if index < len(seeds):
                data = seeds[index]
            else:
                parent = random.index(len(corpus))
                data = mutate(corpus[parent], random)
            report['current_execution'] = index
            regions, invalid = probe(binary, data, directory, args.timeout)
            novel = regions - seen
            seen.update(regions)
            if novel or index < len(seeds):
                digest = hashlib.sha256(data).hexdigest()
                (directory / 'corpus' / f'{digest}.bin').write_bytes(data)
                corpus.append(data)
                report['retained'].append({'execution': index, 'sha256': digest, 'new_regions': len(novel),
                                           'parent_corpus_index': parent, 'bytes': len(data)})
            report['executions'] += 1
            report['invalid_utf8'] += invalid
            report['regions'] = len(seen)
            if index % 100 == 0:
                print(f'{index + 1} executions; {len(corpus)} corpus entries; {len(seen)} core regions', flush=True)
        if not seen:
            if not args.replay:
                raise ValueError('campaign produced no covered core regions')
        report['status'] = 'passed'
    except (Exception, KeyboardInterrupt) as error:
        report['status'] = 'failed'
        report['failure_type'] = type(error).__name__
        (directory / 'failure.txt').write_text(repr(error) + '\n')
        print(f'Failure retained in {directory / "current.bin"}', flush=True)
        raise
    finally:
        (directory / 'report.json').write_text(json.dumps(report, indent=2) + '\n')
        (directory / 'regions.json').write_text(json.dumps(sorted(seen), indent=2) + '\n')
    print(f'Passed {report["executions"]} inputs; report: {directory / "report.json"}', flush=True)


if __name__ == '__main__':
    main()
