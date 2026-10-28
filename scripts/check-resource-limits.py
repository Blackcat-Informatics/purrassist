#!/usr/bin/env python3
# SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
# SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0
"""Validate resource report structure and, unless --schema-only, fresh limits.

Hygiene checks committed evidence structure. Qualification checks fresh measured
values against the JSON limits in docs/RESOURCES.md.
"""
import argparse
import json
import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
DOC = ROOT / 'docs/RESOURCES.md'

REQUIRED_TOP_LEVEL_KEYS = (
    'format', 'source', 'platform', 'tools', 'scope', 'rss_observation', 'runtime',
    'builds', 'workloads', 'latency_workloads', 'syntax_latency_workloads',
    'recovery_benchmark', 'cold_build',
)
REQUIRED_WORKLOAD_KEYS = (
    'case', 'process_elapsed_ns', 'process_peak_rss_bytes', 'allocation_calls', 'free_calls',
    'total_allocated_bytes', 'observed_peak_requested_heap_bytes',
    'observed_peak_heap_plus_estimated_overhead_bytes', 'snapshots',
)
REQUIRED_LATENCY_KEYS = ('case', 'samples', 'median_ns', 'p95_ns', 'allocation_calls', 'peak_live_bytes', 'process_peak_rss_bytes')
REQUIRED_SYNTAX_LATENCY_KEYS = (
    'case', 'samples', 'median_ns', 'p95_ns', 'retained_bytes', 'peak_bytes', 'max_overshoot_bytes',
    'tree_allocation_calls', 'rust_allocation_calls', 'rust_peak_live_bytes', 'process_peak_rss_bytes',
)


def load_limits():
    """Extract the first ```json fence following the "## Resource limits"
    heading in docs/RESOURCES.md; this is the single source of truth both
    the doc's rationale prose and this script read."""
    text = DOC.read_text()
    heading = text.find('## Resource limits')
    if heading == -1:
        raise SystemExit(f'{DOC}: missing "## Resource limits" heading')
    match = re.search(r'```json\n(.*?)\n```', text[heading:], re.DOTALL)
    if not match:
        raise SystemExit(f'{DOC}: no ```json fence found after "## Resource limits"')
    return json.loads(match.group(1))


PROFILING_CASES = {'empty', 'schema_compile', 'json_parse', 'json_composed', 'json_ambiguous',
                   'markdown_render', 'markdown_list', 'unicode', 'matching'}
LATENCY_CASES = {
    'schema_compile', 'schema_validate', 'schema_recover', 'matching_rank', 'matching_resolve',
    'paths_resolve', 'pattern_compile', 'pattern_match_derivative', 'pattern_match_backtracking',
    'edits_exact', 'edits_fuzzy', 'edits_unified', 'markdown_render', 'markdown_insert_section',
    'markdown_remove_section', 'markdown_move_section', 'markdown_rename_heading',
    'markdown_list_edit', 'markdown_list_renumber', 'markdown_table_row_move',
    'markdown_table_column_move', 'markdown_reference_add', 'markdown_reference_relabel',
    'markdown_reference_remove', 'markdown_reference_unlink',
}
SYNTAX_CASES = {'python', 'rust', 'javascript', 'typescript', 'tsx'}
RECOVERY_CASES = {'exact', 'nested', 'composed', 'duplicate', 'inferred', 'no_candidate'}
RECOVERY_KEYS = ('case', 'samples', 'correct', 'wrong_corrections', 'median_ns', 'p95_ns',
                 'governor_peak_scratch_bytes', 'governor_peak_work')


def require_keys(entry, keys, where):
    if not isinstance(entry, dict):
        raise SystemExit(f'{where}: expected object')
    missing = [key for key in keys if key not in entry]
    if missing:
        raise SystemExit(f'{where}: missing field(s) {missing}')


def unsigned(value, where, positive=False):
    if type(value) is not int or value < (1 if positive else 0):
        raise SystemExit(f'{where}: expected {"positive" if positive else "nonnegative"} integer')


def nonempty_text(value, where):
    if not isinstance(value, str) or not value.strip():
        raise SystemExit(f'{where}: expected nonempty string')


def digest(value, where, length=64):
    if not isinstance(value, str) or not re.fullmatch(f'[0-9a-f]{{{length}}}', value):
        raise SystemExit(f'{where}: expected lowercase hexadecimal digest')


def artifact(entry, where):
    require_keys(entry, ('sha256',), where)
    digest(entry['sha256'], where + '.sha256')


