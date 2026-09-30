#!/usr/bin/env python3
# SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
# SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0
"""Record bounded HTML grammar and link-title comparisons; reference limits are observations.

Run against a campaign's immutable private probe, with --report pointing to
its report.json, then archive stdout. An unexpected mismatch exits nonzero.
Known comment disagreements never count as successful independent checks and
do not change fuzz.py's mandatory campaign acceptance profile.
"""
import argparse
import hashlib
import importlib.util
import itertools
import json
import os
import subprocess
import tempfile
from pathlib import Path

spec = importlib.util.spec_from_file_location('fuzz', Path(__file__).with_name('fuzz.py'))
if spec is None or spec.loader is None:
    raise SystemExit('could not load fuzz.py')
FUZZ = importlib.util.module_from_spec(spec)
spec.loader.exec_module(FUZZ)


def forms():
    """Exhaust each chosen boundary dimension, independent of core parsing."""
    result = set()
    for name, space, body in itertools.product(
        ['', 'E', 'EA', 'e', 'Ea', 'eA', 'E1', '1', 'DOCTYPE'],
        ['', ' ', '\t', '\n', '\r', '\x0b', '\x0c', '\u00a0'],
        ['', 'x', '<', '>', '\x00'],
    ):
        result.add('<!' + name + space + body + '>')
    for begin, end, body in itertools.product(
        ['<!--', '<?', '<![CDATA['], ['>', '-->', '?>', ']]>', '--->'],
        ['', 'x', '-', '--', 'a--b', '\n', '\x00'],
    ):
        result.add(begin + body + end)
    for tag, space, tail in itertools.product(
        ['x', 'X', 'x1', '1x', 'x-y', 'x_y'], ['', ' ', '\t', '\n'],
        ['>', '/>', ' a>', ' a="v">', " a='v'>", ' a=v>', ' a=>', ' a="v"b=2>'],
    ):
        result.add('<' + tag + space + tail)
    return sorted(result)


def sources(case_set):
    """Generate either HTML grammar or inline/reference title boundaries."""
    if case_set == 'html':
        for form, wrapper in itertools.product(forms(), ['{}\n', 'a {} b\n', '> a {} b\n', '- a {} b\n']):
            comment_end = form.find('-->', 4) if form.startswith('<!--') else -1
            limited = wrapper != '{}\n' and comment_end > 4 and form[comment_end - 1] == '-'
            yield wrapper.format(form), limited
        return
    if case_set == 'tables':
        for newline, caption, header, delimiter, body, container in itertools.product(
            ['\n', '\r\n'], ['before', '[', 'text *em*'],
            ['a|b', '|a|b|', '|a\\|c|b|', 'a | b'],
            ['|-|-|', '---|---', '|:---|---:|', '|-|'],
            ['', '| 1 | 2 |', '|', ']', 'para', '<x>'], ['', '> ', '- '],
        ):
            continuation = '  ' if container == '- ' else container
            lines = [caption, header, delimiter] + ([body] if body else [])
            source = container + lines[0] + newline
            source += ''.join(continuation + line + newline for line in lines[1:])
            # The pinned extension leaves a header-only table marked as blank,
            # loosening its containing list when a following block exists.
            limited = container == '- ' and delimiter != '|-|' and body in ['|', '<x>']
            yield source, limited
        return
    for label, destination, title, delimiter, image, reference in itertools.product(
        ['tea', 't`ea`', 't*ea*'], ['/tea', '<>', '<a b>', '/a(b)'],
        ['', 'x', ' ', '\t', '\n', '&amp;', '&NewLine;'], ['"', "'", '('],
        [False, True], [False, True],
    ):
        close = ')' if delimiter == '(' else delimiter
        prefix = '!' if image else ''
        title_source = f'{delimiter}{title}{close}'
        if reference:
            source = f'[tea]: {destination} {title_source}\n\n{prefix}[{label}][tea]\n'
        else:
            source = f'{prefix}[{label}]({destination} {title_source})\n'
        yield source, False


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--report', type=Path, required=True)
    parser.add_argument('--case-set', choices=['html', 'links', 'tables'], default='html')
    args = parser.parse_args()
    campaign = json.loads(args.report.read_text())
    probe = args.report.parent / 'probe'
    identity = hashlib.sha256(probe.read_bytes()).hexdigest()
    if identity != campaign['build']['sha256']:
        raise ValueError('private probe differs from recorded campaign executable')
    oracle = FUZZ.ensure_cmark_gfm()
    result = {
        'format': 'purrassist.markdown-html-boundaries.v1',
        'interpretation': 'bounded diagnostic observations; reference limits are not successful comparisons',
        'case_set': args.case_set, 'build': campaign['build'], 'tools': campaign['tools'],
        'diagnostic_sha256': hashlib.sha256(Path(__file__).read_bytes()).hexdigest(),
        'executions': 0, 'differential_checks': 0, 'reference_limits': [], 'unexpected_mismatches': [],
    }
    with tempfile.TemporaryDirectory(prefix='purrassist-html-boundaries-') as profile_directory:
        environment = dict(os.environ, LLVM_PROFILE_FILE=str(Path(profile_directory) / 'probe.profraw'))
        environment.pop('PURRASSIST_FUZZ_TASKLIST_REFERENCE_PROFILE', None)
        # Known limits are independently selected before either renderer runs.
        for source, limited in sources(args.case_set):
            process = subprocess.run([str(probe)], input=source.encode(), capture_output=True, env=environment, timeout=10)
            if process.returncode:
                raise ValueError(f'probe failed on {source!r}: {process.stderr!r}')
            record = json.loads(process.stdout.splitlines()[1])
            result['executions'] += 1
            try:
                FUZZ.check_markdown_differential(record, oracle, 10)
                result['differential_checks'] += 1
            except FUZZ.DifferentialMismatch as error:
                entry = {'source': source, 'reason': ('header-only table incorrectly loosens containing list' if args.case_set == 'tables' else 'inline HTML comment trailing-hyphen scanner restriction') if limited else 'unexpected disagreement', 'source_sha256': hashlib.sha256(source.encode()).hexdigest(), 'diagnostic': str(error)}
                key = 'reference_limits' if limited else 'unexpected_mismatches'
                result[key].append(entry)
    print(json.dumps(result, indent=2))
    if result['unexpected_mismatches']:
        raise SystemExit(1)


if __name__ == '__main__':
    main()
