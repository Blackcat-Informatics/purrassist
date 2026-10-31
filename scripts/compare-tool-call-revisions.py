#!/usr/bin/env python3
# SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
# SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0
"""Compare complete tool-call wire results against the immutable prior engine.

This is the repair-search optimization's semantics-preserving acceptance
comparison, not a baseline contract for future capability changes.
Both revisions build the current fuzz driver and consume the same seeds and
bounded deterministic mutations. No wire fields, including usage, are removed.
Matching resource refusals are compared as results, never skipped; execution
failures, timeouts, cancellation and mismatches fail the comparison.
"""

import importlib.util
import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import time


ROOT = Path(__file__).resolve().parent.parent
BASELINE = '28de2a4b4acd6dd837badba521d547aeb0676801'
DRIVER = Path('crates/core/examples/fuzz_tool_call.rs')
MUTATIONS = 32
RANDOM_SEED = 0x5A1727EE614D934B
PROCESS_SECONDS = 30
CAMPAIGN_SECONDS = 900


def run(command, *, cwd=ROOT, timeout=300, **kwargs):
    """Run one required command, preserving failures and bounding its duration."""
    return subprocess.run(command, cwd=cwd, timeout=timeout, check=True, **kwargs)


def build(checkout, target):
    """Build the same existing probe against one revision's actual library."""
    environment = dict(os.environ, CARGO_TARGET_DIR=str(target))
    run(['cargo', 'build', '--release', '--locked', '-p', 'purrassist-core',
         '--example', 'fuzz-tool-call'], cwd=checkout, env=environment, timeout=600)
    return target / 'release/examples/fuzz-tool-call'


def differing_path(previous, current, path='$'):
    """Locate the first structural difference without normalizing wire bytes."""
    if type(previous) is not type(current):
        return path
    if isinstance(previous, dict):
        if previous.keys() != current.keys():
            return path + ' (object keys)'
        for key in previous:
            difference = differing_path(previous[key], current[key], path + '[' + json.dumps(key) + ']')
            if difference is not None:
                return difference
    elif isinstance(previous, list):
        if len(previous) != len(current):
            return path + ' (array length)'
        for index, (left, right) in enumerate(zip(previous, current)):
            difference = differing_path(left, right, f'{path}[{index}]')
            if difference is not None:
                return difference
    elif previous != current:
        return path
    return None


def snapshot(binary, data, arguments, deadline):
    """Execute exactly one recovery; require its complete structured result."""
    remaining = deadline - time.monotonic()
    if remaining <= 0:
        raise TimeoutError('cross-revision campaign exceeded its time bound')
    result = run([str(binary), '--snapshot', *arguments], input=data,
                 stdout=subprocess.PIPE, stderr=subprocess.PIPE,
                 timeout=min(PROCESS_SECONDS, remaining))
    document = json.loads(result.stdout)
    kind = document['receipt']['termination']['kind']
    if kind not in {'completed', 'no_candidate', 'budget_exceeded', 'invalid_input'}:
        raise ValueError(f'unexpected recovery termination: {kind}')
    return result.stdout, document, kind


def cases(fuzz):
    """Keep every existing header association and the exact original slow input."""
    for target in ('tool-call', 'tool-call-ref'):
        specification = fuzz.TARGETS[target]
        seeds = specification['seeds']
        arguments = fuzz.probe_arguments(target)
        for index, data in enumerate(seeds):
            yield f'{target}/seed/{index}', data, arguments
        random = fuzz.Random(RANDOM_SEED)
        for index in range(MUTATIONS):
            parent = random.index(len(seeds))
            data = fuzz.mutate(seeds[parent], random, specification['tokens'])
            yield f'{target}/mutation/{index}/parent/{parent}', data, arguments
    # Preserve the issue's original four-byte prefix, in addition to the
    # sized requests already appended to the existing ref seed set.
    yield 'tool-call-ref/original-repair-reproduction', (
        bytes.fromhex('5b5b0052') + fuzz._REF_REPAIR_COST_TURN
    ), ['--tools', 'ref']


def compare(previous, current, fuzz):
    """Compare every byte, retaining the exact input and path on failure."""
    deadline = time.monotonic() + CAMPAIGN_SECONDS
    counts = {}
    total = 0
    for label, data, arguments in cases(fuzz):
        if len(data) > 4096:
            raise ValueError(f'{label}: input exceeds the existing probe limit')
        try:
            old_wire, old, old_kind = snapshot(previous, data, arguments, deadline)
            new_wire, new, _ = snapshot(current, data, arguments, deadline)
            if old_wire != new_wire:
                path = differing_path(old, new) or '$ (wire serialization)'
                raise ValueError(f'complete wire mismatch at {path}')
        except (OSError, subprocess.SubprocessError, ValueError, KeyError, TypeError) as error:
            detail = error.stderr.decode('utf-8', errors='replace') if isinstance(
                error, subprocess.CalledProcessError) and error.stderr else ''
            raise RuntimeError(f'{label}: {error}\ninput_hex={data.hex()}\n{detail}') from error
        counts[old_kind] = counts.get(old_kind, 0) + 1
        total += 1
    print(f'Compared {total} complete wire results; termination counts: {json.dumps(counts, sort_keys=True)}')


def main():
    specification = importlib.util.spec_from_file_location('tool_call_fuzz_inputs', ROOT / 'scripts/fuzz.py')
    if specification is None or specification.loader is None:
        raise RuntimeError('cannot load the existing fuzz input definitions')
    fuzz = importlib.util.module_from_spec(specification)
    specification.loader.exec_module(fuzz)
    common = Path(run(['git', 'rev-parse', '--path-format=absolute', '--git-common-dir'],
                      stdout=subprocess.PIPE, encoding='utf-8').stdout.strip())
    worktrees = common.parent / '.worktrees'
    worktrees.mkdir(exist_ok=True)
    baseline = Path(tempfile.mkdtemp(prefix='tool-call-previous-', dir=worktrees))
    baseline.rmdir()
    # Fetch precisely the immutable source identity rather than a moving branch.
    run(['git', 'fetch', '--no-tags', 'origin', BASELINE])
    run(['git', 'worktree', 'add', '--detach', str(baseline), BASELINE])
    try:
        shutil.copyfile(ROOT / DRIVER, baseline / DRIVER)
        cache = ROOT / '.cache'
        cache.mkdir(exist_ok=True)
        with tempfile.TemporaryDirectory(prefix='tool-call-differential-', dir=cache) as directory:
            targets = Path(directory)
            previous = build(baseline, targets / 'previous')
            current = build(ROOT, targets / 'current')
            head = run(['git', 'rev-parse', 'HEAD'], stdout=subprocess.PIPE,
                       encoding='utf-8').stdout.strip()
            print(f'Previous engine: {BASELINE}; current engine: {head}; mutation seed: {RANDOM_SEED}', flush=True)
            compare(previous, current, fuzz)
    finally:
        run(['git', 'worktree', 'remove', '--force', str(baseline)])


if __name__ == '__main__':
    main()