def workloads(rows, keys, expected, where):
    if not isinstance(rows, list):
        raise SystemExit(f'{where}: expected array')
    cases = []
    for row in rows:
        require_keys(row, keys, where)
        name = row['case']
        nonempty_text(name, where + '.case')
        cases.append(name)
        for key in keys:
            if key != 'case':
                unsigned(row[key], f'{where}.{name}.{key}')
        if 'samples' in keys:
            if row['samples'] != 200 or row['median_ns'] <= 0 or row['p95_ns'] < row['median_ns']:
                raise SystemExit(f'{where}.{name}: invalid sample count or latency quantiles')
        if 'process_peak_rss_bytes' in keys:
            unsigned(row['process_peak_rss_bytes'], f'{where}.{name}.process_peak_rss_bytes', positive=True)
    if len(cases) != len(set(cases)) or set(cases) != expected:
        raise SystemExit(f'{where}: duplicated, missing, or unexpected cases; expected {sorted(expected)}')


def check_schema(report):
    require_keys(report, REQUIRED_TOP_LEVEL_KEYS, 'report')
    if report['format'] != 'purrassist.resources.v2':
        raise SystemExit('report: expected current format purrassist.resources.v2; regenerate older evidence')
    require_keys(report['source'], ('commit', 'dirty', 'nonignored_file_tree_sha256',
                                 'engine_source_tree_sha256'), 'source')
    digest(report['source']['commit'], 'source.commit', 40)
    digest(report['source']['nonignored_file_tree_sha256'], 'source.nonignored_file_tree_sha256')
    digest(report['source']['engine_source_tree_sha256'], 'source.engine_source_tree_sha256')
    if type(report['source']['dirty']) is not bool:
        raise SystemExit('source.dirty: expected boolean')
    require_keys(report['platform'], ('system', 'architecture', 'libc'), 'platform')
    for key in ('system', 'architecture'):
        nonempty_text(report['platform'][key], 'platform.' + key)
    if report['platform']['system'] != 'Linux':
        raise SystemExit('platform.system: this report format qualifies Linux only')
    libc = report['platform']['libc']
    if not isinstance(libc, list) or len(libc) != 2 or any(not isinstance(value, str) for value in libc):
        raise SystemExit('platform.libc: expected two runtime identity strings')
    require_keys(report['tools'], ('valgrind', 'rustc'), 'tools')
    for key, value in report['tools'].items():
        nonempty_text(value, 'tools.' + key)
    for key in ('scope', 'rss_observation'):
        nonempty_text(report[key], key)
    require_keys(report['runtime'], ('kind',), 'runtime')
    nonempty_text(report['runtime']['kind'], 'runtime.kind')
    if 'archive_sha256' in report['runtime']:
        require_keys(report['runtime'], ('name', 'version', 'files_sha256', 'extractor'), 'runtime')
        digest(report['runtime']['archive_sha256'], 'runtime.archive_sha256')
        for key in ('name', 'version', 'extractor'):
            nonempty_text(report['runtime'][key], 'runtime.' + key)
        members = report['runtime']['files_sha256']
        if not isinstance(members, dict) or not members:
            raise SystemExit('runtime.files_sha256: expected nonempty member identity map')
        for key, value in members.items():
            nonempty_text(key, 'runtime.files_sha256 member')
            digest(value, 'runtime.files_sha256.' + key)
    if not isinstance(report['builds'], list) or len(report['builds']) != 2:
        raise SystemExit('builds: expected benchmark and CLI build evidence')
    targets = []
    for row in report['builds']:
        require_keys(row, ('target', 'command', 'elapsed_ns', 'cache_state', 'fresh_artifacts',
                           'built_artifacts', 'profile', 'executable_sha256'), 'builds')
        unsigned(row['elapsed_ns'], 'builds.elapsed_ns', positive=True)
        for key in ('fresh_artifacts', 'built_artifacts'):
            unsigned(row[key], 'builds.' + key)
        if not row['fresh_artifacts'] + row['built_artifacts']:
            raise SystemExit('builds: missing compiler-artifact observations')
        targets.append(row['target'])
        nonempty_text(row['cache_state'], 'builds.cache_state')
        if not isinstance(row['command'], list) or not row['command']:
            raise SystemExit('builds.command: expected nonempty argument array')
        for argument in row['command']:
            nonempty_text(argument, 'builds.command argument')
        if not isinstance(row['profile'], dict):
            raise SystemExit('builds.profile: expected Cargo profile object')
        digest(row['executable_sha256'], 'builds.executable_sha256')
    if sorted(targets) != ['purrassist', 'resources']:
        raise SystemExit('builds: expected one resources and one purrassist artifact')
    workloads(report['workloads'], REQUIRED_WORKLOAD_KEYS, PROFILING_CASES, 'workloads')
    for row in report['workloads']:
        if row['observed_peak_requested_heap_bytes'] > row['observed_peak_heap_plus_estimated_overhead_bytes']:
            raise SystemExit('workloads: requested heap exceeds heap including overhead')
        unsigned(row['snapshots'], 'workloads.snapshots', positive=True)
    workloads(report['latency_workloads'], REQUIRED_LATENCY_KEYS, LATENCY_CASES, 'latency_workloads')
    workloads(report['syntax_latency_workloads'], REQUIRED_SYNTAX_LATENCY_KEYS, SYNTAX_CASES, 'syntax_latency_workloads')
    for row in report['syntax_latency_workloads']:
        if row['retained_bytes'] > row['peak_bytes']:
            raise SystemExit('syntax_latency_workloads: retained heap exceeds peak heap')
    require_keys(report['cold_build'], ('method', 'elapsed_ns', 'fresh_target_directory',
                 'fresh_build_directory', 'compiler_cache_disabled', 'fresh_artifacts',
                 'built_artifacts', 'artifact'), 'cold_build')
    cold = report['cold_build']
    nonempty_text(cold['method'], 'cold_build.method')
    for key in ('fresh_target_directory', 'fresh_build_directory', 'compiler_cache_disabled'):
        if cold[key] is not True:
            raise SystemExit('cold_build.' + key + ': expected true')
    unsigned(cold['elapsed_ns'], 'cold_build.elapsed_ns', positive=True)
    unsigned(cold['built_artifacts'], 'cold_build.built_artifacts', positive=True)
    if type(cold['fresh_artifacts']) is not int or cold['fresh_artifacts'] != 0:
        raise SystemExit('cold_build.fresh_artifacts: cold build reused compiled artifacts')
    artifact(cold['artifact'], 'cold_build.artifact')
    workloads(report['recovery_benchmark'], RECOVERY_KEYS, RECOVERY_CASES, 'recovery_benchmark')
    for row in report['recovery_benchmark']:
        if row['correct'] != row['samples'] or row['wrong_corrections'] != 0:
            raise SystemExit('recovery_benchmark: correctness failure')


