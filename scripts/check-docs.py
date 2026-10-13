#!/usr/bin/env python3
# SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
# SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0
"""Check that the Markdown documentation states facts that match their sources.

Prose restates facts (the release set, the version, the fuzz targets, the
receipt wire version, the dialect ids) that no compiler reads. Each is read
here from the file that owns it, never hard-coded, and compared with every
sentence that restates it:

* the release set and order from `RELEASE_ORDER` in `scripts/check-release.py`;
* the workspace version from `Cargo.toml`;
* the fuzz targets from `TARGETS` in `scripts/fuzz.py`, cross-checked against
  the `Makefile`;
* the receipt wire version from `RECEIPT_VERSION` in `crates/core/src/contract.rs`;
* the dialect ids from the stems of `docs/tool-calls/dialects/*.md`;
* the known limitations from `docs/known-limitations.json`, in the
  qualification record, the fuzzing guide and the `### Known limitations`
  subsection of the current version's changelog section (the release body),
  and the targets on which the default `make fuzz` stops from the entries'
  `fuzz_stop_targets`;
* the changelog version, crates and interfaces, the supported-versions table
  and reporting channels of `SECURITY.md`, the reservation table and the
  qualification-workflow summary of `docs/RELEASE.md` (from the `name:` of every
  named step of `.github/workflows/release-qualification.yml`, where a `run:`
  step without a `name:` is an error), and the README link to the changelog;
* the release toolchain: every `RUSTUP_TOOLCHAIN=<x>` and every patch-form `Rust <x.y.z>` in
  `README.md`, `docs/RELEASE.md` (except its dated `## 0.1.0 publication record`) and
  `docs/QUALIFICATION.md` equals the stable pin of `release-qualification.yml`, read through
  the extractor of `scripts/check-release.py`;
* the tool names from the generated block of `docs/TOOLS.md` (the Rust test
  `tools_reference_matches_registry` pins that block to the registry);
* the crate map, the workspace boundaries and the packaged READMEs from the
  workspace members and `cargo metadata`, and the repository links of those
  READMEs and of each `documentation` field, which must name `rust-v<version>`
  and resolve in the tree;
* the three-interfaces block of the README, including its MCP client
  configuration, and the MCP Registry name in the cli README, derived from the
  workspace `repository` URL.

It also scans the Rust sources the published crates ship: the `src/` tree of
every publishable workspace package (from `cargo metadata`) must not carry
process jargon of the form "task N" or "requirement N", with the same task pattern as the
Markdown ban; a reference wrapped across comment lines is found.
`tests/`, `benches/` and `examples/` are outside that rule, because only
`src/` renders on docs.rs.

Every fact is found by a regular expression over the whitespace-collapsed text,
and a regular expression that matches nothing is itself an error ("fact
sentence not found"), so rewording a sentence cannot silently disable a check.
The scope is every tracked `*.md` file outside `.stage/`, `vendor/` and
`crates/core/tests/data/`. Every error starts with the name of its rule in
brackets.
"""
import functools
import hashlib
import importlib.util
import json
from pathlib import Path
import re
import struct
import subprocess
import sys
import tomllib
import urllib.parse

ROOT = Path(__file__).resolve().parent.parent
EXCLUDED_PREFIXES = ('.stage/', 'vendor/', 'crates/core/tests/data/')

NUMBER_WORDS = {word: number for number, word in enumerate((
    'zero', 'one', 'two', 'three', 'four', 'five', 'six', 'seven', 'eight', 'nine', 'ten', 'eleven',
    'twelve', 'thirteen', 'fourteen', 'fifteen', 'sixteen', 'seventeen', 'eighteen', 'nineteen', 'twenty'))}
NUMBER = '|'.join(NUMBER_WORDS)

# A numbered reference to a work item ("Task 12"). Shared by the Markdown ban and
# the published-source rule, so the two cannot drift.
TASK_NUMBER = r'\btask \d+\b'

# A numbered reference to a requirement of a plan that the shipped sources never
# define ("requirement 2"). Banned in published sources with `TASK_NUMBER`.
REQUIREMENT_NUMBER = r'\brequirements? \d+\b'
SOURCE_JARGON = (TASK_NUMBER, REQUIREMENT_NUMBER)
COMMENT_LINE = re.compile(r'\s*//[/!]?(.*)$')

# Phrases that mark stale or process-bound wording. Matched case-insensitively
# after every run of whitespace is collapsed to one space, so a phrase wrapped
# across a line break is still found.
BANNED_PHRASES = (
    r'issue branch',
    r'nine (mutation |coverage-guided )?targets',
    r'wire v2',
    r'reviewed merge commit',
    r'follows? (the )?review and merge',
    r'after review and merge',
    r'no placeholder record',
    r'not yet done',
    r'five of six',
    r'Pre-release implementation',
    r'stage report',
    r'proved by the [^.]*hosted',
    r'uploaded manually',
    r'compares the output byte for byte',
    r'every result carries',
    TASK_NUMBER,
)

# The `[X.Y.Z]` changelog section is also the body of the GitHub release, so it must not
# carry pre-tag status or claim a publication that has not happened. The heading's
# status (`- unreleased`, or the release date) is not part of the body.
RELEASE_BODY_BANNED = (r'not yet tagged', r'unreleased', r'published in this order')

KNOWN_LIMITATIONS_JSON = 'docs/known-limitations.json'
KNOWN_LIMITATIONS_HEADING = '## Known limitations'
CHANGELOG_LIMITATIONS_HEADING = '### Known limitations'
LIMITATION_KEYS = {'issue', 'title', 'impact', 'blocks_release'}
STOP_TARGETS_KEY = 'fuzz_stop_targets'
BASELINE_HEADING = '### Recorded 10,000-mutation baselines'
DIALECT_PAGES = 'docs/tool-calls/dialects'

# Documents that must keep a dialect list (or table) and a dialect count, so
# that deleting the sentence cannot disable the check. Any other document that
# lists dialects is checked as well.
DIALECT_LIST_DOCS = (
    'README.md', 'crates/core/README.md', 'crates/purrassist/README.md', 'crates/host/README.md',
    'docs/QUALIFICATION.md', 'docs/IMPLEMENTATION.md', 'docs/MATCHING-QUALITY.md',
    'docs/tool-calls/README.md', 'docs/tool-calls/GRAMMAR.md')
DIALECT_COUNT_DOCS = (
    'README.md', 'crates/core/README.md', 'crates/purrassist/README.md', 'crates/host/README.md',
    'docs/QUALIFICATION.md', 'docs/IMPLEMENTATION.md', 'docs/RESOURCES.md',
    'docs/tool-calls/README.md', 'docs/tool-calls/GRAMMAR.md', 'docs/tool-calls/dialects/bare-json.v1.md')
CHANGELOG = 'CHANGELOG.md'
SECURITY = 'SECURITY.md'
SECURITY_EMAIL = 'security@blackcatinformatics.ca'
RELEASE_DOC = 'docs/RELEASE.md'
RESERVATIONS_HEADING = '## Name reservations'
QUALIFICATION_HEADING = '## Qualification workflow (release-qualification.yml)'
QUALIFICATION_WORKFLOW = '.github/workflows/release-qualification.yml'
TOOLCHAIN_DOCS = ('README.md', RELEASE_DOC, 'docs/QUALIFICATION.md')
PUBLICATION_RECORD_HEADING = '## 0.1.0 publication record'
TOOLCHAIN_MENTION = re.compile(r'RUSTUP_TOOLCHAIN=([^\s`\'")]+)|\bRust\s+(\d+\.\d+\.\d+)\b')
CHANGELOG_INTERFACES = ('**Rust crate**', '**MCP server**', '**CLI**')
TOOLS_DOC = 'docs/TOOLS.md'
TOOLS_BEGIN = '<!-- tools-reference:begin -->\n'
TOOLS_END = '<!-- tools-reference:end -->\n'
INTERFACES_BEGIN = '<!-- three-interfaces:begin -->\n'
INTERFACES_END = '<!-- three-interfaces:end -->\n'
INTERFACE_HEADINGS = ('### Rust crate', '### MCP server', '### CLI')
CLI_PACKAGE = 'purrassist-cli'
REGISTRY_PROJECT = 'purrassist'
PNG_SIGNATURE = b'\x89PNG\r\n\x1a\n'
SOCIAL_WIDTH, SOCIAL_HEIGHT, SOCIAL_MAX_BYTES = 1280, 640, 1_000_000
SOCIAL_FONT = 'Noto Sans'
# The test that backs the sentence "the CLI and MCP results are identical".
EQUIVALENCE_TEST = 'every_registry_entry_has_a_fixture_validated_through_cli_and_mcp'
PROTOCOL_TESTS = 'crates/cli/tests/protocol.rs'

