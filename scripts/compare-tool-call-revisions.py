#!/usr/bin/env python3
# SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
# SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0
"""Compare tool-call readings against the immutable prior engine.

This is the resynchronization optimization's semantics-preserving acceptance
comparison, not a baseline contract for future capability changes.
Both revisions build the current fuzz driver and consume the same seeds and
bounded deterministic mutations. Only receipt.usage is excluded; readings,
ranking, source spans, provenance, budgets and termination remain compared.
Matching resource refusals are compared as results, never skipped; execution
failures, timeouts, cancellation and mismatches fail the comparison.
"""

import importlib.util
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import tempfile
import time


ROOT = Path(__file__).resolve().parent.parent
BASELINE = 'c30595347c073404af8421ff7602a4a00c3c4c28'
DRIVER = Path('crates/core/examples/fuzz_tool_call.rs')
MUTATIONS = 32
RANDOM_SEED = 0x5A1727EE614D934B
PROCESS_SECONDS = 30
CAMPAIGN_SECONDS = 900


class WireObject(list):
    """Retain ordered object members, including duplicate argument keys."""

    def field_index(self, name):
        indices = [index for index, (key, _) in enumerate(self) if key == name]
        if len(indices) != 1:
            raise ValueError(f'expected one wire envelope field {name!r}, found {len(indices)}')
        return indices[0]

    def field(self, name):
        return self[self.field_index(name)][1]


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
    if isinstance(previous, WireObject):
        keys = [key for key, _ in previous]
        if keys != [key for key, _ in current]:
            return path + ' (object keys or order)'
        for index, ((key, left), (_, right)) in enumerate(zip(previous, current)):
            difference = differing_path(left, right, path + '[' + json.dumps(key) + ']')
            if difference is not None:
                if keys.count(key) > 1:
                    return f'{difference} (duplicate key at object member {index})'
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
    result = run([str(binary), *arguments], input=data,
                 stdout=subprocess.PIPE, stderr=subprocess.PIPE,
                 timeout=min(PROCESS_SECONDS, remaining))
    # Tuples cannot arise from JSON strings, arrays or objects. Preserve
    # numeric lexemes without rounding floats or conflating numbers with strings.
    document = json.loads(result.stdout, object_pairs_hook=WireObject,
                          parse_int=lambda token: (token,),
                          parse_float=lambda token: (token,))
    kind = document.field('receipt').field('termination').field('kind')
    if kind not in {'completed', 'no_candidate', 'budget_exceeded', 'invalid_input'}:
        raise ValueError(f'unexpected recovery termination: {kind}')
    return result.stdout, document, kind


def cases(fuzz):
    """Keep fuzz header associations and require completed restarted samples."""
    for target in ('tool-call', 'tool-call-ref'):
        specification = fuzz.TARGETS[target]
        seeds = specification['seeds']
        arguments = ['--snapshot', *fuzz.probe_arguments(target)]
        for index, data in enumerate(seeds):
            yield f'{target}/seed/{index}', data, arguments, False
        random = fuzz.Random(RANDOM_SEED)
        for index in range(MUTATIONS):
            parent = random.index(len(seeds))
            data = fuzz.mutate(seeds[parent], random, specification['tokens'])
            yield f'{target}/mutation/{index}/parent/{parent}', data, arguments, False
    # Read the exact existing Rust fixture literals. These literals use the
    # JSON-compatible subset of Rust string escapes; no shape is reconstructed.
    source = (ROOT / 'crates/core/tests/tool_call_cost_support/mod.rs').read_text(encoding='utf-8')
    literals = []
    # In a raw regex, \\ matches one source backslash; . consumes its escape.
    string_literal = r'"(?:\\.|[^"\\])*"'
    for name in ('DEEPSEEK_RESTART_SECTION', 'DEEPSEEK_RESTART_TAIL'):
        match = re.search(r'const ' + name + r': &str = (' + string_literal + r');', source)
        if match is None:
            raise ValueError(f'missing exact restarted fixture literal: {name}')
        literals.append(json.loads(match[1]))
    section, tail = literals
    for count in (1, 2, 4, 8):
        data = (section * count + tail).encode('utf-8')
        yield f'restarted-sections/{count}', data, ['--snapshot-restarted'], True


def compare(previous, current, fuzz):
    """Exclude usage only; retain the exact failing input and field path."""
    deadline = time.monotonic() + CAMPAIGN_SECONDS
    counts = {}
    total = 0
    for label, data, arguments, require_completed in cases(fuzz):
        if len(data) > 4096:
            raise ValueError(f'{label}: input exceeds the existing probe limit')
        try:
            _, old, old_kind = snapshot(previous, data, arguments, deadline)
            _, new, new_kind = snapshot(current, data, arguments, deadline)
            if require_completed and (old_kind != 'completed' or new_kind != 'completed'):
                raise ValueError(f'restarted sample must complete on both engines: {old_kind}, {new_kind}')
            for document in (old, new):
                receipt = document.field('receipt')
                del receipt[receipt.field_index('usage')]
            # The field comparison retains object/member order and numeric
            # lexemes; only the explicitly removed usage object is ignored.
            path = differing_path(old, new)
            if path is not None:
                raise ValueError(f'wire mismatch outside receipt.usage at {path}')
        except (OSError, subprocess.SubprocessError, ValueError, KeyError, TypeError, AttributeError) as error:
            detail = error.stderr.decode('utf-8', errors='replace') if isinstance(
                error, subprocess.CalledProcessError) and error.stderr else ''
            raise RuntimeError(f'{label}: {error}\ninput_hex={data.hex()}\n{detail}') from error
        counts[old_kind] = counts.get(old_kind, 0) + 1
        total += 1
    print(f'Compared {total} wire results excluding only receipt.usage; '
          f'termination counts: {json.dumps(counts, sort_keys=True)}')


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