def check_limits(report, limits):
    violations = []

    def check(actual, limit, where):
        if actual > limit:
            violations.append(f'{where}: {actual} exceeds limit {limit}')

    check(report['cold_build']['elapsed_ns'], limits['cold_build_elapsed_ns'], 'cold_build.elapsed_ns')
    for workload in report['latency_workloads']:
        for field, limit in limits['latency_workloads'].items():
            check(workload[field], limit, f"latency_workloads.{workload['case']}.{field}")
    for workload in report['syntax_latency_workloads']:
        for field, limit in limits['syntax_latency_workloads'].items():
            check(workload[field], limit, f"syntax_latency_workloads.{workload['case']}.{field}")
    workload_limits = {
        'process_peak_rss_bytes': limits['workloads']['process_peak_rss_bytes'],
        'total_allocated_bytes': limits['workloads']['total_allocated_bytes'],
        'observed_peak_heap_plus_estimated_overhead_bytes':
            limits['workloads']['observed_peak_heap_plus_estimated_overhead_bytes'],
    }
    for workload in report['workloads']:
        for field, limit in workload_limits.items():
            check(workload[field], limit, f"workloads.{workload['case']}.{field}")
    return violations


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('report', type=Path, help='resource-report.json path (fresh or committed)')
    parser.add_argument('--schema-only', action='store_true', help='validate committed evidence without applying fresh-run limits')
    args = parser.parse_args()
    report = json.loads(args.report.read_text())
    check_schema(report)
    if args.schema_only:
        print(f'{args.report}: schema valid')
        return
    limits = load_limits()
    violations = check_limits(report, limits)
    if violations:
        for violation in violations:
            print(violation, file=sys.stderr)
        raise SystemExit(f'{len(violations)} resource limit violation(s)')
    print(f'{args.report}: schema valid, all measurements within docs/RESOURCES.md limits')


if __name__ == '__main__':
    main()