# Documents that restate the status line "not yet tagged `rust-vX.Y.Z`" (or, once the
# tag is published, "published as `rust-vX.Y.Z`"). All of them use the same form.
STATUS_DOCS = ('README.md', 'SECURITY.md', 'docs/IMPLEMENTATION.md', 'docs/QUALIFICATION.md', 'docs/RELEASE.md')
STATUS_FORMS = ('not yet tagged', 'published as')
# What a document says when no known limitation stops the default `make fuzz`.
NO_STOP_SENTENCE = 'stops on no known signature'

DIALECT_ID = r'[a-z][a-z0-9.-]*\.v\d+'
DIALECT_TOKEN = rf'(?:\[`{DIALECT_ID}`\]\([^)]*\)|`{DIALECT_ID}`)'
DIALECT_SEPARATOR = r'\s*(?:,\s*(?:and\s+|then\s+)?|and\s+|then\s+)?'
DIALECT_RUN = re.compile(rf'{DIALECT_TOKEN}(?:{DIALECT_SEPARATOR}{DIALECT_TOKEN})+')
DIALECT_RUN_MINIMUM = 6
DIALECT_ROW = re.compile(rf'^\|\s*(?:\[)?`({DIALECT_ID})`')
# A count of all the dialects: "all twelve built-in dialects", "all twelve dialects" (before
# "in one turn" or a closing parenthesis), "over twelve built-in dialects", "Twelve dialects ship / are built in / are written", "twelve ship (".
# A subset ("across two dialects", "all six dialects" of one envelope) is not matched.
DIALECT_COUNT = re.compile(
    rf'\ball ({NUMBER}) (?:built-in dialects\b|dialects(?= in one turn|\)))|\bover ({NUMBER}) built-in dialects\b'
    rf'|\b({NUMBER}) (?:built-in )?dialects (?:ship|are built in|are written)\b|\b({NUMBER}) ship \(',
    re.IGNORECASE)

# `Verify run 123` is the label; the sentence also carries the run URL and the commit it ran on.
RUN_URL = re.compile(r'actions/runs/(\d+)')
COMMIT = re.compile(r'\b[0-9a-f]{40}\b')


def load_module(path, name):
    """Import a script by path (the `test-check-release.py` precedent)."""
    spec = importlib.util.spec_from_file_location(name, path)
    if spec is None or spec.loader is None:
        raise SystemExit(f'could not load module spec for {path}')
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def number_value(word):
    """The integer a count written as a word or digits denotes, or None."""
    word = word.lower()
    if word in NUMBER_WORDS:
        return NUMBER_WORDS[word]
    return int(word) if word.isdigit() else None


def collapse_offsets(text):
    """For each character of `collapse(text)`, its offset in `text` (for line numbers)."""
    offsets, last = [], 0
    for run in re.finditer(r'\s+', text):
        offsets.extend(range(last, run.start() + 1))
        last = run.end()
    offsets.extend(range(last, len(text)))
    return offsets


def collapse(text):
    """`text` with every whitespace run (newlines included) collapsed to one space."""
    return re.sub(r'\s+', ' ', text)


def paragraphs(text):
    """Blank-line separated blocks as (first line number, block text)."""
    blocks, start, lines = [], 1, []
    for number, line in enumerate(text.splitlines(), 1):
        if line.strip():
            if not lines:
                start = number
            lines.append(line)
        elif lines:
            blocks.append((start, '\n'.join(lines)))
            lines = []
    if lines:
        blocks.append((start, '\n'.join(lines)))
    return blocks


def sections(text):
    """Map each `##`/`###` heading line (as written) to its body text, up to the
    next heading of the same or a higher level."""
    lines = text.splitlines()
    result = {}
    for index, line in enumerate(lines):
        match = re.match(r'(#{2,3}) ', line)
        if not match:
            continue
        level = len(match.group(1))
        body = []
        for later in lines[index + 1:]:
            heading = re.match(r'(#{1,6}) ', later)
            if heading and len(heading.group(1)) <= level:
                break
            body.append(later)
        result[line] = '\n'.join(body)
    return result


def heading_slug(heading):
    """GitHub's anchor for a heading line's text."""
    text = re.sub(r'\[([^\]]*)\]\([^)]*\)', r'\1', heading.lstrip('#').strip())
    text = text.replace('`', '').replace('*', '').lower()
    text = re.sub(r'[^\w\- ]', '', text)
    return text.replace(' ', '-')


@functools.lru_cache(maxsize=None)
def anchors(text):
    """The heading anchors (with GitHub's `-N` suffix for repeats) and explicit
    `id`/`name` anchors a Markdown file defines."""
    found, seen, fenced = set(), {}, False
    for line in text.splitlines():
        if line.lstrip().startswith(('```', '~~~')):
            fenced = not fenced
        elif not fenced and re.match(r'#{1,6} ', line):
            slug = heading_slug(line)
            count = seen.get(slug, 0)
            seen[slug] = count + 1
            found.add(slug if count == 0 else f'{slug}-{count}')
    found.update(re.findall(r'<a\s+(?:id|name)="([^"]+)"', text))
    return frozenset(found)


def blank_code(text):
    """`text` with fenced blocks and inline code spans blanked (newlines kept)."""
    def blank(match):
        return re.sub(r'[^\n]', ' ', match.group(0))
    text = re.sub(r'^(```|~~~).*?^\1[^\n]*$', blank, text, flags=re.MULTILINE | re.DOTALL)
    return re.sub(r'(`+)[^`\n]*?\1', blank, text)


def report_commits(report):
    """The source commits a qualification report records: `source_commit`,
    `source.commit`, or the `source_commit` of each entry of an index's
    `reports`. Empty when the report records none."""
    if not isinstance(report, dict):
        return set()
    commits = set()
    if isinstance(report.get('source_commit'), str):
        commits.add(report['source_commit'])
    source = report.get('source')
    if isinstance(source, dict) and isinstance(source.get('commit'), str):
        commits.add(source['commit'])
    for entry in report.get('reports', []) if isinstance(report.get('reports'), list) else []:
        if isinstance(entry, dict) and isinstance(entry.get('source_commit'), str):
            commits.add(entry['source_commit'])
    return commits


def commit_named(text, commit):
    """Whether `text` names `commit` in full or by a prefix of at least 7 characters."""
    return any(commit.startswith(token) for token in re.findall(r'\b[0-9a-f]{7,40}\b', text))


class Docs:
    """The documents of one tree, read lazily."""

    def __init__(self, root, names):
        self.root = Path(root)
        self.names = sorted(names)
        self._text = {}
        self._collapsed = {}
        self._offsets = {}
        self._anchors = {}

    def anchors_of(self, path):
        """The anchors of the Markdown file at `path`, read once."""
        if path not in self._anchors:
            self._anchors[path] = anchors(path.read_text(encoding='utf-8'))
        return self._anchors[path]

    def text(self, name):
        if name not in self._text:
            self._text[name] = (self.root / name).read_text(encoding='utf-8')
        return self._text[name]

    def collapsed(self, name):
        if name not in self._collapsed:
            self._collapsed[name] = collapse(self.text(name))
        return self._collapsed[name]

    def line(self, name, collapsed_index):
        """The line of `name` on which the collapsed text's character begins."""
        if name not in self._offsets:
            self._offsets[name] = collapse_offsets(self.text(name))
        return self.text(name).count('\n', 0, self._offsets[name][collapsed_index]) + 1


class Report:
    """Accumulates errors under a rule name."""

    def __init__(self):
        self.errors = []

    def add(self, rule, where, message):
        self.errors.append(f'[{rule}] {where}: {message}')

    def fact(self, docs, rule, name, pattern, flags=0):
        """All matches of `pattern` in the collapsed text of `name`; none is an error."""
        text = docs.collapsed(name)
        matches = list(re.finditer(pattern, text, flags))
        if not matches:
            self.add(rule, name, f'fact sentence not found: /{pattern}/')
        return matches

    def count_equals(self, docs, rule, name, match, group, expected, what):
        word = match.group(group)
        value = number_value(word)
        if value != expected:
            self.add(rule, f'{name}:{docs.line(name, match.start(group))}',
                     f'states {word!r} {what}, but the source has {expected}')


def backticked(text):
    return re.findall(r'`([^`]+)`', text)


def check_release_set(docs, report, release_order):
    rule = 'release-set'
    expected = list(release_order)
    for match in report.fact(docs, rule, 'docs/RELEASE.md',
                             r'release set is (\w+) crates, in this order: (.+?`)\. '):
        report.count_equals(docs, rule, 'docs/RELEASE.md', match, 1, len(expected), 'crates')
        listed = backticked(match.group(2))
        if listed != expected:
            report.add(rule, f'docs/RELEASE.md:{docs.line("docs/RELEASE.md", match.start(2))}',
                       f'lists {listed} but RELEASE_ORDER is {expected}')
    for pattern in (r'\b(\w+) Cargo-verified archives', r'\ball (\w+) downloadable archives'):
        for match in report.fact(docs, rule, 'docs/QUALIFICATION.md', pattern):
            report.count_equals(docs, rule, 'docs/QUALIFICATION.md', match, 1, len(expected), 'archives')


def check_workspace_version(docs, report, version):
    rule = 'workspace-version'
    singles = (('docs/RELEASE.md', r'PurRAssist (\d+\.\d+\.\d+) has an implemented scope'),
               ('docs/IMPLEMENTATION.md', r'Workspace version (\d+\.\d+\.\d+);'),
               ('docs/QUALIFICATION.md', r'implemented (\d+\.\d+\.\d+) capability'))
    for name, pattern in singles:
        for match in report.fact(docs, rule, name, pattern):
            if match.group(1) != version:
                report.add(rule, f'{name}:{docs.line(name, match.start(1))}',
                           f'states version {match.group(1)} but the workspace version is {version}')
    status = rf'({"|".join(STATUS_FORMS)}) `rust-v(\d+\.\d+\.\d+)`'
    forms = {}
    for name in STATUS_DOCS:
        for match in report.fact(docs, rule, name, status, re.IGNORECASE):
            forms.setdefault(name, set()).add(match.group(1).lower())
            if match.group(2) != version:
                report.add(rule, f'{name}:{docs.line(name, match.start(2))}',
                           f'states version {match.group(2)} but the workspace version is {version}')
    if len(set().union(*forms.values())) > 1:
        report.add(rule, ', '.join(STATUS_DOCS),
                   'the status lines mix forms, but all must read the same one: '
                   + '; '.join(f'{name} says {sorted(found)}' for name, found in sorted(forms.items())))


def check_fuzz_targets(docs, report, targets, makefile):
    rule = 'fuzz-targets'
    expected = list(targets)
    for match in report.fact(docs, rule, 'docs/FUZZING.md',
                             r'across (\w+) `purrassist-core` targets: (.+?`)\. '):
        report.count_equals(docs, rule, 'docs/FUZZING.md', match, 1, len(expected), 'targets')
        if backticked(match.group(2)) != expected:
            report.add(rule, 'docs/FUZZING.md',
                       f'lists {backticked(match.group(2))} but TARGETS is {expected}')
    counts = (('docs/FUZZING.md', r'\ball (\w+) targets'),
              ('docs/QUALIFICATION.md', r'\b(\w+) targets \(including'),
              ('docs/QUALIFICATION.md', r'\bthe (\w+) mutation targets'),
              ('README.md', r'\b(\w+) coverage-guided targets'))
    for name, pattern in counts:
        for match in report.fact(docs, rule, name, pattern, re.IGNORECASE):
            report.count_equals(docs, rule, name, match, 1, len(expected), 'targets')
    flat = re.sub(r'\s+', ' ', makefile)
    loops = re.findall(r'for target in ([a-z -]+?); do', flat)
    if not loops:
        report.add(rule, 'Makefile', 'fact sentence not found: the `for target in ...; do` loop')
    for loop in loops:
        if loop.split() != expected:
            report.add(rule, 'Makefile', f'fuzz loop runs {loop.split()} but TARGETS is {expected}')
    comment = re.search(r'TARGET selects one of ([a-z/-]+)', flat)
    if comment is None:
        report.add(rule, 'Makefile', 'fact sentence not found: /TARGET selects one of .../')
    elif comment.group(1).split('/') != expected:
        report.add(rule, 'Makefile', f'comment lists {comment.group(1).split("/")} but TARGETS is {expected}')
    ten = re.search(rf'across all ({NUMBER}) targets', flat, re.IGNORECASE)
    if ten is None:
        report.add(rule, 'Makefile', 'fact sentence not found: /across all N targets/')
    elif number_value(ten.group(1)) != len(expected):
        report.add(rule, 'Makefile', f'states {ten.group(1)!r} targets but TARGETS has {len(expected)}')


def check_receipt_version(docs, report, source):
    rule = 'receipt-version'
    found = re.search(r'pub const RECEIPT_VERSION: u32 = (\d+);', source)
    if found is None:
        report.add(rule, 'crates/core/src/contract.rs', 'fact sentence not found: RECEIPT_VERSION')
        return
    for match in report.fact(docs, rule, 'docs/QUALIFICATION.md', r'receipt wire v(\d+)'):
        if match.group(1) != found.group(1):
            report.add(rule, f'docs/QUALIFICATION.md:{docs.line("docs/QUALIFICATION.md", match.start(1))}',
                       f'states receipt wire v{match.group(1)} but RECEIPT_VERSION is {found.group(1)}')


@functools.lru_cache(maxsize=None)
def dialect_lists(text):
    """Every dialect list in `text` (collapsed): the (start, ids) of each run of
    at least DIALECT_RUN_MINIMUM ids."""
    lists = []
    for run in DIALECT_RUN.finditer(text):
        ids = re.findall(rf"`({DIALECT_ID})`", run.group(0))
        if len(ids) >= DIALECT_RUN_MINIMUM:
            lists.append((run.start(), ids))
    return tuple(lists)


@functools.lru_cache(maxsize=None)
def dialect_counts(text):
    """The (start, group, word) of each claim, in collapsed `text`, of how many dialects exist."""
    return tuple((match.start(group), group, match.group(group)) for match in DIALECT_COUNT.finditer(text)
                 for group in [next(index for index in range(1, 5) if match.group(index))])


def dialect_tables(raw):
    """Yield (line number, ids) for each contiguous `|` block whose first-cell
    ids number at least DIALECT_RUN_MINIMUM."""
    block, first = [], 0
    for number, line in enumerate(raw.splitlines() + [''], 1):
        if line.startswith('|'):
            if not block:
                first = number
            block.append(line)
            continue
        ids = [m.group(1) for row in block if (m := DIALECT_ROW.match(row))]
        if len(ids) >= DIALECT_RUN_MINIMUM:
            yield first, ids
        block = []


def check_dialects(docs, report, stems):
    expected = sorted(stems)
    rule = 'dialect-ids'
    for name in docs.names:
        text = docs.collapsed(name)
        found = 0
        for start, ids in dialect_lists(text):
            found += 1
            if sorted(ids) != expected or len(set(ids)) != len(ids):
                report.add(rule, f'{name}:{docs.line(name, start)}',
                           f'list of dialects differs from {DIALECT_PAGES}/*.md: '
                           f'missing {sorted(set(expected) - set(ids))}, extra {sorted(set(ids) - set(expected))}')
        for line, ids in dialect_tables(docs.text(name)):
            found += 1
            if sorted(set(ids)) != expected:
                report.add(rule, f'{name}:{line}',
                           f'table of dialects differs from {DIALECT_PAGES}/*.md: '
                           f'missing {sorted(set(expected) - set(ids))}, extra {sorted(set(ids) - set(expected))}')
        if name in DIALECT_LIST_DOCS and not found:
            report.add(rule, name, 'fact sentence not found: no list or table of the dialect ids')
    rule = 'dialect-count'
    for name in docs.names:
        claims = dialect_counts(docs.collapsed(name))
        for start, _, word in claims:
            if number_value(word) != len(expected):
                report.add(rule, f'{name}:{docs.line(name, start)}',
                           f'states {word!r} dialects, but the source has {len(expected)}')
        if name in DIALECT_COUNT_DOCS and not claims:
            report.add(rule, name, 'fact sentence not found: no dialect count')


@functools.lru_cache(maxsize=None)
def link_targets(raw):
    """The (line, destination) of every link, reference definition, and `src`/`href`
    attribute outside code."""
    text = blank_code(raw)
    found = [(m.start(1), m.group(1)) for m in re.finditer(r'\]\(\s*(<[^>]*>|[^)\s]+)', text)]
    found += [(m.start(1), m.group(1)) for m in re.finditer(r'^\s*\[[^\]]+\]:\s*(\S+)', text, re.MULTILINE)]
    found += [(m.start(1), m.group(1)) for m in re.finditer(r'\b(?:src|href)="([^"]+)"', text)]
    return tuple((text.count('\n', 0, start) + 1, target) for start, target in found)


def check_links(docs, report):
    rule = 'links'
    for name in docs.names:
        for line, target in link_targets(docs.text(name)):
            target = target.strip('<>')
            if re.match(r'[A-Za-z][A-Za-z0-9+.-]*:|//', target):
                continue
            path, _, fragment = target.partition('#')
            path = urllib.parse.unquote(path)
            where = f'{name}:{line}'
            if path:
                base = docs.root if path.startswith('/') else (docs.root / name).parent
                resolved = (base / path.lstrip('/')).resolve()
                if not resolved.exists():
                    report.add(rule, where, f'relative link {target!r} does not resolve')
                    continue
            else:
                resolved = docs.root / name
            if fragment and resolved.suffix == '.md' and resolved.is_file():
                if urllib.parse.unquote(fragment) not in docs.anchors_of(resolved):
                    report.add(rule, where, f'link {target!r} names a heading that does not exist')


def check_stage_references(docs, report):
    for name in docs.names:
        for match in re.finditer(r'(?<![\w.])\.stage(?!\w)', docs.text(name)):
            line = docs.text(name).count('\n', 0, match.start()) + 1
            report.add('stage-references', f'{name}:{line}', 'public documents must not refer to `.stage`')


@functools.lru_cache(maxsize=None)
def banned_matches(collapsed):
    """The (start, matched text, phrase) of every banned phrase in collapsed text."""
    return tuple((match.start(), match.group(0), phrase) for phrase in BANNED_PHRASES
                 for match in re.finditer(phrase, collapsed, re.IGNORECASE))


_source_cache = {}


def source_jargon(path):
    """The (line, matched text, pattern) of every numbered reference in the Rust file `path`.

    A run of consecutive comment lines is collapsed to one text with the markers stripped, so
    a reference wrapped across two comment lines is found and reported at its first line.
    The result is cached by the file's identity (device and inode, which a link shares with its target), mtime
    and size, so the many trees that link the same sources are read once per process."""
    stat = path.stat()
    key = (stat.st_dev, stat.st_ino, stat.st_mtime_ns, stat.st_size)
    if key not in _source_cache:
        units = []  # (first line, text) of each comment run or code line
        for number, line in enumerate(path.read_text(encoding='utf-8').splitlines(), 1):
            comment = COMMENT_LINE.match(line)
            if comment and units and units[-1][2]:
                units[-1][1].append(comment.group(1).strip())
            else:
                units.append([number, [comment.group(1).strip() if comment else line], bool(comment)])
        found = []
        for number, parts, _ in units:
            text = ' '.join(parts)
            found.extend((number, match.group(0), pattern) for pattern in SOURCE_JARGON
                         for match in re.finditer(pattern, text, re.IGNORECASE))
        _source_cache[key] = tuple(found)
    return _source_cache[key]


def check_source_jargon(root, report, packages):
    """No numbered work-item or requirement reference in the `src/` of a publishable crate."""
    for package in publishable(packages):
        source = Path(root) / package['directory'] / 'src'
        for path in sorted(source.rglob('*.rs')):
            for number, matched, pattern in source_jargon(path):
                report.add('source-jargon', f"{path.relative_to(root).as_posix()}:{number}",
                           f'process wording {matched!r} in a published crate source (/{pattern}/); '
                           'name the feature or state the requirement instead')


def check_banned_phrases(docs, report):
    for name in docs.names:
        for start, matched, phrase in banned_matches(docs.collapsed(name)):
            report.add('banned-phrase', f'{name}:{docs.line(name, start)}',
                       f'stale or process-bound wording {matched!r} (/{phrase}/)')


def load_limitations(docs, report, targets):
    """The validated issue records of `docs/known-limitations.json`, or None. `targets` are
    the fuzz targets an entry's optional `fuzz_stop_targets` may name."""
    rule = 'known-limitations-source'
    try:
        data = json.loads(docs.text(KNOWN_LIMITATIONS_JSON))
        entries = data['limitations']
        if not isinstance(data['observed'], str) or not data['observed'] or not isinstance(entries, list):
            raise TypeError('observed must be a date string and limitations a list')
    except (ValueError, KeyError, TypeError) as error:
        report.add(rule, KNOWN_LIMITATIONS_JSON, f'unreadable ({type(error).__name__}: {error})')
        return None
    problems = len(report.errors)
    seen = set()
    for entry in entries:
        label = f'{KNOWN_LIMITATIONS_JSON} entry {entry.get("issue") if isinstance(entry, dict) else entry!r}'
        if not isinstance(entry, dict) or set(entry) not in (LIMITATION_KEYS, LIMITATION_KEYS | {STOP_TARGETS_KEY}):
            report.add(rule, label, f'needs exactly issue, title, impact and blocks_release, and optionally {STOP_TARGETS_KEY}')
            continue
        if not isinstance(entry['issue'], int) or isinstance(entry['issue'], bool) or entry['issue'] <= 0:
            report.add(rule, label, 'issue must be a positive integer')
        if entry['issue'] in seen:
            report.add(rule, label, 'duplicate issue')
        seen.add(entry['issue'])
        for key in ('title', 'impact'):
            if not isinstance(entry[key], str) or not entry[key].strip() or '\n' in entry[key]:
                report.add(rule, label, f'{key} must be one non-empty line')
        if entry['blocks_release'] is not False:
            report.add(rule, label, 'blocks_release must be false: a blocking issue is not a known limitation of 0.1.0')
        stops = entry.get(STOP_TARGETS_KEY, [])
        if not isinstance(stops, list) or not all(isinstance(stop, str) for stop in stops):
            report.add(rule, label, f'{STOP_TARGETS_KEY} must be a list of fuzz target names')
        else:
            for stop in stops:
                if stop not in targets:
                    report.add(rule, label, f'{STOP_TARGETS_KEY} names {stop!r}, which is not a target of '
                                            f'scripts/fuzz.py TARGETS {list(targets)}')
            if len(set(stops)) != len(stops):
                report.add(rule, label, f'{STOP_TARGETS_KEY} lists a target twice')
    if not entries:
        report.add(rule, KNOWN_LIMITATIONS_JSON, 'lists no limitations')
    return data if len(report.errors) == problems else None


def changelog_section(text, version):
    """The (line number of the first body line, body lines) of the `## [version]` section of
    a changelog, found by heading position: from its heading to the next `## ` heading. None
    when there is no such heading."""
    lines = text.splitlines()
    start = next((index for index, line in enumerate(lines) if re.match(rf'## \[{re.escape(version)}\]', line)), None)
    if start is None:
        return None
    end = next((index for index in range(start + 1, len(lines)) if lines[index].startswith('## ')), len(lines))
    return start + 2, lines[start + 1:end]


def subsection(lines, heading):
    """The text under the one `heading` line (a `##` or `###` line) of `lines`, up to the next
    heading of the same or a higher level; None unless the heading occurs exactly once."""
    starts = [index for index, line in enumerate(lines) if line == heading]
    if len(starts) != 1:
        return None
    level = len(heading) - len(heading.lstrip('#'))
    body = []
    for line in lines[starts[0] + 1:]:
        later = re.match(r'(#{1,6}) ', line)
        if later and len(later.group(1)) <= level:
            break
        body.append(line)
    return '\n'.join(body)


def check_changelog(docs, report, version, release_order):
    rule = 'changelog'
    text = docs.text(CHANGELOG)
    versions = re.findall(r'^## \[(\d+\.\d+\.\d+)\]', text, re.MULTILINE)
    if not versions:
        report.add(rule, CHANGELOG, 'fact sentence not found: no "## [X.Y.Z]" version heading')
        return
    if versions[0] != version:
        report.add(rule, CHANGELOG, f'the newest version heading is [{versions[0]}] but the workspace version is {version}')
    found = changelog_section(text, version)
    if found is None:
        report.add(rule, CHANGELOG, f'has no "## [{version}]" section')
        return
    first, lines = found
    body = '\n'.join(lines)
    flat, offsets = collapse(body), collapse_offsets(body)
    for phrase in RELEASE_BODY_BANNED:
        for match in re.finditer(phrase, flat, re.IGNORECASE):
            report.add(rule, f'{CHANGELOG}:{first + body.count(chr(10), 0, offsets[match.start()])}',
                       f'the [{version}] section is the release body, which must not contain {match.group(0)!r} '
                       f'(the heading carries the status)')
    for crate in release_order:
        if f'`{crate}`' not in body:
            report.add(rule, CHANGELOG, f'the [{version}] section does not name the crate `{crate}`')
    for label in CHANGELOG_INTERFACES:
        if label not in body:
            report.add(rule, CHANGELOG, f'the [{version}] section does not highlight the interface {label}')


def check_security_policy(docs, report, version, repository):
    rule = 'security-policy'
    major_minor = '.'.join(version.split('.')[:2])
    text = docs.text(SECURITY)
    rows = [line for line in text.splitlines() if re.match(rf'\|\s*{re.escape(major_minor)}\.', line)]
    if not rows:
        report.add(rule, SECURITY, f'fact sentence not found: no supported-versions table row for {major_minor}.x')
    for channel in (SECURITY_EMAIL, f'{repository}/security/advisories/new'):
        if channel not in text:
            report.add(rule, SECURITY, f'does not name the reporting channel {channel}')


def check_release_reservations(docs, report, release_order):
    rule = 'release-reservations'
    text = docs.text(RELEASE_DOC)
    if text.splitlines().count(RESERVATIONS_HEADING) != 1:
        report.add(rule, RELEASE_DOC, f'fact sentence not found: needs exactly one {RESERVATIONS_HEADING!r} heading')
        return
    rows = [re.match(r'\|\s*`([^`]+)`', line) for line in sections(text)[RESERVATIONS_HEADING].splitlines()]
    listed = [row.group(1) for row in rows if row]
    if not listed:
        report.add(rule, RELEASE_DOC, 'fact sentence not found: the reservation table has no crate rows')
    elif listed != list(release_order):
        report.add(rule, RELEASE_DOC, f'the reservation table lists {listed} but RELEASE_ORDER is {list(release_order)}')


def workflow_steps(workflow):
    """The steps of the jobs of a workflow file as (line number, name or None, has `run:`), read
    as text (the standard library has no YAML parser). A step is an item of a job's block
    `steps:` list, found by indentation: the lines indented deeper than `steps:` form the
    block, the items are the `- ` lines at the indentation of the first one, and a step's keys
    are those at the column after its `- `. A `run: |` body is indented deeper still, so a line
    of it that starts with `- ` is no item; a `name:` may precede or follow the `run:`; and
    items outside `steps:` (a matrix `include`) are not steps."""
    lines = workflow.splitlines()
    steps, in_jobs, index = [], False, 0
    while index < len(lines):
        opener = re.match(r'(\s*)steps:\s*(?:#.*)?$', lines[index]) if in_jobs else None
        if re.match(r'jobs:\s*$', lines[index]):
            in_jobs = True
        index += 1
        if opener is None:
            continue
        block = []
        while index < len(lines):
            line = lines[index]
            stripped = line.lstrip()
            if stripped and not stripped.startswith('#') and len(line) - len(stripped) <= len(opener.group(1)):
                break
            block.append((index + 1, line))
            index += 1
        steps.extend(block_steps(block))
    return steps


def block_steps(block):
    """The (line number, name or None, has `run:`) of each item of a `steps:` block's lines."""
    steps, item, item_column, key_column = [], None, 0, 0
    for number, line in block:
        opening = re.match(r'(\s*)-\s+', line)
        if opening and (item is None or len(opening.group(1)) == item_column):
            if item is None:
                item_column = len(opening.group(1))
            key_column = len(opening.group(0))
            item = [number, None, False]
            steps.append(item)
            line = ' ' * key_column + line[key_column:]
        if item is None:
            continue
        key = re.match(rf'\s{{{key_column}}}([A-Za-z_][\w-]*):\s*(.*?)\s*$', line)
        if key and key.group(1) == 'name':
            item[1] = key.group(2).strip('\'"') or None
        elif key and key.group(1) == 'run':
            item[2] = True
    return [tuple(step) for step in steps]


def workflow_step_names(workflow):
    """The `name:` of every named step of the jobs of a workflow file."""
    return [name for _, name, _ in workflow_steps(workflow) if name is not None]


def check_release_qualification(docs, report):
    rule = 'release-qualification-steps'
    workflow = docs.text(QUALIFICATION_WORKFLOW)
    for number, name, runs in workflow_steps(workflow):
        if runs and name is None:
            report.add(rule, f'{QUALIFICATION_WORKFLOW}:{number}', 'a step that runs a command needs a `name:`, '
                                                                   'which the RELEASE.md summary then names')
    steps = workflow_step_names(workflow)
    if not steps:
        report.add(rule, QUALIFICATION_WORKFLOW, 'no named step found in the jobs')
        return
    text = docs.text(RELEASE_DOC)
    if text.splitlines().count(QUALIFICATION_HEADING) != 1:
        report.add(rule, RELEASE_DOC, f'fact sentence not found: needs exactly one {QUALIFICATION_HEADING!r} heading')
        return
    section = collapse(sections(text)[QUALIFICATION_HEADING])
    for step in steps:
        if f'`{step}`' not in section:
            report.add(rule, RELEASE_DOC, f'the qualification summary does not name the workflow step `{step}`')


def without_section(text, heading):
    """`text` with the `##` section headed `heading` blanked (newlines kept, so offsets and line
    numbers survive): a dated historical record is not a statement about the current state."""
    blanked, inside = [], False
    for line in text.splitlines(keepends=True):
        if line.startswith('## '):
            inside = line.rstrip() == heading
        blanked.append(re.sub(r'[^\n]', ' ', line) if inside else line)
    return ''.join(blanked)


def check_release_toolchain(docs, report, pins):
    """Every `RUSTUP_TOOLCHAIN=<x>` and patch-form `Rust <x.y.z>` in `TOOLCHAIN_DOCS` equals the
    stable pin; `pins` are the stable pins of the release-qualification workflow."""
    rule = 'release-toolchain'
    if len(pins) != 1:
        report.add(rule, QUALIFICATION_WORKFLOW, f'expected exactly one stable toolchain pin, found {pins}')
        return
    found = 0
    for name in TOOLCHAIN_DOCS:
        text = docs.text(name)
        if name == RELEASE_DOC:
            text = without_section(text, PUBLICATION_RECORD_HEADING)
        for match in TOOLCHAIN_MENTION.finditer(text):
            found += 1
            value = match[1] or match[2]
            if value != pins[0]:
                report.add(rule, f'{name}:{text.count(chr(10), 0, match.start()) + 1}',
                           f'names toolchain {value} but {QUALIFICATION_WORKFLOW} pins {pins[0]}')
    if not found:
        report.add(rule, ', '.join(TOOLCHAIN_DOCS), 'fact sentence not found: no RUSTUP_TOOLCHAIN=<x> or Rust x.y.z')


def check_readme_changelog(docs, report):
    if not re.search(r'\]\(CHANGELOG\.md(?:#[^)]*)?\)', docs.text('README.md')):
        report.add('readme-changelog', 'README.md', 'fact sentence not found: no link to CHANGELOG.md')


def stop_issues(text, prefix=''):
    """The issues a collapsed `text` says the default `make fuzz` stops on, or None when it says
    nothing about a stop. Two forms are read:

    * "[prefix]stops on the #A signatures", "... the #A and #B signatures" or "... the #A, #B,
      and #C signatures" (an Oxford comma is accepted): the issues named;
    * "[prefix]stops on no known signature": the empty set, which is the sentence a document
      must carry when no entry of `docs/known-limitations.json` has a non-empty
      `fuzz_stop_targets`. A document then must not name a stop issue, and one that names stops
      must not claim there is none.
    """
    stop = re.search(re.escape(prefix) + r'stops on the ((?:#\d+, )*#\d+(?:,? and #\d+)?) signatures', text)
    if stop is not None:
        return {int(number) for number in re.findall(r'#(\d+)', stop.group(1))}
    return set() if re.search(re.escape(prefix + NO_STOP_SENTENCE), text) else None


def states_both_stop_forms(text, prefix=''):
    """Whether a collapsed `text` both names stop issues and says there is no stop: a
    contradiction `stop_issues` cannot report, because it returns the first form it reads."""
    return bool(re.search(re.escape(prefix) + r'stops on the (?:#\d+, )*#\d+(?:,? and #\d+)? signatures', text)
                and re.search(re.escape(prefix + NO_STOP_SENTENCE), text))


def check_known_limitations(docs, report, data, targets, version):
    rule = 'known-limitations-section'
    entries = data['limitations']
    expected = {entry['issue'] for entry in entries}
    stops = {entry['issue'] for entry in entries if entry.get(STOP_TARGETS_KEY)}
    holders = ['docs/QUALIFICATION.md', 'docs/FUZZING.md', CHANGELOG]
    for name in holders:
        text = docs.text(name)
        if name == CHANGELOG:
            found = changelog_section(text, version)
            body = None if found is None else subsection(found[1], CHANGELOG_LIMITATIONS_HEADING)
            if body is None:
                report.add(rule, name, f'needs exactly one {CHANGELOG_LIMITATIONS_HEADING!r} heading inside the '
                                       f'[{version}] section')
                continue
        elif text.splitlines().count(KNOWN_LIMITATIONS_HEADING) != 1:
            report.add(rule, name, f'needs exactly one {KNOWN_LIMITATIONS_HEADING!r} heading')
            continue
        else:
            body = sections(text)[KNOWN_LIMITATIONS_HEADING]
        section = collapse(body)
        remainder = section
        for entry in entries:
            for key in ('title', 'impact'):
                if entry[key] not in section:
                    report.add(rule, name, f'issue #{entry["issue"]} {key} is not stated verbatim '
                                           f'from {KNOWN_LIMITATIONS_JSON}')
                remainder = remainder.replace(entry[key], ' ')
        referenced = {int(number) for number in re.findall(r'(?<![\w&/])#(\d+)\b', remainder)}
        if referenced != expected:
            report.add(rule, name, f'issue set {sorted(referenced)} differs from {KNOWN_LIMITATIONS_JSON} '
                                   f'{sorted(expected)}')
        if name != CHANGELOG:
            stated = stop_issues(section, 'default `make fuzz` ')
            if states_both_stop_forms(section, 'default `make fuzz` '):
                report.add(rule, name, f'the default `make fuzz` stop both names issues and says "{NO_STOP_SENTENCE}"')
            if stated is None:
                report.add(rule, name, f'fact sentence not found: the default `make fuzz` stop on the #N and #N signatures, '
                                       f'or "{NO_STOP_SENTENCE}" when {KNOWN_LIMITATIONS_JSON} names no stop')
            else:
                report_stop_set(report, rule, name, stated, stops, expected)
    check_fuzz_rows(docs, report, expected, stops)
    check_baseline_table(docs, report, targets)


def report_stop_set(report, rule, where, stated, stops, expected):
    """Compare the issues a document says the default `make fuzz` stops on with the issues whose
    `fuzz_stop_targets` name a stop in `docs/known-limitations.json`."""
    if stated - expected:
        report.add(rule, where, f'the `make fuzz` stop cites {sorted(stated - expected)}, which '
                                f'is not a known limitation')
        stated = stated & expected
    if stated != stops:
        report.add(rule, where, f'the `make fuzz` stop names issues {sorted(stated)} but {KNOWN_LIMITATIONS_JSON} '
                                f'gives {STOP_TARGETS_KEY} to {sorted(stops)}: missing {sorted(stops - stated)}, '
                                f'extra {sorted(stated - stops)}')


def check_fuzz_rows(docs, report, expected, stops):
    rule = 'fuzz-row-limits'
    name = 'docs/QUALIFICATION.md'
    rows = [(number, line) for number, line in enumerate(docs.text(name).splitlines(), 1)
            if line.startswith('|') and '`make fuzz`' in line]
    if not rows:
        report.add(rule, name, 'fact sentence not found: no table row for `make fuzz`')
    for number, line in rows:
        if '](#known-limitations)' not in line:
            report.add(rule, f'{name}:{number}', 'a `make fuzz` row must link to #known-limitations')
        stated = stop_issues(line)
        if states_both_stop_forms(line):
            report.add(rule, f'{name}:{number}', f'a `make fuzz` row both names stop issues and says "{NO_STOP_SENTENCE}"')
        if stated is None:
            report.add(rule, f'{name}:{number}',
                       'a `make fuzz` row must state that the default run stops on the #N and #N signatures, '
                       f'or "{NO_STOP_SENTENCE}" when {KNOWN_LIMITATIONS_JSON} names no stop')
        else:
            report_stop_set(report, rule, f'{name}:{number}', stated, stops, expected)


def check_baseline_table(docs, report, targets):
    rule = 'fuzz-baseline-table'
    name = 'docs/FUZZING.md'
    body = sections(docs.text(name)).get(BASELINE_HEADING)
    if body is None:
        report.add(rule, name, f'fact sentence not found: heading {BASELINE_HEADING!r}')
        return
    rows = []
    for line in body.splitlines():
        if line.startswith('|'):
            rows.append([cell.strip() for cell in line.strip().strip('|').split('|')])
        elif rows:
            break
    rows = [row for row in rows[2:]]
    names = [re.sub(r'^\[([^\]]+)\]\(.*\)$', r'\1', row[0]) for row in rows]
    if names != list(targets):
        report.add(rule, name, f'baseline rows {names} differ from TARGETS {list(targets)}')
        return
    for target, row in zip(targets, rows):
        archive = f'qualification/fuzz-{target}-linux-x86_64.json'
        if (docs.root / 'docs' / archive).is_file():
            if row[0] != f'[{target}]({archive})':
                report.add(rule, name, f'{target} has a baseline report, so its row must link {archive}')
            if 'none' in row[1:]:
                report.add(rule, name, f'{target} has a baseline report but its row says none')
        elif any(cell != 'none' for cell in row[1:]):
            report.add(rule, name, f'{target} has no baseline report ({archive}), so every cell must say none')


def check_archived_evidence(docs, report):
    rule = 'archived-evidence'
    name = 'docs/QUALIFICATION.md'
    found = 0
    for line, block in paragraphs(docs.text(name)):
        reports = re.findall(r'\]\((qualification/[^)#\s]+\.json)\)', block)
        if not reports:
            continue
        found += 1
        flat = collapse(block)
        for path in reports:
            try:
                commits = report_commits(json.loads((docs.root / 'docs' / path).read_text(encoding='utf-8')))
            except (OSError, ValueError) as error:
                report.add(rule, f'{name}:{line}', f'{path} is unreadable ({type(error).__name__})')
                continue
            if not commits:
                if 'records no source commit' not in flat:
                    report.add(rule, f'{name}:{line}',
                               f'{path} records no source commit, and the paragraph must say it records no source commit')
            for commit in sorted(commits):
                if not commit_named(flat, commit):
                    report.add(rule, f'{name}:{line}', f'the paragraph does not name commit {commit[:12]}, '
                                                       f'the one {path} is bound to')
    if not found:
        report.add(rule, name, 'fact sentence not found: no paragraph links an archived qualification report')


def check_stage2_heading(docs, report):
    rule = 'stage2-heading'
    name = 'docs/QUALIFICATION.md'
    text = docs.text(name)
    headings = [line for line in text.splitlines() if line.startswith('#') and 'Stage 2' in line]
    if not headings:
        report.add(rule, name, 'fact sentence not found: no Stage 2 heading')
    for heading in headings:
        match = re.fullmatch(r'## Historical: Stage 2 record bound to `([0-9a-f]{7,40})`', heading)
        if match is None:
            report.add(rule, name, f'{heading!r} must read "## Historical: Stage 2 record bound to `<commit>`"')
            continue
        for path in re.findall(r'\]\((qualification/[^)#\s]+\.json)\)', sections(text)[heading]):
            for commit in report_commits(json.loads((docs.root / 'docs' / path).read_text(encoding='utf-8'))):
                if not commit.startswith(match.group(1)):
                    report.add(rule, name, f'heading commit {match.group(1)} is not the commit {path} is bound to')


def check_windows_run(docs, report):
    rule = 'windows-run'
    name = 'docs/QUALIFICATION.md'
    found = False
    for line, block in paragraphs(docs.text(name)):
        flat = collapse(block)
        for match in re.finditer(r'windows-engines', flat):
            found = True
            window = flat[max(0, match.start() - 400):match.end() + 400]
            url = RUN_URL.search(window)
            label = re.search(r'Verify run (\d+)', window)
            if url is None or label is None or url.group(1) != label.group(1):
                report.add(rule, f'{name}:{line}', 'the Windows claim must cite its run as "Verify run N" with the '
                                                   'matching actions/runs/N link')
            if COMMIT.search(window) is None:
                report.add(rule, f'{name}:{line}', 'the Windows claim must name the full commit the run executed on')
    if not found:
        report.add(rule, name, 'fact sentence not found: no sentence about the windows-engines lane')



def strip_hidden(text):
    """`text` with HTML comments and fenced code blocks blanked (newlines kept), so that
    what remains is what a reader of the rendered page sees."""
    def blank(match):
        return re.sub(r'[^\n]', ' ', match.group(0))
    text = re.sub(r'<!--.*?-->', blank, text, flags=re.DOTALL)
    return re.sub(r'^(```|~~~).*?^\1[^\n]*$', blank, text, flags=re.MULTILINE | re.DOTALL)


def marked_block(text, begin, end):
    """The text between the `begin` and `end` marker lines, or None unless each occurs once, in order."""
    if text.count(begin) != 1 or text.count(end) != 1:
        return None
    start = text.index(begin) + len(begin)
    stop = text.index(end)
    return text[start:stop] if start <= stop else None


_metadata_cache = {}


def workspace_packages(root):
    """The workspace packages of the tree at `root`, from `cargo metadata`: for each, its
    name, `publish` list (None when publishable), `documentation`, and the repository-relative
    paths of its directory and README. Results are cached by the manifests' content."""
    root = Path(root)
    manifest = tomllib.loads((root / 'Cargo.toml').read_text(encoding='utf-8'))
    members = manifest['workspace']['members']
    key = hashlib.sha256()
    for path in ['Cargo.toml', *(f'{member}/Cargo.toml' for member in members)]:
        key.update((root / path).read_bytes())
    key = key.hexdigest()
    if key not in _metadata_cache:
        run = subprocess.run(['cargo', 'metadata', '--no-deps', '--format-version', '1', '--offline'],
                             cwd=root, capture_output=True, encoding='utf-8')
        if run.returncode != 0:
            raise OSError(f'cargo metadata failed: {run.stderr.strip()}')
        directories = {tomllib.loads((root / member / 'Cargo.toml').read_text(encoding='utf-8'))['package']['name']: member
                       for member in members}
        packages = []
        for package in json.loads(run.stdout)['packages']:
            directory = directories[package['name']]
            readme = package['readme'] or 'README.md'
            packages.append({'name': package['name'], 'publish': package['publish'],
                             'documentation': package['documentation'], 'directory': directory,
                             'readme': f'{directory}/{readme}'})
        _metadata_cache[key] = packages
    return _metadata_cache[key]


def publishable(packages):
    """The packages `cargo publish` accepts: `publish` is unset or lists a registry."""
    return [package for package in packages if package['publish'] is None or package['publish']]


def tool_names(block):
    """The tool names a generated tools-reference block lists, in order."""
    return re.findall(r'^- \*\*`([a-z0-9_]+)`\*\*:', block, re.MULTILINE)


def check_tool_headings(docs, report):
    rule = 'tools-headings'
    text = docs.text(TOOLS_DOC)
    block = marked_block(text, TOOLS_BEGIN, TOOLS_END)
    if block is None:
        report.add(rule, TOOLS_DOC, f'needs exactly one {TOOLS_BEGIN.strip()} and one {TOOLS_END.strip()} line, in order')
        return
    expected = tool_names(block)
    if not expected:
        report.add(rule, TOOLS_DOC, 'fact sentence not found: the generated block lists no tool')
    if len(set(expected)) != len(expected):
        report.add(rule, TOOLS_DOC, 'the generated block lists a tool twice')
    outside = text.replace(block, '')
    headings = []
    for number, line in enumerate(strip_hidden(outside).splitlines(), 1):
        if line.startswith('### '):
            headings.append((number, line[4:].strip().strip('`')))
    seen = [name for _, name in headings]
    for name in expected:
        if seen.count(name) != 1:
            report.add(rule, TOOLS_DOC, f'tool {name!r} needs exactly one "### {name}" heading outside the '
                                        f'generated block, found {seen.count(name)}')
    for number, name in headings:
        if name not in expected:
            report.add(rule, f'{TOOLS_DOC}:{number}', f'heading {name!r} names no tool in the generated block')


def check_crate_map(docs, report, release_order, packages):
    rule = 'crate-map'
    body = sections(docs.text('README.md')).get('## Crate map')
    if body is None:
        report.add(rule, 'README.md', 'fact sentence not found: no "## Crate map" heading')
        return
    listed = re.findall(r'^\|\s*`([^`]+)`', body, re.MULTILINE)
    if listed != list(release_order):
        report.add(rule, 'README.md', f'crate map lists {listed} but RELEASE_ORDER is {list(release_order)}')
    flat = collapse(body)
    for package in packages:
        if package['publish'] is not None and not package['publish'] and f"`{package['name']}`" not in flat:
            report.add(rule, 'README.md', f"the unpublished workspace crate {package['name']!r} is not named as internal")
    if not re.search(r'\binternal\b', flat):
        report.add(rule, 'README.md', 'fact sentence not found: the crate map does not say which crates are internal')


def check_three_interfaces(docs, report, tests):
    rule = 'three-interfaces'
    block = marked_block(docs.text('README.md'), INTERFACES_BEGIN, INTERFACES_END)
    if block is None:
        report.add(rule, 'README.md', f'needs exactly one {INTERFACES_BEGIN.strip()} and one {INTERFACES_END.strip()} line, in order')
        return
    lines = block.splitlines()
    positions = [lines.index(heading) if heading in lines else -1 for heading in INTERFACE_HEADINGS]
    if -1 in positions or positions != sorted(positions):
        report.add(rule, 'README.md', f'the block needs the headings {list(INTERFACE_HEADINGS)}, in that order')
    flat = collapse(block)
    for needle in ('cargo add purrassist', 'cargo install purrassist-cli', 'purrassist mcp --root',
                   'purrassist decode', 'purrassist call'):
        if needle not in block:
            report.add(rule, 'README.md', f'fact sentence not found: the block never shows `{needle}`')
    if '`.purrassist/`' not in flat:
        report.add(rule, 'README.md', 'fact sentence not found: the block does not say the server creates `.purrassist/`')
    if EQUIVALENCE_TEST not in flat:
        report.add(rule, 'README.md', f'fact sentence not found: the block never names the test {EQUIVALENCE_TEST}')
    elif not re.search(rf'\bfn {EQUIVALENCE_TEST}\b', tests):
        report.add(rule, 'README.md', f'the test {EQUIVALENCE_TEST} named by the block does not exist in {PROTOCOL_TESTS}')
    check_mcp_config(report, block)


def config_servers(node):
    """Every object in a parsed JSON document that has a `command` member."""
    found = []
    if isinstance(node, dict):
        if 'command' in node:
            found.append(node)
        for value in node.values():
            found.extend(config_servers(value))
    elif isinstance(node, list):
        for value in node:
            found.extend(config_servers(value))
    return found


def check_mcp_config(report, block):
    rule = 'mcp-config'
    configs = re.findall(r'^```json\n(.*?)^```', block, re.MULTILINE | re.DOTALL)
    if len(configs) != 1:
        report.add(rule, 'README.md', f'the three-interfaces block needs exactly one json MCP client configuration, found {len(configs)}')
        return
    try:
        servers = config_servers(json.loads(configs[0]))
    except ValueError as error:
        report.add(rule, 'README.md', f'the MCP client configuration is not valid JSON ({error})')
        return
    if len(servers) != 1:
        report.add(rule, 'README.md', f'the MCP client configuration needs exactly one server with a command, found {len(servers)}')
        return
    server = servers[0]
    args = server.get('args')
    if server['command'] != 'purrassist':
        report.add(rule, 'README.md', f"the MCP client command is {server['command']!r}, expected 'purrassist'")
    if (not isinstance(args, list) or len(args) != 3 or args[:2] != ['mcp', '--root']
            or not isinstance(args[2], str) or not args[2]):
        report.add(rule, 'README.md', f'the MCP client args are {args!r}, expected ["mcp", "--root", DIRECTORY]')


def check_packaged_readmes(docs, report, release_order, packages):
    rule = 'packaged-readmes'
    published = publishable(packages)
    if sorted(package['name'] for package in published) != sorted(release_order):
        report.add(rule, 'Cargo.toml', f"publishable packages {sorted(package['name'] for package in published)} "
                                       f'differ from RELEASE_ORDER {sorted(release_order)}')
    for package in published:
        name = package['readme']
        if not (docs.root / name).is_file():
            report.add(rule, name, f"the README of {package['name']} is missing")
            continue
        for start, matched, phrase in banned_matches(docs.collapsed(name) if name in docs.names else collapse(docs.text(name))):
            report.add(rule, name, f'packaged README has stale or process-bound wording {matched!r} (/{phrase}/)')
        for match in re.finditer(r'docs\.rs/[^/\s)>"]+/latest\b', docs.text(name)):
            report.add(rule, f'{name}:{docs.text(name).count(chr(10), 0, match.start()) + 1}',
                       f'packaged README links {match.group(0)!r}; pin the version, since the page outlives the release')


def check_pinned_links(docs, report, version, repository, packages):
    """Every `blob`/`tree`/`raw` link into the repository in a packaged README, and in the
    `documentation` of a publishable package, names the tag `rust-v<version>` (the page outlives
    `main`), and its path, and the `#fragment` of a Markdown target, resolve in the tree. Links to
    the repository root, `issues/` and `security/` are not matched."""
    rule = 'pinned-links'
    tag = f'rust-v{version}'
    pattern = re.compile('(?i:' + re.escape(repository) + ')' + r'/(blob|tree|raw)/([^/\s)>"\'`]+)((?:/[^\s)>"\'`#]*)?)(?:#([^\s)>"\'`]*))?')
    for package in publishable(packages):
        sources = [(package['readme'], docs.text(package['readme']) if (docs.root / package['readme']).is_file() else '')]
        sources.append((f"{package['directory']}/Cargo.toml", package['documentation'] or ''))
        for where, text in sources:
            for match in pattern.finditer(text):
                line = text.count('\n', 0, match.start()) + 1
                at = f'{where}:{line}' if where.endswith('.md') else where
                kind, ref, path, fragment = match.groups()
                path = urllib.parse.unquote(path.rstrip('.,;:')).strip('/')
                if ref != tag:
                    report.add(rule, at, f'{match.group(0)!r} names {ref!r}, but a published crate page must link {tag}')
                    continue
                target = docs.root / path
                if not target.exists():
                    report.add(rule, at, f'{match.group(0)!r} names {path!r}, which does not exist in the tree')
                elif fragment and target.suffix == '.md' and target.is_file():
                    if urllib.parse.unquote(fragment.rstrip('.,;:')) not in docs.anchors_of(target):
                        report.add(rule, at, f'{match.group(0)!r} names a heading that does not exist in {path}')


def check_workspace_boundaries(docs, report, packages):
    rule = 'workspace-boundaries'
    name = 'docs/IMPLEMENTATION.md'
    body = sections(docs.text(name)).get('## Workspace boundaries')
    if body is None:
        report.add(rule, name, 'fact sentence not found: no "## Workspace boundaries" heading')
        return
    items = re.split(r'^- ', body, flags=re.MULTILINE)[1:]
    listed = {}
    for item in items:
        match = re.match(r'`([^`]+)`', item)
        if match is None:
            report.add(rule, name, f'list item {collapse(item)[:40]!r} does not start with a backticked crate name')
            continue
        listed[match.group(1)] = collapse(item)
    expected = {package['name']: package for package in packages}
    if sorted(listed) != sorted(expected):
        report.add(rule, name, f'lists {sorted(listed)} but the workspace members are {sorted(expected)}')
    for crate, package in expected.items():
        unpublished = package['publish'] is not None and not package['publish']
        if crate in listed and unpublished and not re.search(r'\binternal\b', listed[crate]):
            report.add(rule, name, f'{crate} is not published, so its item must say it is internal')


def check_cli_package(docs, report, token_owner, packages):
    rule = 'registry-name'
    cli = next((package for package in packages if package['name'] == CLI_PACKAGE), None)
    if cli is None:
        report.add(rule, 'Cargo.toml', f'the workspace has no {CLI_PACKAGE} package')
        return
    documentation = cli['documentation'] or ''
    if not documentation or urllib.parse.urlparse(documentation).hostname == 'docs.rs':
        report.add('cli-documentation', f"{cli['directory']}/Cargo.toml",
                   f'the documentation link {documentation!r} must point at the tool reference, not docs.rs')
    name = cli['readme']
    expected = f'mcp-name: io.github.{token_owner}/{REGISTRY_PROJECT}'
    links = sections(strip_hidden(docs.text(name))).get('## Links', '')
    bullets = [line for line in links.splitlines() if re.match(r'[-*] ', line)]
    wanted = f'- MCP Registry name: `{expected}`'
    if wanted not in [line.rstrip() for line in bullets]:
        report.add(rule, name, f'needs a visible "## Links" bullet {wanted!r}, outside any HTML comment or code fence '
                               f'(the owner comes from the workspace repository URL)')


def check_social_preview(root, report):
    """The social preview asset: PNG dimensions and size, registry digests, one font family."""
    rule = 'social-preview'
    registry = 'docs/assets.json'
    entries = json.loads((root / registry).read_text(encoding='utf-8'))['assets']
    entry = next((item for item in entries if item.get('name') == 'social-preview'), None)
    if entry is None:
        report.add(rule, registry, 'no "social-preview" asset is registered')
        return
    source, output = entry['source']['path'], entry['output']['path']
    svg, png = (root / source).read_bytes(), (root / output).read_bytes()
    header = struct.unpack('>II', png[16:24]) if png[:8] == PNG_SIGNATURE and png[12:16] == b'IHDR' else None
    if header != (SOCIAL_WIDTH, SOCIAL_HEIGHT):
        report.add(rule, output, f'IHDR is {header}, expected a PNG of {SOCIAL_WIDTH}x{SOCIAL_HEIGHT}')
    if len(png) >= SOCIAL_MAX_BYTES:
        report.add(rule, output, f'{len(png)} bytes is not under {SOCIAL_MAX_BYTES}')
    for path, data, recorded in ((source, svg, entry['source']['sha256']), (output, png, entry['output']['sha256'])):
        if hashlib.sha256(data).hexdigest() != recorded:
            report.add(rule, path, f'sha256 differs from {registry}; re-render and re-register it')
    text = svg.decode()
    families = [a or b for a, b in re.findall(r'font-family\s*=\s*(?:"([^"]*)"|\'([^\']*)\')', text)]
    families += [value.strip().strip('"\'') if re.fullmatch(r'\s*(["\']).*\1\s*', value) else value.strip()
                 for value in re.findall(r'font-family\s*:\s*([^;}<]*)', text)]
    if not families or any(family != SOCIAL_FONT for family in families):
        report.add(rule, source, f'font-family must be exactly {SOCIAL_FONT!r} with no fallback list; found {families}')
    if entry['font']['family'] != SOCIAL_FONT:
        report.add(rule, registry, f'font family must be {SOCIAL_FONT!r}')


def markdown_names(root):
    """Tracked `*.md` files in scope, from git's index."""
    listing = subprocess.check_output(['git', 'ls-files', '-z', '--', '*.md'], cwd=root, encoding='utf-8')
    return [name for name in listing.split('\0') if name and not name.startswith(EXCLUDED_PREFIXES)]


def check(root, names):
    """Run every rule over `names` (repository-relative Markdown files) under
    `root`; return the error list."""
    root = Path(root)
    docs = Docs(root, names)
    report = Report()
    release = load_module(root / 'scripts/check-release.py', 'purrassist_docs_check_release')
    fuzz = load_module(root / 'scripts/fuzz.py', 'purrassist_docs_fuzz')
    version = tomllib.loads((root / 'Cargo.toml').read_text(encoding='utf-8'))['workspace']['package']['version']
    stems = [path.stem for path in (root / DIALECT_PAGES).glob('*.md')]
    repository = tomllib.loads((root / 'Cargo.toml').read_text(encoding='utf-8'))['workspace']['package']['repository']
    owner = urllib.parse.urlparse(repository).path.strip('/').split('/')[0]
    try:
        packages = workspace_packages(root)
    except OSError as error:
        report.add('cargo-metadata', 'Cargo.toml', str(error))
        packages = None
    rules = (
        lambda: check_release_set(docs, report, release.RELEASE_ORDER),
        lambda: check_workspace_version(docs, report, version),
        lambda: check_fuzz_targets(docs, report, list(fuzz.TARGETS), (root / 'Makefile').read_text(encoding='utf-8')),
        lambda: check_receipt_version(docs, report, (root / 'crates/core/src/contract.rs').read_text(encoding='utf-8')),
        lambda: check_dialects(docs, report, stems),
        lambda: check_links(docs, report),
        lambda: check_stage_references(docs, report),
        lambda: check_banned_phrases(docs, report),
        lambda: check_archived_evidence(docs, report),
        lambda: check_stage2_heading(docs, report),
        lambda: check_windows_run(docs, report),
        lambda: check_tool_headings(docs, report),
        lambda: check_three_interfaces(docs, report, (root / PROTOCOL_TESTS).read_text(encoding='utf-8')),
        lambda: check_changelog(docs, report, version, release.RELEASE_ORDER),
        lambda: check_security_policy(docs, report, version, repository),
        lambda: check_release_reservations(docs, report, release.RELEASE_ORDER),
        lambda: check_release_qualification(docs, report),
        lambda: check_release_toolchain(docs, report, release.stable_pins(docs.text(QUALIFICATION_WORKFLOW))),
        lambda: check_readme_changelog(docs, report),
        lambda: check_social_preview(root, report),
    )
    if packages is not None:
        rules += (
            lambda: check_crate_map(docs, report, release.RELEASE_ORDER, packages),
            lambda: check_packaged_readmes(docs, report, release.RELEASE_ORDER, packages),
            lambda: check_pinned_links(docs, report, version, repository, packages),
            lambda: check_workspace_boundaries(docs, report, packages),
            lambda: check_cli_package(docs, report, owner, packages),
            lambda: check_source_jargon(root, report, packages),
        )
    for rule in rules:
        try:
            rule()
        except FileNotFoundError as error:
            report.add('missing-file', error.filename, 'a file that a rule reads is missing')
    try:
        data = load_limitations(docs, report, list(fuzz.TARGETS))
        if data is not None:
            check_known_limitations(docs, report, data, list(fuzz.TARGETS), version)
    except FileNotFoundError as error:
        report.add('missing-file', error.filename, 'a file that a rule reads is missing')
    return report.errors


def main():
    errors = check(ROOT, markdown_names(ROOT))
    if errors:
        raise SystemExit('\n'.join(errors))
    print('Documentation facts, links, wording and known limitations match their sources.')


if __name__ == '__main__':
    sys.exit(main())
