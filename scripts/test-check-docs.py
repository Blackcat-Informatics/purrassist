#!/usr/bin/env python3
# SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
# SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0
"""Tests for scripts/check-docs.py.

The checker passes on this checkout. Every other test copies the documents and
the files that carry the facts into a temporary tree, breaks one thing, and
requires the rule that owns it to fail by name. Each fact also has a fixture
in which the sentence the rule reads is absent, which must fail with "fact
sentence not found", and a phrase wrapped across a line break must still be
found. Nothing here touches the network or the checkout.
"""
import ast
import importlib.util
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import tempfile
import unittest
from unittest import mock

ROOT = Path(__file__).resolve().parent.parent
SOURCES = ('Cargo.toml', 'Makefile', 'scripts/check-release.py', 'scripts/fuzz.py',
           'crates/core/src/contract.rs', 'docs/known-limitations.json',
           '.github/workflows/release-qualification.yml')
NOT_FOUND = 'fact sentence not found'


def _load():
    path = ROOT / 'scripts/check-docs.py'
    spec = importlib.util.spec_from_file_location('purrassist_check_docs', path)
    if spec is None or spec.loader is None:
        raise SystemExit(f'could not load module spec for {path}')
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


CD = _load()
TRACKED = subprocess.check_output(['git', 'ls-files', '-z'], cwd=ROOT, encoding='utf-8').split('\0')[:-1]


def link_or_copy(source, target):
    """Make `target` a link to `source`, or a copy of it where the platform refuses links
    (Windows without the symlink privilege raises OSError)."""
    try:
        os.symlink(source, target)
    except OSError:
        shutil.copy2(source, target)


# The checkout's 0.1.0 changelog heading, whose status is `unreleased` until the
# release commit replaces it with the tagging date.
CHANGELOG_HEADING = re.search(r'(?m)^## \[0\.1\.0\] - .+$',
                              (ROOT / 'CHANGELOG.md').read_text(encoding='utf-8')).group(0)


class Tree:
    """A temporary copy of the documents and fact sources."""

    def __init__(self, test):
        directory = tempfile.TemporaryDirectory()
        test.addCleanup(directory.cleanup)
        self.root = Path(directory.name)
        self.names = CD.markdown_names(ROOT)
        for name in [*self.names, *SOURCES]:
            self.copy(name)
        shutil.copytree(ROOT / 'docs/qualification', self.root / 'docs/qualification')
        # Every other tracked file is linked in (copied where links are refused), so that
        # relative links to it resolve. A test only ever writes a copied or a new file, never
        # one of these links.
        for name in TRACKED:
            target = self.root / name
            if not target.exists() and not target.is_symlink():
                target.parent.mkdir(parents=True, exist_ok=True)
                link_or_copy(ROOT / name, target)

    def copy(self, name):
        target = self.root / name
        target.parent.mkdir(parents=True, exist_ok=True)
        shutil.copy2(ROOT / name, target)

    def detach(self, name):
        """Replace the link to a tracked file with a private copy that a test may edit."""
        target = self.root / name
        if target.is_symlink() or target.exists():
            target.unlink()
        self.copy(name)

    def read(self, name):
        return (self.root / name).read_text(encoding='utf-8')

    def write(self, name, text):
        (self.root / name).write_text(text, encoding='utf-8')

    def sub(self, name, pattern, replacement, count=1):
        """Replace the regex `pattern` in `name`, which must occur `count` times (0 = any)."""
        text, replaced = re.subn(pattern, lambda _: replacement, self.read(name), flags=re.DOTALL)
        if replaced == 0 or (count and replaced != count):
            raise AssertionError(f'{name}: /{pattern}/ replaced {replaced} times, expected {count or "some"}')
        self.write(name, text)

    def replace(self, name, old, new, count=1):
        """Replace `old` (any whitespace in it matches any whitespace run, so a wrapped
        sentence is found) with the literal `new`."""
        self.sub(name, r'\s+'.join(re.escape(word) for word in old.split()), new, count)

    def append(self, name, text):
        self.write(name, self.read(name) + text)

    def drop(self, name):
        (self.root / name).unlink()
        if name in self.names:
            self.names.remove(name)

    def limitations(self):
        return json.loads(self.read('docs/known-limitations.json'))

    def save_limitations(self, data):
        self.write('docs/known-limitations.json', json.dumps(data, indent=2) + '\n')

    def errors(self):
        return CD.check(self.root, self.names)


class Case(unittest.TestCase):
    def tree(self):
        return Tree(self)

    def assertFails(self, errors, rule, *needles):
        matching = [error for error in errors if error.startswith(f'[{rule}] ')]
        self.assertTrue(
            any(all(needle in error for needle in needles) for error in matching),
            f'no [{rule}] error containing {needles}; got:\n' + '\n'.join(errors))

    def assertNoRule(self, errors, rule):
        self.assertEqual([error for error in errors if error.startswith(f'[{rule}] ')], [])


class Checkout(Case):
    def test_the_checkout_passes(self):
        self.assertEqual(CD.check(ROOT, CD.markdown_names(ROOT)), [])

    def test_scope_is_tracked_markdown_outside_stage_vendor_and_test_data(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            subprocess.run(['git', 'init', '--quiet'], cwd=root, check=True)
            names = ['README.md', 'docs/A.md', 'vendor/x/README.md', 'crates/core/tests/data/x/README.md',
                     '.stage/p/plan.md', 'untracked.md', 'notes.txt']
            for name in names:
                (root / name).parent.mkdir(parents=True, exist_ok=True)
                (root / name).write_text('x\n', encoding='utf-8')
            subprocess.run(['git', 'add', '-f', *names[:5], 'notes.txt'], cwd=root, check=True)
            self.assertEqual(sorted(CD.markdown_names(root)), ['README.md', 'docs/A.md'])


class Helpers(Case):
    def test_collapse_maps_back_to_lines(self):
        text = 'one\n\n  two\tthree\n'
        collapsed, offsets = CD.collapse(text), CD.collapse_offsets(text)
        self.assertEqual(collapsed, 'one two three ')
        self.assertEqual(len(offsets), len(collapsed))
        self.assertEqual(text.count('\n', 0, offsets[collapsed.index('three')]) + 1, 3)

    def test_heading_slug_follows_github(self):
        self.assertEqual(CD.heading_slug('## Known limitations'), 'known-limitations')
        self.assertEqual(CD.heading_slug('### The `make fuzz` (10,000) run'), 'the-make-fuzz-10000-run')

    def test_anchors_number_repeats_and_ignore_fences(self):
        found = CD.anchors('# A\n## A\n```\n# not\n```\n<a id="x"></a>\n')
        self.assertEqual(found, {'a', 'a-1', 'x'})

    def test_report_commits_reads_each_shape(self):
        self.assertEqual(CD.report_commits({'source_commit': 'a' * 40}), {'a' * 40})
        self.assertEqual(CD.report_commits({'source': {'commit': 'b' * 40}}), {'b' * 40})
        self.assertEqual(CD.report_commits({'reports': [{'source_commit': 'c' * 40}, {}]}), {'c' * 40})
        self.assertEqual(CD.report_commits([{'case': 'exact'}]), set())

    def test_commit_named_accepts_a_prefix_of_seven_or_more(self):
        self.assertTrue(CD.commit_named('bound to `66b3756`', '66b3756776a072af'))
        self.assertFalse(CD.commit_named('bound to `66b375`', '66b3756776a072af'))

    def test_number_value(self):
        self.assertEqual((CD.number_value('Twelve'), CD.number_value('12'), CD.number_value('many')), (12, 12, None))


class Portability(Case):
    def test_the_tree_builder_copies_where_symlinks_are_refused(self):
        with tempfile.TemporaryDirectory() as directory:
            source, target = Path(directory) / 'source.txt', Path(directory) / 'target.txt'
            source.write_text('content\n', encoding='utf-8')
            with mock.patch.object(os, 'symlink', side_effect=OSError('privilege not held')) as symlink:
                link_or_copy(source, target)
            symlink.assert_called_once()
            self.assertFalse(target.is_symlink())
            self.assertEqual(target.read_text(encoding='utf-8'), 'content\n')

    def test_the_tree_builder_links_where_symlinks_work(self):
        with tempfile.TemporaryDirectory() as directory:
            source, target = Path(directory) / 'source.txt', Path(directory) / 'target.txt'
            source.write_text('content\n', encoding='utf-8')
            link_or_copy(source, target)
            self.assertEqual(target.read_text(encoding='utf-8'), 'content\n')

    def test_a_tree_still_checks_clean_when_every_symlink_is_refused(self):
        with mock.patch.object(os, 'symlink', side_effect=OSError('privilege not held')):
            tree = Tree(self)
        self.assertEqual(tree.errors(), [])
        self.assertFalse((tree.root / 'Cargo.lock').is_symlink())

    def test_text_is_read_and_written_with_an_explicit_encoding(self):
        """Without `encoding=`, Windows decodes with its locale code page. A call that names none
        is an error under `-X warn_default_encoding -W error::EncodingWarning` on every OS, which
        is how `make hygiene` runs these scripts; this finds the same calls without running them."""
        names = ('check-docs.py', 'test-check-docs.py', 'check-repository.py', 'test-repository-audit.py',
                 'check-release.py', 'fuzz.py', 'test-check-release.py', 'test-fuzz.py')
        for name in names:
            tree = ast.parse((ROOT / 'scripts' / name).read_text(encoding='utf-8'), name)
            for node in ast.walk(tree):
                if not isinstance(node, ast.Call):
                    continue
                function = node.func.attr if isinstance(node.func, ast.Attribute) else getattr(node.func, 'id', '')
                keywords = {keyword.arg: keyword.value for keyword in node.keywords}
                with self.subTest(script=name, line=node.lineno):
                    if function in ('read_text', 'write_text') or (function == 'open' and isinstance(node.func, ast.Name)):
                        self.assertIn('encoding', keywords)
                    for flag in ('text', 'universal_newlines'):
                        self.assertNotIn(flag, keywords)


class ReleaseSet(Case):
    def test_reordered_set_fails(self):
        tree = self.tree()
        tree.replace('docs/RELEASE.md', '`purrassist-core`, `purrassist-derive`', '`purrassist-derive`, `purrassist-core`')
        self.assertFails(tree.errors(), 'release-set', 'RELEASE_ORDER')

    def test_wrong_count_fails(self):
        tree = self.tree()
        tree.replace('docs/RELEASE.md', 'release set is six crates', 'release set is five crates')
        self.assertFails(tree.errors(), 'release-set', "'five'", 'has 6')

    def test_wrong_archive_count_fails(self):
        tree = self.tree()
        tree.replace('docs/QUALIFICATION.md', 'Six Cargo-verified archives', 'Five Cargo-verified archives')
        self.assertFails(tree.errors(), 'release-set', 'archives')

    def test_the_order_comes_from_check_release(self):
        tree = self.tree()
        tree.replace('scripts/check-release.py', "    'purrassist-core',\n", "    'purrassist-core',\n    'purrassist-extra',\n")
        self.assertFails(tree.errors(), 'release-set', 'purrassist-extra')


class WorkspaceVersion(Case):
    def test_a_version_bump_fails_every_restatement(self):
        tree = self.tree()
        tree.replace('Cargo.toml', 'version = "0.1.0"', 'version = "0.2.0"', count=0)
        errors = tree.errors()
        for name in ('docs/RELEASE.md', 'docs/IMPLEMENTATION.md', 'docs/QUALIFICATION.md', 'SECURITY.md', 'README.md'):
            self.assertFails(errors, 'workspace-version', name, '0.2.0')

    def test_a_stale_tag_in_the_status_line_fails(self):
        tree = self.tree()
        tree.sub('SECURITY.md', r'rust-v0\.1\.0', 'rust-v0.0.9')
        self.assertFails(tree.errors(), 'workspace-version', 'SECURITY.md', '0.0.9')

    PUBLISHED = 'published as `rust-v0.1.0`'
    UNPUBLISHED = 'not yet tagged `rust-v0.1.0`'
    # Either status form, so the fixtures hold before and after publication.
    EITHER = r'(?i:not yet tagged|published as)\s+`rust-v0\.1\.0`'

    def publish(self, tree, names):
        for name in names:
            tree.sub(name, self.EITHER, self.PUBLISHED, count=0)

    def unpublish(self, tree, names):
        for name in names:
            tree.sub(name, self.EITHER, self.UNPUBLISHED, count=0)

    def test_the_published_form_in_every_status_doc_passes(self):
        tree = self.tree()
        self.publish(tree, CD.STATUS_DOCS)
        self.assertNoRule(tree.errors(), 'workspace-version')

    def test_each_form_is_accepted_for_the_workspace_version_only(self):
        for old in ('not yet tagged', 'published as'):
            with self.subTest(form=old):
                tree = self.tree()
                if old == 'published as':
                    self.publish(tree, CD.STATUS_DOCS)
                else:
                    self.unpublish(tree, CD.STATUS_DOCS)
                tree.sub('SECURITY.md', rf'{old}\s+`rust-v0\.1\.0`', f'{old} `rust-v0.0.9`')
                self.assertFails(tree.errors(), 'workspace-version', 'SECURITY.md', '0.0.9')

    def test_a_status_line_in_neither_form_is_not_found(self):
        tree = self.tree()
        tree.sub('README.md', self.EITHER, 'out as `rust-v0.1.0`')
        self.assertFails(tree.errors(), 'workspace-version', 'README.md', NOT_FOUND)

    def test_a_mix_of_published_and_unpublished_status_lines_fails(self):
        for published in (('SECURITY.md',), ('README.md', 'docs/RELEASE.md'), CD.STATUS_DOCS[:-1]):
            with self.subTest(published=published):
                tree = self.tree()
                self.unpublish(tree, CD.STATUS_DOCS)
                self.publish(tree, published)
                self.assertFails(tree.errors(), 'workspace-version', 'mix forms', 'not yet tagged', 'published as')

    def test_the_status_forms_are_read_case_insensitively_but_the_version_exactly(self):
        for form in ('Published as', 'PUBLISHED AS', 'Not yet tagged', 'not Yet tagged'):
            with self.subTest(form=form):
                tree = self.tree()
                self.publish(tree, CD.STATUS_DOCS)
                tree.sub('README.md', r'published as `rust-v0\.1\.0`', f'{form} `rust-v0.1.0`')
                errors = tree.errors()
                if form.lower().startswith('published'):
                    self.assertNoRule(errors, 'workspace-version')
                else:
                    self.assertFails(errors, 'workspace-version', 'mix forms')
                tree.sub('README.md', r'`rust-v0\.1\.0`', '`rust-v0.1.1`')
                self.assertFails(tree.errors(), 'workspace-version', 'README.md', '0.1.1')

    def test_a_capitalised_status_line_at_a_sentence_start_passes_unpublished(self):
        tree = self.tree()
        self.unpublish(tree, CD.STATUS_DOCS)
        tree.sub('SECURITY.md', r'not yet tagged', 'Not yet tagged', count=0)
        self.assertNoRule(tree.errors(), 'workspace-version')

    def test_the_release_procedure_documents_the_published_form(self):
        text = CD.collapse((ROOT / 'docs/RELEASE.md').read_text(encoding='utf-8'))
        self.assertIn('read "published as `rust-vX.Y.Z`"', text)


class FuzzTargets(Case):
    def test_a_new_target_fails_every_restatement(self):
        tree = self.tree()
        tree.replace('scripts/fuzz.py', "    'tool-call-ref': {", "    'extra': {},\n    'tool-call-ref': {")
        errors = tree.errors()
        self.assertFails(errors, 'fuzz-targets', 'docs/FUZZING.md', 'extra')
        self.assertFails(errors, 'fuzz-targets', 'Makefile', 'fuzz loop')
        self.assertFails(errors, 'fuzz-targets', 'README.md', 'has 11')
        self.assertFails(errors, 'fuzz-targets', 'docs/QUALIFICATION.md', 'has 11')

    def test_wrong_counts_fail(self):
        for name, old, new in (('README.md', 'ten coverage-guided targets', 'nine coverage-guided targets'),
                               ('docs/QUALIFICATION.md', 'the ten mutation targets', 'the eleven mutation targets'),
                               ('docs/FUZZING.md', 'across ten `purrassist-core`', 'across eleven `purrassist-core`'),
                               ('Makefile', 'across all ten targets', 'across all nine targets')):
            with self.subTest(name=name):
                tree = self.tree()
                tree.replace(name, old, new)
                self.assertFails(tree.errors(), 'fuzz-targets', name)

    def test_the_makefile_loop_must_run_every_target(self):
        tree = self.tree()
        tree.replace('Makefile', ' tool-call-ref; do', '; do', count=0)
        self.assertFails(tree.errors(), 'fuzz-targets', 'Makefile', 'fuzz loop')

    def test_the_hosted_fuzz_loop_must_run_every_target_too(self):
        tree = self.tree()
        tree.replace('Makefile', 'tool-call tool-call-ref; do \\\n\t\tcase', 'tool-call; do \\\n\t\tcase')
        self.assertFails(tree.errors(), 'fuzz-targets', 'Makefile', 'fuzz loop')

    def test_the_makefile_comment_must_list_every_target(self):
        tree = self.tree()
        tree.replace('Makefile', '/tool-call/tool-call-ref', '/tool-call-ref')
        self.assertFails(tree.errors(), 'fuzz-targets', 'Makefile', 'comment lists')

    def test_the_documented_list_must_match(self):
        tree = self.tree()
        tree.replace('docs/FUZZING.md', '`tool-call`, `tool-call-ref`. Each', '`tool-call`. Each')
        self.assertFails(tree.errors(), 'fuzz-targets', 'docs/FUZZING.md', 'lists')


class ReceiptVersion(Case):
    def test_stale_wire_version_fails(self):
        tree = self.tree()
        tree.replace('docs/QUALIFICATION.md', 'receipt wire v3', 'receipt wire v4')
        self.assertFails(tree.errors(), 'receipt-version', 'RECEIPT_VERSION is 3')

    def test_the_source_constant_is_the_truth(self):
        tree = self.tree()
        tree.replace('crates/core/src/contract.rs', 'RECEIPT_VERSION: u32 = 3;', 'RECEIPT_VERSION: u32 = 4;')
        self.assertFails(tree.errors(), 'receipt-version', 'RECEIPT_VERSION is 4')


class DialectIds(Case):
    def test_a_new_dialect_page_fails_every_list(self):
        tree = self.tree()
        tree.write('docs/tool-calls/dialects/pythonic.v1.md', '# x\n')
        errors = tree.errors()
        for name in ('README.md', 'crates/core/README.md', 'docs/QUALIFICATION.md', 'docs/IMPLEMENTATION.md',
                     'docs/tool-calls/GRAMMAR.md', 'docs/tool-calls/README.md', 'docs/MATCHING-QUALITY.md'):
            self.assertFails(errors, 'dialect-ids', name, "missing ['pythonic.v1']")

    def test_a_removed_page_fails_with_the_extra_id(self):
        tree = self.tree()
        tree.drop('docs/tool-calls/dialects/hermes.v1.md')
        self.assertFails(tree.errors(), 'dialect-ids', 'README.md', "extra ['hermes.v1']")

    def test_a_list_missing_an_id_fails(self):
        tree = self.tree()
        tree.replace('crates/purrassist/README.md', '`hermes.v1`, ', '')
        self.assertFails(tree.errors(), 'dialect-ids', 'crates/purrassist/README.md', "missing ['hermes.v1']")

    def test_a_linked_list_missing_an_id_fails(self):
        tree = self.tree()
        tree.sub('docs/tool-calls/GRAMMAR.md', r'\[`hermes\.v1`\]\([^)]*\),\n', '')
        self.assertFails(tree.errors(), 'dialect-ids', 'docs/tool-calls/GRAMMAR.md', "missing ['hermes.v1']")

    def test_a_table_missing_a_row_fails(self):
        tree = self.tree()
        tree.sub('docs/tool-calls/README.md', r'\| `harmony\.v1` \|[^\n]*\n', '')
        self.assertFails(tree.errors(), 'dialect-ids', 'docs/tool-calls/README.md', "missing ['harmony.v1']")

    def test_a_list_with_an_unknown_id_fails(self):
        tree = self.tree()
        tree.replace('crates/host/README.md', '`bare-json.v1`', '`bare-json.v2`')
        self.assertFails(tree.errors(), 'dialect-ids', 'crates/host/README.md', 'bare-json.v2')

    def test_a_list_broken_into_short_pieces_is_not_found(self):
        tree = self.tree()
        tree.replace('crates/host/README.md', '`llama3-json.v1`', 'llama3-json.v1')
        tree.replace('crates/host/README.md', '`kimi-k2.v1`', 'kimi-k2.v1')
        self.assertFails(tree.errors(), 'dialect-ids', 'crates/host/README.md', NOT_FOUND)

    def test_a_list_wrapped_across_lines_is_still_a_list(self):
        tree = self.tree()
        tree.replace('crates/core/README.md', '`hermes.v1`, `openai-tool-calls.v1`,', '`hermes.v1`,\n`openai-tool-calls.v1`,')
        tree.replace('crates/core/README.md', '`mistral-v3.v1`, ', '')
        self.assertFails(tree.errors(), 'dialect-ids', 'crates/core/README.md', "missing ['mistral-v3.v1']")

    def test_wrong_count_fails(self):
        for name, old, new in (('README.md', 'Twelve dialects ship', 'Eleven dialects ship'),
                               ('docs/tool-calls/GRAMMAR.md', 'twelve built-in dialects', 'ten built-in dialects'),
                               ('docs/RESOURCES.md', 'all twelve built-in dialects', 'all ten built-in dialects'),
                               ('docs/tool-calls/dialects/bare-json.v1.md', 'all twelve dialects', 'all ten dialects'),
                               ('docs/IMPLEMENTATION.md', 'twelve ship', 'ten ship'),
                               ('crates/host/README.md', 'over twelve built-in', 'over ten built-in')):
            with self.subTest(name=name):
                tree = self.tree()
                tree.replace(name, old, new)
                self.assertFails(tree.errors(), 'dialect-count', name, 'has 12')

    def test_a_subset_count_is_not_a_dialect_count(self):
        tree = self.tree()
        tree.append('CONTRIBUTING.md', '\nThe slip is tolerated across two dialects (all six dialects, here).\n')
        self.assertNoRule(tree.errors(), 'dialect-count')

    def test_a_missing_count_sentence_is_not_found(self):
        tree = self.tree()
        tree.replace('crates/purrassist/README.md', 'Twelve dialects ship:', 'These dialects exist:')
        self.assertFails(tree.errors(), 'dialect-count', 'crates/purrassist/README.md', NOT_FOUND)


class Links(Case):
    def test_a_broken_relative_link_fails(self):
        tree = self.tree()
        tree.append('README.md', '\nSee [gone](docs/NOPE.md).\n')
        self.assertFails(tree.errors(), 'links', 'README.md', 'docs/NOPE.md')

    def test_a_broken_anchor_fails(self):
        tree = self.tree()
        tree.append('README.md', '\nSee [x](docs/QUALIFICATION.md#no-such-heading).\n')
        self.assertFails(tree.errors(), 'links', 'no-such-heading')

    def test_a_broken_same_file_anchor_fails(self):
        tree = self.tree()
        tree.append('README.md', '\nSee [x](#no-such-heading).\n')
        self.assertFails(tree.errors(), 'links', 'no-such-heading')

    def test_a_broken_reference_definition_fails(self):
        tree = self.tree()
        tree.append('README.md', '\n[ref]: docs/NOPE.md\n')
        self.assertFails(tree.errors(), 'links', 'docs/NOPE.md')

    def test_a_broken_image_source_fails(self):
        tree = self.tree()
        tree.append('README.md', '\n<img src="docs/nope.svg">\n')
        self.assertFails(tree.errors(), 'links', 'docs/nope.svg')

    def test_links_in_code_and_external_links_are_not_checked(self):
        tree = self.tree()
        tree.append('README.md', '\n```\n[gone](docs/NOPE.md)\n```\n`[gone](docs/NOPE.md)` [site](https://example.org/x#y)\n')
        self.assertNoRule(tree.errors(), 'links')

    def test_a_valid_anchor_passes(self):
        tree = self.tree()
        tree.append('README.md', '\nSee [x](docs/QUALIFICATION.md#known-limitations).\n')
        self.assertNoRule(tree.errors(), 'links')


class StageReferences(Case):
    def test_a_stage_directory_reference_fails(self):
        tree = self.tree()
        tree.append('docs/RELEASE.md', '\nSee `.stage/plan/plan.md`.\n')
        self.assertFails(tree.errors(), 'stage-references', 'docs/RELEASE.md')

    def test_words_that_merely_contain_stage_pass(self):
        tree = self.tree()
        tree.append('docs/RELEASE.md', '\nA backstage .stagecoach is fine; so is stage 2.\n')
        self.assertNoRule(tree.errors(), 'stage-references')


BANNED_SAMPLES = {
    r'issue branch': 'Delivered on the issue branch.',
    r'nine (mutation |coverage-guided )?targets': 'It runs Nine targets.',
    r'wire v2': 'The receipt wire v2 is current.',
    r'reviewed merge commit': 'Use the reviewed merge commit.',
    r'follows? (the )?review and merge': 'Publication follows review and merge.',
    r'after review and merge': 'Release after review and merge.',
    r'no placeholder record': 'There is no placeholder record.',
    r'not yet done': 'That is not yet done.',
    r'five of six': 'Configured for five of six crates.',
    r'Pre-release implementation': 'This is a pre-release implementation.',
    r'stage report': 'See its stage report.',
    r'proved by the [^.]*hosted': 'Windows is proved by the windows-engines hosted lane.',
    r'uploaded manually': 'The image is uploaded manually.',
    r'compares the output byte for byte': 'The test compares the output byte for byte.',
    r'every result carries': 'Every result carries its evidence.',
    CD.TASK_NUMBER: 'See Task 12.',
}


class BannedPhrases(Case):
    def test_samples_cover_the_exact_list(self):
        self.assertEqual(set(BANNED_SAMPLES), set(CD.BANNED_PHRASES))

    def test_every_phrase_fails(self):
        for phrase, sample in BANNED_SAMPLES.items():
            with self.subTest(phrase=phrase):
                tree = self.tree()
                tree.append('CONTRIBUTING.md', f'\n{sample}\n')
                self.assertFails(tree.errors(), 'banned-phrase', 'CONTRIBUTING.md', f'/{phrase}/')

    def test_a_phrase_wrapped_across_lines_fails_with_its_first_line(self):
        for phrase, sample in BANNED_SAMPLES.items():
            with self.subTest(phrase=phrase):
                tree = self.tree()
                match = re.search(phrase, sample, re.IGNORECASE)
                if ' ' not in match.group(0):
                    continue
                wrapped = sample[:match.start()] + match.group(0).replace(' ', '\n', 1) + sample[match.end():]
                before = len(tree.read('CONTRIBUTING.md').splitlines())
                tree.append('CONTRIBUTING.md', f'\n{wrapped}\n')
                # The phrase starts on the first line of the wrapped sentence.
                self.assertFails(tree.errors(), 'banned-phrase', f'CONTRIBUTING.md:{before + 2}', f'/{phrase}/')

    def test_a_phrase_wrapped_with_an_indent_fails(self):
        tree = self.tree()
        tree.append('CONTRIBUTING.md', '\nUse the reviewed\n    merge   commit now.\n')
        self.assertFails(tree.errors(), 'banned-phrase', 'reviewed merge commit')

    def test_matching_ignores_case(self):
        tree = self.tree()
        tree.append('CONTRIBUTING.md', '\nPRE-RELEASE IMPLEMENTATION\n')
        self.assertFails(tree.errors(), 'banned-phrase', 'PRE-RELEASE IMPLEMENTATION')

    def test_a_pre_release_version_is_not_a_pre_release_implementation(self):
        tree = self.tree()
        tree.append('CONTRIBUTING.md', '\nA pre-release version such as `0.2.0-alpha.1` is a SemVer pre-release.\n')
        self.assertNoRule(tree.errors(), 'banned-phrase')

    def test_task_numbers_are_banned_but_ordinary_words_are_not(self):
        tree = self.tree()
        tree.append('CONTRIBUTING.md', '\nThe task list has 12 items; tasks 12 and 13 pass 2 arguments.\n')
        self.assertNoRule(tree.errors(), 'banned-phrase')
        tree.append('CONTRIBUTING.md', '\nSee task\n7 for the rest.\n')
        self.assertFails(tree.errors(), 'banned-phrase', 'CONTRIBUTING.md', 'task 7')

    def test_a_task_number_in_a_packaged_readme_fails(self):
        tree = self.tree()
        tree.append('crates/host/README.md', '\nSee Task 12.\n')
        self.assertFails(tree.errors(), 'packaged-readmes', 'crates/host/README.md', 'Task 12')

    def test_published_in_this_order_is_not_a_global_ban(self):
        self.assertNotIn('published in this order', CD.BANNED_PHRASES)
        tree = self.tree()
        tree.append('README.md', '\nThe release set, published in this order:\n')
        self.assertNoRule(tree.errors(), 'banned-phrase')
        self.assertNoRule(tree.errors(), 'changelog')


class SourceJargon(Case):
    def test_the_pattern_is_shared_with_the_markdown_ban(self):
        self.assertIn(CD.TASK_NUMBER, CD.BANNED_PHRASES)

    def test_a_task_number_in_a_published_crate_source_fails(self):
        tree = self.tree()
        tree.write('crates/host/src/zz_jargon.rs', '//! Wired in Task 12.\n')
        self.assertFails(tree.errors(), 'source-jargon', 'crates/host/src/zz_jargon.rs:1', 'Task 12')

    def test_a_nested_source_and_any_case_fail(self):
        tree = self.tree()
        tree.write('crates/core/src/text/zz_jargon.rs', 'fn f() {}\n// see TASK 7\n')
        self.assertFails(tree.errors(), 'source-jargon', 'crates/core/src/text/zz_jargon.rs:2', 'TASK 7')

    def test_the_same_text_outside_published_sources_passes(self):
        tree = self.tree()
        tree.write('crates/host/tests/zz_jargon.rs', '//! Wired in Task 12.\n')
        (tree.root / 'crates/host/benches').mkdir()
        tree.write('crates/host/benches/zz_jargon.rs', '//! Wired in Task 12.\n')
        tree.write('crates/helper-census/src/zz_jargon.rs', '//! Wired in Task 12.\n')
        self.assertNoRule(tree.errors(), 'source-jargon')

    def test_a_requirement_number_fails_and_the_word_alone_passes(self):
        tree = self.tree()
        tree.write('crates/core/src/zz_jargon.rs', '/// Requirement 5: x.\n')
        self.assertFails(tree.errors(), 'source-jargon', 'crates/core/src/zz_jargon.rs:1', 'Requirement 5')
        tree.write('crates/core/src/zz_jargon.rs', '/// Each requirement is stated; requirements 1x, a requirement list.\n')
        self.assertNoRule(tree.errors(), 'source-jargon')

    def test_a_reference_wrapped_across_comment_lines_fails_at_its_first_line(self):
        for first, second, needle in (('/// See Task', '/// 12 for it.', 'Task 12'),
                                      ('    // the requirement', '    // 3 of it', 'requirement 3'),
                                      ('//! A Task', '//!     7.', 'Task 7')):
            with self.subTest(needle=needle):
                tree = self.tree()
                tree.write('crates/host/src/zz_jargon.rs', f'fn f() {{}}\n{first}\n{second}\n')
                self.assertFails(tree.errors(), 'source-jargon', 'crates/host/src/zz_jargon.rs:2', needle)

    def test_a_number_on_a_code_line_after_a_comment_is_not_joined(self):
        tree = self.tree()
        tree.write('crates/host/src/zz_jargon.rs', '/// the task\nlet x = 12;\n')
        self.assertNoRule(tree.errors(), 'source-jargon')

    def test_ordinary_words_pass(self):
        tree = self.tree()
        tree.write('crates/host/src/zz_jargon.rs', '//! The tasks run; the task list and a task 1x are fine.\n')
        self.assertNoRule(tree.errors(), 'source-jargon')


class KnownLimitationsSource(Case):
    def test_a_blocking_issue_fails(self):
        tree = self.tree()
        data = tree.limitations()
        data['limitations'][0]['blocks_release'] = True
        tree.save_limitations(data)
        self.assertFails(tree.errors(), 'known-limitations-source', 'blocks_release must be false')

    def test_a_missing_key_fails(self):
        tree = self.tree()
        data = tree.limitations()
        del data['limitations'][1]['impact']
        tree.save_limitations(data)
        self.assertFails(tree.errors(), 'known-limitations-source', 'exactly issue, title, impact')

    def test_a_duplicate_issue_fails(self):
        tree = self.tree()
        data = tree.limitations()
        data['limitations'][1]['issue'] = data['limitations'][0]['issue']
        tree.save_limitations(data)
        self.assertFails(tree.errors(), 'known-limitations-source', 'duplicate issue')

    def test_a_multi_line_impact_fails(self):
        tree = self.tree()
        data = tree.limitations()
        data['limitations'][0]['impact'] = 'one\ntwo'
        tree.save_limitations(data)
        self.assertFails(tree.errors(), 'known-limitations-source', 'impact must be one non-empty line')

    def test_unparseable_json_fails(self):
        tree = self.tree()
        tree.write('docs/known-limitations.json', '{')
        self.assertFails(tree.errors(), 'known-limitations-source', 'unreadable')

    def test_a_missing_file_fails(self):
        tree = self.tree()
        (tree.root / 'docs/known-limitations.json').unlink()
        self.assertFails(tree.errors(), 'missing-file', 'known-limitations.json')

    def test_the_real_set_is_the_agreed_one(self):
        data = json.loads((ROOT / 'docs/known-limitations.json').read_text(encoding='utf-8'))
        self.assertEqual([entry['issue'] for entry in data['limitations']], [6, 7, 10, 11, 12, 13, 14, 16, 17, 18])


class KnownLimitationsSection(Case):
    def test_a_missing_heading_fails_in_each_document(self):
        for name in ('docs/QUALIFICATION.md', 'docs/FUZZING.md'):
            with self.subTest(name=name):
                tree = self.tree()
                tree.replace(name, '## Known limitations', '## Open items')
                self.assertFails(tree.errors(), 'known-limitations-section', name, 'exactly one')

    def test_a_dropped_issue_fails_in_each_document(self):
        for name in ('docs/QUALIFICATION.md', 'docs/FUZZING.md'):
            with self.subTest(name=name):
                tree = self.tree()
                tree.sub(name, r'- \[#18\][^\n]*\n', '')
                self.assertFails(tree.errors(), 'known-limitations-section', name, 'differs')

    def test_an_extra_issue_in_the_source_fails_in_each_document(self):
        tree = self.tree()
        data = tree.limitations()
        data['limitations'].append({'issue': 99, 'title': 'New', 'impact': 'New impact.', 'blocks_release': False})
        tree.save_limitations(data)
        for name in ('docs/QUALIFICATION.md', 'docs/FUZZING.md'):
            self.assertFails(tree.errors(), 'known-limitations-section', name)

    def test_an_issue_outside_the_source_fails(self):
        tree = self.tree()
        tree.replace('docs/FUZZING.md', 'The default `make fuzz` stops on the #11 and #6 signatures.',
                     'The default `make fuzz` stops on the #11 and #6 signatures (see #3).')
        self.assertFails(tree.errors(), 'known-limitations-section', 'docs/FUZZING.md', '3')

    def test_a_changed_title_in_the_source_fails(self):
        tree = self.tree()
        data = tree.limitations()
        data['limitations'][0]['title'] = 'A different title'
        tree.save_limitations(data)
        self.assertFails(tree.errors(), 'known-limitations-section', 'docs/QUALIFICATION.md', '#6 title')

    def test_a_changed_impact_in_the_document_fails(self):
        tree = self.tree()
        data = tree.limitations()
        tree.replace('docs/FUZZING.md', data['limitations'][3]['impact'], 'It is fine.')
        self.assertFails(tree.errors(), 'known-limitations-section', 'docs/FUZZING.md', '#11 impact')

    def test_a_missing_stop_sentence_is_not_found(self):
        for name in ('docs/QUALIFICATION.md', 'docs/FUZZING.md'):
            with self.subTest(name=name):
                tree = self.tree()
                tree.replace(name, '`make fuzz` stops on the #11 and #6 signatures', '`make fuzz` passes', count=0 if name.endswith('QUALIFICATION.md') else 1)
                self.assertFails(tree.errors(), 'known-limitations-section', name, NOT_FOUND)

    def test_a_stop_citing_an_unlisted_issue_fails(self):
        tree = self.tree()
        tree.replace('docs/FUZZING.md', 'stops on the #11 and #6 signatures.', 'stops on the #11 and #8 signatures.')
        self.assertFails(tree.errors(), 'known-limitations-section', 'docs/FUZZING.md')

    def test_a_changelog_is_held_to_the_same_source(self):
        tree = self.tree()
        tree.write('CHANGELOG.md', '<!-- SPDX-FileCopyrightText: x -->\n# Changelog\n\n## [0.1.0] - unreleased\n\n'
                                   '### Known limitations\n\n- [#6](x)\n')
        tree.names.append('CHANGELOG.md')
        self.assertFails(tree.errors(), 'known-limitations-section', 'CHANGELOG.md')


class FuzzStopTargets(Case):
    """The issues on which the default `make fuzz` stops come from `fuzz_stop_targets`."""

    def stops(self, tree):
        return [entry['issue'] for entry in tree.limitations()['limitations'] if entry.get('fuzz_stop_targets')]

    def set_stop(self, tree, issue, value):
        data = tree.limitations()
        entry = next(entry for entry in data['limitations'] if entry['issue'] == issue)
        if value is None:
            entry.pop('fuzz_stop_targets', None)
        else:
            entry['fuzz_stop_targets'] = value
        tree.save_limitations(data)

    def test_the_documented_stops_are_the_json_stops(self):
        self.assertEqual(self.stops(self.tree()), [6, 11])

    def test_a_stop_named_by_the_json_but_omitted_by_the_documents_fails(self):
        tree = self.tree()
        self.set_stop(tree, 10, ['json'])
        errors = tree.errors()
        for name in ('docs/QUALIFICATION.md', 'docs/FUZZING.md'):
            self.assertFails(errors, 'known-limitations-section', name, 'missing [10]')
        self.assertFails(errors, 'fuzz-row-limits', 'docs/QUALIFICATION.md', 'missing [10]')

    def test_a_stop_named_by_the_documents_but_not_the_json_fails(self):
        tree = self.tree()
        self.set_stop(tree, 6, None)
        errors = tree.errors()
        for name in ('docs/QUALIFICATION.md', 'docs/FUZZING.md'):
            self.assertFails(errors, 'known-limitations-section', name, 'extra [6]')
        self.assertFails(errors, 'fuzz-row-limits', 'docs/QUALIFICATION.md', 'extra [6]')

    def test_an_empty_list_is_no_stop(self):
        tree = self.tree()
        self.set_stop(tree, 11, [])
        self.assertFails(tree.errors(), 'known-limitations-section', 'docs/FUZZING.md', 'extra [11]')

    def test_documents_and_json_that_agree_on_three_stops_pass(self):
        tree = self.tree()
        self.set_stop(tree, 10, ['json'])
        tree.replace('docs/QUALIFICATION.md', 'stops on the #11 and #6 signatures', 'stops on the #11, #6 and #10 signatures', count=0)
        tree.replace('docs/FUZZING.md', 'stops on the #11 and #6 signatures', 'stops on the #11, #6 and #10 signatures')
        errors = tree.errors()
        self.assertNoRule(errors, 'known-limitations-section')
        self.assertNoRule(errors, 'fuzz-row-limits')

    def test_a_single_stop_is_read_in_its_own_form(self):
        tree = self.tree()
        self.set_stop(tree, 6, None)
        tree.replace('docs/QUALIFICATION.md', 'stops on the #11 and #6 signatures', 'stops on the #11 signatures', count=0)
        tree.replace('docs/FUZZING.md', 'stops on the #11 and #6 signatures', 'stops on the #11 signatures')
        errors = tree.errors()
        self.assertNoRule(errors, 'known-limitations-section')
        self.assertNoRule(errors, 'fuzz-row-limits')

    def test_an_oxford_comma_is_accepted(self):
        tree = self.tree()
        self.set_stop(tree, 10, ['json'])
        tree.replace('docs/QUALIFICATION.md', 'stops on the #11 and #6 signatures', 'stops on the #11, #6, and #10 signatures', count=0)
        tree.replace('docs/FUZZING.md', 'stops on the #11 and #6 signatures', 'stops on the #11, #6, and #10 signatures')
        errors = tree.errors()
        self.assertNoRule(errors, 'known-limitations-section')
        self.assertNoRule(errors, 'fuzz-row-limits')

    def test_an_oxford_comma_before_a_second_issue_is_accepted(self):
        tree = self.tree()
        tree.replace('docs/FUZZING.md', 'stops on the #11 and #6 signatures', 'stops on the #11, and #6 signatures')
        self.assertNoRule(tree.errors(), 'known-limitations-section')

    NO_STOP = 'stops on no known signature'

    def without_stops(self, tree):
        self.set_stop(tree, 6, None)
        self.set_stop(tree, 11, [])

    def say_no_stop(self, tree, names=('docs/QUALIFICATION.md', 'docs/FUZZING.md')):
        for name in names:
            tree.replace(name, 'stops on the #11 and #6 signatures', self.NO_STOP, count=0)

    def test_no_stop_in_the_json_requires_the_documents_to_say_so(self):
        tree = self.tree()
        self.without_stops(tree)
        self.say_no_stop(tree)
        errors = tree.errors()
        self.assertNoRule(errors, 'known-limitations-section')
        self.assertNoRule(errors, 'fuzz-row-limits')

    def test_no_stop_in_the_json_fails_documents_that_still_name_stops(self):
        tree = self.tree()
        self.without_stops(tree)
        errors = tree.errors()
        for name in ('docs/QUALIFICATION.md', 'docs/FUZZING.md'):
            self.assertFails(errors, 'known-limitations-section', name, 'extra [6, 11]')
        self.assertFails(errors, 'fuzz-row-limits', 'docs/QUALIFICATION.md', 'extra [6, 11]')

    def test_no_stop_in_the_json_does_not_demand_a_stop_sentence_but_a_no_stop_one(self):
        tree = self.tree()
        self.without_stops(tree)
        self.say_no_stop(tree)
        tree.replace('docs/FUZZING.md', self.NO_STOP, 'passes')
        self.assertFails(tree.errors(), 'known-limitations-section', 'docs/FUZZING.md', NOT_FOUND, self.NO_STOP)

    def test_a_document_that_claims_no_stop_while_the_json_names_one_fails(self):
        tree = self.tree()
        self.say_no_stop(tree)
        errors = tree.errors()
        for name in ('docs/QUALIFICATION.md', 'docs/FUZZING.md'):
            self.assertFails(errors, 'known-limitations-section', name, 'missing [6, 11]')
        self.assertFails(errors, 'fuzz-row-limits', 'docs/QUALIFICATION.md', 'missing [6, 11]')

    def test_a_document_that_states_both_stop_forms_fails(self):
        tree = self.tree()
        tree.replace('docs/FUZZING.md', 'stops on the #11 and #6 signatures.',
                     f'stops on the #11 and #6 signatures. The default `make fuzz` {self.NO_STOP}.')
        tree.replace('docs/QUALIFICATION.md', 'stops on the #11 and #6 signatures, so it',
                     f'stops on the #11 and #6 signatures and {self.NO_STOP}, so it')
        errors = tree.errors()
        self.assertFails(errors, 'known-limitations-section', 'docs/FUZZING.md', 'both names issues')
        self.assertFails(errors, 'fuzz-row-limits', 'docs/QUALIFICATION.md', 'both names stop issues')

    def test_an_unknown_target_fails(self):
        tree = self.tree()
        self.set_stop(tree, 6, ['tool-call', 'no-such-target'])
        self.assertFails(tree.errors(), 'known-limitations-source', 'entry 6', "'no-such-target'", 'TARGETS')

    def test_a_target_that_scripts_fuzz_gains_is_known(self):
        tree = self.tree()
        self.set_stop(tree, 10, ['extra'])
        tree.replace('scripts/fuzz.py', "    'tool-call-ref': {", "    'extra': {},\n    'tool-call-ref': {")
        self.assertNoRule(tree.errors(), 'known-limitations-source')

    def test_a_malformed_value_fails(self):
        for value in ('tool-call', [1], [['tool-call']], {'tool-call': True}, None):
            with self.subTest(value=value):
                tree = self.tree()
                data = tree.limitations()
                data['limitations'][0]['fuzz_stop_targets'] = value
                tree.save_limitations(data)
                self.assertFails(tree.errors(), 'known-limitations-source', 'entry 6', 'fuzz_stop_targets must be a list')

    def test_a_target_listed_twice_fails(self):
        tree = self.tree()
        self.set_stop(tree, 6, ['tool-call', 'tool-call'])
        self.assertFails(tree.errors(), 'known-limitations-source', 'entry 6', 'twice')

    def test_the_exact_key_check_is_not_loosened(self):
        tree = self.tree()
        data = tree.limitations()
        data['limitations'][0]['stop_targets'] = ['tool-call']
        tree.save_limitations(data)
        self.assertFails(tree.errors(), 'known-limitations-source', 'entry 6', 'exactly issue, title, impact')

    def test_the_stop_sentence_is_still_required(self):
        tree = self.tree()
        tree.replace('docs/FUZZING.md', 'The default `make fuzz` stops on the #11 and #6 signatures.', 'The default `make fuzz` passes.')
        self.assertFails(tree.errors(), 'known-limitations-section', 'docs/FUZZING.md', NOT_FOUND)


class KnownLimitationsChangelog(Case):
    HEADING = '### Known limitations'

    def limitations_block(self, tree):
        text = tree.read('CHANGELOG.md')
        return text[text.index(self.HEADING):]

    def test_the_known_limitations_are_part_of_the_release_body(self):
        text = (ROOT / 'CHANGELOG.md').read_text(encoding='utf-8')
        _, body = CD.changelog_section(text, '0.1.0')
        self.assertIn(self.HEADING, body)
        self.assertNotIn('\n## Known limitations', text)

    def test_a_missing_heading_fails(self):
        tree = self.tree()
        tree.replace('CHANGELOG.md', self.HEADING, '### Open items')
        self.assertFails(tree.errors(), 'known-limitations-section', 'CHANGELOG.md', 'exactly one')

    def test_a_sibling_level_two_section_is_outside_the_release_body(self):
        tree = self.tree()
        tree.replace('CHANGELOG.md', self.HEADING, '## Known limitations')
        self.assertFails(tree.errors(), 'known-limitations-section', 'CHANGELOG.md', 'exactly one', '[0.1.0]')

    def test_the_subsection_outside_the_version_section_fails(self):
        tree = self.tree()
        block = self.limitations_block(tree)
        tree.replace('CHANGELOG.md', block.rstrip('\n'), '')
        tree.replace('CHANGELOG.md', CHANGELOG_HEADING, block.rstrip('\n') + '\n\n' + CHANGELOG_HEADING)
        self.assertFails(tree.errors(), 'known-limitations-section', 'CHANGELOG.md', 'exactly one', '[0.1.0]')

    def test_a_second_subsection_in_the_version_section_fails(self):
        tree = self.tree()
        tree.append('CHANGELOG.md', '\n' + self.limitations_block(tree))
        self.assertFails(tree.errors(), 'known-limitations-section', 'CHANGELOG.md', 'exactly one')

    def test_each_version_has_its_own_subsection_and_only_the_current_one_is_compared(self):
        newer = '## [0.2.0] - 2027-01-01\n\nMore.\n\n### Known limitations\n\n- [#99](x) A later issue.\n\n'
        older = '\n## [0.0.9] - 2020-01-01\n\nEarlier.\n\n### Known limitations\n\n- [#98](x) An earlier issue.\n'
        tree = self.tree()
        tree.replace('CHANGELOG.md', CHANGELOG_HEADING, newer + CHANGELOG_HEADING)
        tree.append('CHANGELOG.md', older)
        errors = tree.errors()
        self.assertNoRule(errors, 'known-limitations-section')
        self.assertFails(errors, 'changelog', '[0.2.0]', 'workspace version is 0.1.0')

    def test_another_versions_subsection_does_not_satisfy_the_current_one(self):
        tree = self.tree()
        block = self.limitations_block(tree)
        tree.sub('CHANGELOG.md', r'- \[#18\][^\n]*\n', '')
        tree.append('CHANGELOG.md', '\n## [0.0.9] - 2020-01-01\n\n' + block)
        self.assertFails(tree.errors(), 'known-limitations-section', 'CHANGELOG.md', 'differs')

    def test_a_workspace_version_without_a_section_fails(self):
        tree = self.tree()
        tree.replace('Cargo.toml', 'version = "0.1.0"', 'version = "0.2.0"', count=0)
        self.assertFails(tree.errors(), 'known-limitations-section', 'CHANGELOG.md', '[0.2.0]')

    def test_a_dropped_issue_fails(self):
        tree = self.tree()
        tree.sub('CHANGELOG.md', r'- \[#18\][^\n]*\n', '')
        self.assertFails(tree.errors(), 'known-limitations-section', 'CHANGELOG.md', 'differs')

    def test_an_extra_issue_in_the_source_fails(self):
        tree = self.tree()
        data = tree.limitations()
        data['limitations'].append({'issue': 99, 'title': 'New', 'impact': 'New impact.', 'blocks_release': False})
        tree.save_limitations(data)
        self.assertFails(tree.errors(), 'known-limitations-section', 'CHANGELOG.md')

    def test_a_changed_impact_fails(self):
        tree = self.tree()
        data = tree.limitations()
        tree.replace('CHANGELOG.md', data['limitations'][3]['impact'], 'It is fine.')
        self.assertFails(tree.errors(), 'known-limitations-section', 'CHANGELOG.md', '#11 impact')

    def test_a_changed_title_in_the_source_fails(self):
        tree = self.tree()
        data = tree.limitations()
        data['limitations'][0]['title'] = 'A different title'
        tree.save_limitations(data)
        self.assertFails(tree.errors(), 'known-limitations-section', 'CHANGELOG.md', '#6 title')

    def test_a_missing_changelog_fails(self):
        tree = self.tree()
        tree.drop('CHANGELOG.md')
        self.assertFails(tree.errors(), 'missing-file', 'CHANGELOG.md')


class Changelog(Case):
    def test_a_workspace_version_bump_fails(self):
        tree = self.tree()
        tree.replace('Cargo.toml', 'version = "0.1.0"', 'version = "0.2.0"', count=0)
        self.assertFails(tree.errors(), 'changelog', '[0.1.0]', 'workspace version is 0.2.0')

    def test_a_changelog_version_that_differs_fails(self):
        tree = self.tree()
        tree.replace('CHANGELOG.md', CHANGELOG_HEADING, '## [0.0.9] - unreleased')
        self.assertFails(tree.errors(), 'changelog', '[0.0.9]', 'workspace version is 0.1.0')

    def test_a_newer_unreleased_version_above_fails(self):
        tree = self.tree()
        tree.replace('CHANGELOG.md', CHANGELOG_HEADING, '## [0.2.0] - unreleased\n\n## [0.1.0] - unreleased')
        self.assertFails(tree.errors(), 'changelog', '[0.2.0]')

    def test_a_crate_that_is_not_named_fails(self):
        release = CD.load_module(ROOT / 'scripts/check-release.py', 'purrassist_test_release')
        for crate in release.RELEASE_ORDER:
            with self.subTest(crate=crate):
                tree = self.tree()
                tree.replace('CHANGELOG.md', f'`{crate}`', crate, count=0)
                self.assertFails(tree.errors(), 'changelog', f'`{crate}`')

    def test_an_interface_that_is_not_highlighted_fails(self):
        for label in CD.CHANGELOG_INTERFACES:
            with self.subTest(label=label):
                tree = self.tree()
                tree.replace('CHANGELOG.md', label, label.strip('*'), count=0)
                self.assertFails(tree.errors(), 'changelog', label)

    def test_no_version_heading_is_not_found(self):
        tree = self.tree()
        tree.sub('CHANGELOG.md', r'## \[0\.1\.0\]', '## Version 0.1.0')
        self.assertFails(tree.errors(), 'changelog', NOT_FOUND)

    SENTENCE = 'It is not yet tagged `rust-v0.1.0`; the date above is set when it is tagged.'

    def test_the_heading_carries_the_status_and_the_checkout_body_is_clean(self):
        # `unreleased` until the release commit sets the tagging date.
        self.assertRegex(CHANGELOG_HEADING, r'^## \[0\.1\.0\] - (unreleased|\d{4}-\d{2}-\d{2})$')
        self.assertNoRule(self.tree().errors(), 'changelog')

    def test_the_release_body_must_not_say_it_is_not_yet_tagged(self):
        tree = self.tree()
        tree.replace('CHANGELOG.md', 'Release order:', self.SENTENCE + '\n\nRelease order:')
        self.assertFails(tree.errors(), 'changelog', 'release body', "'not yet tagged'")

    def test_the_sentence_wrapped_across_lines_is_still_found_with_its_line(self):
        tree = self.tree()
        tree.replace('CHANGELOG.md', 'Release order:', 'It is not yet\ntagged.\n\nRelease order:')
        line = tree.read('CHANGELOG.md').splitlines().index('It is not yet') + 1
        self.assertFails(tree.errors(), 'changelog', f'CHANGELOG.md:{line}', "'not yet tagged'")

    def test_the_release_body_must_not_say_unreleased(self):
        tree = self.tree()
        tree.replace('CHANGELOG.md', 'Release order:', 'This Unreleased version follows.\n\nRelease order:')
        self.assertFails(tree.errors(), 'changelog', 'release body', "'Unreleased'")

    def test_the_release_body_must_not_claim_a_publication_order(self):
        tree = self.tree()
        tree.replace('CHANGELOG.md', 'Release order:', 'The crates are published in this order:')
        self.assertFails(tree.errors(), 'changelog', 'release body', "'published in this order'")

    def test_another_section_may_say_unreleased(self):
        tree = self.tree()
        tree.replace('CHANGELOG.md', 'No changes yet.', 'Unreleased changes are not yet tagged.')
        self.assertNoRule(tree.errors(), 'changelog')

    def test_a_missing_unreleased_heading_still_needs_the_version(self):
        tree = self.tree()
        tree.sub('CHANGELOG.md', r'## \[Unreleased\]\n\nNo changes yet\.\n\n', '')
        self.assertNoRule(tree.errors(), 'changelog')


class SecurityPolicy(Case):
    def test_a_version_bump_without_a_table_row_fails(self):
        tree = self.tree()
        tree.replace('Cargo.toml', 'version = "0.1.0"', 'version = "0.2.0"', count=0)
        self.assertFails(tree.errors(), 'security-policy', '0.2.x')

    def test_a_missing_table_row_is_not_found(self):
        tree = self.tree()
        tree.sub('SECURITY.md', r'\| 0\.1\.x \|[^\n]*\n', '')
        self.assertFails(tree.errors(), 'security-policy', NOT_FOUND, '0.1.x')

    def test_a_row_for_another_minor_does_not_count(self):
        tree = self.tree()
        tree.replace('SECURITY.md', '| 0.1.x |', '| 0.2.x |')
        self.assertFails(tree.errors(), 'security-policy', '0.1.x')

    def test_a_missing_email_channel_fails(self):
        tree = self.tree()
        tree.replace('SECURITY.md', 'security@blackcatinformatics.ca', 'the maintainers', count=0)
        self.assertFails(tree.errors(), 'security-policy', 'security@blackcatinformatics.ca')

    def test_a_missing_private_reporting_channel_fails(self):
        tree = self.tree()
        tree.replace('SECURITY.md', '/security/advisories/new', '/security', count=0)
        self.assertFails(tree.errors(), 'security-policy', '/security/advisories/new')


class ReleaseReservations(Case):
    def test_a_missing_crate_row_fails(self):
        tree = self.tree()
        tree.sub('docs/RELEASE.md', r'\| `purrassist-derive` \|[^\n]*\n', '')
        self.assertFails(tree.errors(), 'release-reservations', 'purrassist-derive')

    def test_an_unknown_crate_row_fails(self):
        tree = self.tree()
        tree.replace('docs/RELEASE.md', '| `purrassist-cli` | `0.0.0`', '| `purrassist-extra` | `0.0.0`')
        self.assertFails(tree.errors(), 'release-reservations', 'purrassist-extra')

    def test_a_reordered_table_fails(self):
        tree = self.tree()
        text = tree.read('docs/RELEASE.md')
        core = re.search(r'\| `purrassist-core` \|[^\n]*\n', text).group(0)
        derive = re.search(r'\| `purrassist-derive` \|[^\n]*\n', text).group(0)
        tree.write('docs/RELEASE.md', text.replace(core + derive, derive + core))
        self.assertFails(tree.errors(), 'release-reservations', 'RELEASE_ORDER')

    def test_a_missing_heading_is_not_found(self):
        tree = self.tree()
        tree.replace('docs/RELEASE.md', '## Name reservations', '## Placeholders')
        self.assertFails(tree.errors(), 'release-reservations', NOT_FOUND)

    def test_a_section_without_rows_is_not_found(self):
        tree = self.tree()
        tree.sub('docs/RELEASE.md', r'\| `purrassist[^\n]*\| `0\.0\.0`[^\n]*\n', '', count=0)
        self.assertFails(tree.errors(), 'release-reservations', NOT_FOUND)


class ReleaseQualificationSteps(Case):
    def test_the_steps_are_read_from_the_workflow(self):
        names = CD.workflow_step_names((ROOT / '.github/workflows/release-qualification.yml').read_text(encoding='utf-8'))
        self.assertIn('Verify the release gate', names)
        self.assertNotIn('Release qualification', names)

    def test_only_job_steps_are_read(self):
        self.assertEqual(CD.workflow_step_names(
            "name: Top\non: push\njobs:\n  a:\n    name: Job\n    steps:\n"
            "      - uses: x\n      - name: 'One'\n        run: y\n      - name: Two # c\n"), ['One', 'Two # c'])

    WORKFLOW = '.github/workflows/release-qualification.yml'
    UNNAMED = 'a step that runs a command needs a `name:`'

    def test_the_real_workflow_keeps_its_unnamed_checkout_and_passes(self):
        steps = CD.workflow_steps((ROOT / self.WORKFLOW).read_text(encoding='utf-8'))
        self.assertEqual(steps[0][1:], (None, False))
        self.assertTrue(all(name is not None for _, name, runs in steps if runs))
        self.assertNoRule(self.tree().errors(), 'release-qualification-steps')

    def test_a_step_that_runs_without_a_name_fails(self):
        tree = self.tree()
        tree.detach(self.WORKFLOW)
        tree.append(self.WORKFLOW, '      - run: make wasm-test\n')
        line = tree.read(self.WORKFLOW).count('\n')
        self.assertFails(tree.errors(), 'release-qualification-steps', f'{self.WORKFLOW}:{line}', self.UNNAMED)

    def test_an_unnamed_uses_step_is_not_a_qualification_step(self):
        tree = self.tree()
        tree.detach(self.WORKFLOW)
        tree.append(self.WORKFLOW, '      - uses: actions/cache@abc\n        with:\n          path: x\n')
        self.assertNoRule(tree.errors(), 'release-qualification-steps')

    def test_a_name_after_the_run_names_that_step(self):
        text = 'jobs:\n  a:\n    steps:\n      - run: x\n        name: Late\n      - uses: y\n'
        self.assertEqual(CD.workflow_steps(text), [(4, 'Late', True), (6, None, False)])
        tree = self.tree()
        tree.detach(self.WORKFLOW)
        tree.append(self.WORKFLOW, '      - run: make wasm-test\n        name: Verify wasm\n')
        errors = tree.errors()
        self.assertFails(errors, 'release-qualification-steps', 'Verify wasm')
        self.assertFalse([error for error in errors if self.UNNAMED in error], errors)

    def test_a_run_body_line_that_starts_with_a_dash_is_not_a_step(self):
        text = ('jobs:\n  a:\n    steps:\n      - name: One\n        run: |\n          - name: Fake\n'
                '          - run: nothing\n        shell: bash\n      - name: Two\n')
        self.assertEqual(CD.workflow_steps(text), [(4, 'One', True), (9, 'Two', False)])
        tree = self.tree()
        tree.detach(self.WORKFLOW)
        tree.append(self.WORKFLOW, '      - name: Verify body\n        run: |\n          - name: Fake\n          - run: x\n')
        errors = tree.errors()
        self.assertFails(errors, 'release-qualification-steps', 'Verify body')
        self.assertFalse([error for error in errors if 'Fake' in error or self.UNNAMED in error], errors)

    def test_items_outside_steps_are_not_steps(self):
        text = ('jobs:\n  a:\n    strategy:\n      matrix:\n        include:\n          - name: windows\n'
                '            os: windows-latest\n          - run: not-a-step\n    steps:\n      - name: One\n'
                '        run: y\n  b:\n    steps: []\n    needs: [a]\n')
        self.assertEqual(CD.workflow_steps(text), [(10, 'One', True)])

    def test_steps_of_each_job_are_read_and_a_comment_does_not_end_a_block(self):
        text = ('jobs:\n  a:\n    steps:\n      - name: One\n        run: y\n# comment\n'
                '  b:\n    steps:\n      # note\n      - name: Two\n        run: z\n')
        self.assertEqual(CD.workflow_step_names(text), ['One', 'Two'])

    def test_every_step_must_be_named(self):
        names = CD.workflow_step_names((ROOT / '.github/workflows/release-qualification.yml').read_text(encoding='utf-8'))
        for step in names:
            with self.subTest(step=step):
                tree = self.tree()
                tree.replace('docs/RELEASE.md', f'`{step}`', step, count=0)
                self.assertFails(tree.errors(), 'release-qualification-steps', f'`{step}`')

    def test_a_new_workflow_step_fails(self):
        tree = self.tree()
        tree.detach('.github/workflows/release-qualification.yml')
        tree.append('.github/workflows/release-qualification.yml', '      - name: Verify something new\n        run: true\n')
        self.assertFails(tree.errors(), 'release-qualification-steps', 'Verify something new')

    def test_a_workflow_without_steps_fails(self):
        tree = self.tree()
        tree.detach('.github/workflows/release-qualification.yml')
        tree.write('.github/workflows/release-qualification.yml', 'jobs:\n  a:\n    steps: []\n')
        self.assertFails(tree.errors(), 'release-qualification-steps', 'no named step')

    def test_a_missing_heading_is_not_found(self):
        tree = self.tree()
        tree.replace('docs/RELEASE.md', '## Qualification workflow (release-qualification.yml)', '## Qualification')
        self.assertFails(tree.errors(), 'release-qualification-steps', NOT_FOUND)


class ReadmeChangelog(Case):
    def test_a_missing_link_fails(self):
        tree = self.tree()
        tree.replace('README.md', '[changelog](CHANGELOG.md)', 'changelog')
        self.assertFails(tree.errors(), 'readme-changelog', NOT_FOUND)


class FuzzRows(Case):
    def test_a_row_without_the_link_fails(self):
        tree = self.tree()
        tree.replace('docs/QUALIFICATION.md', '`make fuzz` (see [Known limitations](#known-limitations)) |', '`make fuzz` |')
        self.assertFails(tree.errors(), 'fuzz-row-limits', 'must link to #known-limitations')

    def test_a_row_without_the_stop_fails(self):
        tree = self.tree()
        tree.sub('docs/QUALIFICATION.md', r'\. The default `make fuzz` stops on the #11 and #6 signatures, so it is not a passing result for the two tool-call targets', '')
        self.assertFails(tree.errors(), 'fuzz-row-limits', 'stops on the #N and #N signatures')

    def test_the_tool_call_row_must_also_state_the_stop(self):
        tree = self.tree()
        tree.sub('docs/QUALIFICATION.md', r'but the default `make fuzz` stops on the #11 and #6 signatures and has no', 'and has no')
        self.assertFails(tree.errors(), 'fuzz-row-limits', 'stops on the #N and #N signatures')

    def test_no_make_fuzz_row_is_not_found(self):
        tree = self.tree()
        text = '\n'.join(line.replace('`make fuzz`', '`make  fuzz`') if line.startswith('|') else line
                         for line in tree.read('docs/QUALIFICATION.md').splitlines()) + '\n'
        tree.write('docs/QUALIFICATION.md', text)
        self.assertFails(tree.errors(), 'fuzz-row-limits', NOT_FOUND)


class BaselineTable(Case):
    def test_a_missing_target_row_fails(self):
        tree = self.tree()
        tree.sub('docs/FUZZING.md', r'\| tool-call-ref \|[^\n]*\n', '')
        self.assertFails(tree.errors(), 'fuzz-baseline-table', 'differ from TARGETS')

    def test_an_extra_row_fails(self):
        tree = self.tree()
        tree.replace('docs/FUZZING.md', '| tool-call-ref | none', '| other | none | none | none | none | none | none |\n| tool-call-ref | none')
        self.assertFails(tree.errors(), 'fuzz-baseline-table', 'differ from TARGETS')

    def test_a_target_without_a_baseline_must_say_none(self):
        tree = self.tree()
        tree.replace('docs/FUZZING.md', '| tool-call | none | none | none | none | none | none |',
                     '| tool-call | 10,000 | none | none | none | none | none |')
        self.assertFails(tree.errors(), 'fuzz-baseline-table', 'tool-call has no baseline report', 'none')

    def test_a_target_with_a_baseline_must_link_it(self):
        tree = self.tree()
        tree.replace('docs/FUZZING.md', '| [json](qualification/fuzz-json-linux-x86_64.json) |', '| json |')
        self.assertFails(tree.errors(), 'fuzz-baseline-table', 'json has a baseline report')

    def test_a_target_with_a_baseline_must_not_say_none(self):
        tree = self.tree()
        tree.replace('docs/FUZZING.md', '| 10,021 | 3,799 | 86 | 0 | 0 | 0 |', '| none | 3,799 | 86 | 0 | 0 | 0 |')
        self.assertFails(tree.errors(), 'fuzz-baseline-table', 'json has a baseline report but its row says none')

    def test_a_new_baseline_report_requires_a_linked_row(self):
        tree = self.tree()
        shutil.copy(tree.root / 'docs/qualification/fuzz-json-linux-x86_64.json',
                    tree.root / 'docs/qualification/fuzz-tool-call-linux-x86_64.json')
        self.assertFails(tree.errors(), 'fuzz-baseline-table', 'tool-call has a baseline report')

    def test_a_renamed_heading_is_not_found(self):
        tree = self.tree()
        tree.replace('docs/FUZZING.md', '### Recorded 10,000-mutation baselines', '### Baselines')
        self.assertFails(tree.errors(), 'fuzz-baseline-table', NOT_FOUND)


class ArchivedEvidence(Case):
    def test_a_paragraph_without_the_commit_fails(self):
        tree = self.tree()
        tree.sub('docs/QUALIFICATION.md', r'This section records the local integration at `66b3756`, which', 'This section records the local integration, which')
        tree.sub('docs/QUALIFICATION.md', r'`66b3756` by this record only', 'this record only')
        self.assertFails(tree.errors(), 'archived-evidence', 'does not name commit 66b3756776a0')

    def test_a_rebound_report_fails(self):
        tree = self.tree()
        path = 'docs/qualification/resources-stage2-linux-x86_64.json'
        tree.replace(path, '66b3756776a072af1889596ae98a5faa5a50cea6', 'ba2a35cc3ab655cf377fade556d3e0c066cd633c')
        self.assertFails(tree.errors(), 'archived-evidence', 'ba2a35cc3ab6')

    def test_a_report_without_a_commit_must_say_so(self):
        tree = self.tree()
        tree.replace('docs/QUALIFICATION.md', 'records no source commit', 'was measured')
        self.assertFails(tree.errors(), 'archived-evidence', 'recovery-bench-stage2', 'records no source commit')

    def test_no_archived_evidence_paragraph_is_not_found(self):
        tree = self.tree()
        tree.sub('docs/QUALIFICATION.md', r'\]\(qualification/', '](qualifications/', count=0)
        self.assertFails(tree.errors(), 'archived-evidence', NOT_FOUND)

    def test_a_missing_report_fails(self):
        tree = self.tree()
        (tree.root / 'docs/qualification/resources-stage2-linux-x86_64.json').unlink()
        self.assertFails(tree.errors(), 'archived-evidence', 'unreadable')


class Stage2Heading(Case):
    def test_an_unlabelled_heading_fails(self):
        tree = self.tree()
        tree.replace('docs/QUALIFICATION.md', '## Historical: Stage 2 record bound to `66b3756`', '## Stage 2 local integration record')
        self.assertFails(tree.errors(), 'stage2-heading', 'Historical')

    def test_a_heading_bound_to_another_commit_fails(self):
        tree = self.tree()
        tree.replace('docs/QUALIFICATION.md', '## Historical: Stage 2 record bound to `66b3756`', '## Historical: Stage 2 record bound to `699d9ff`')
        self.assertFails(tree.errors(), 'stage2-heading', '699d9ff')

    def test_a_missing_stage2_section_is_not_found(self):
        tree = self.tree()
        tree.replace('docs/QUALIFICATION.md', '## Historical: Stage 2 record bound to `66b3756`', '## Archived record')
        self.assertFails(tree.errors(), 'stage2-heading', NOT_FOUND)


class WindowsRun(Case):
    def test_a_claim_without_a_run_fails(self):
        tree = self.tree()
        tree.sub('docs/QUALIFICATION.md', r'\[Verify run 36702475819\]\([^)]*\)', 'a hosted run')
        self.assertFails(tree.errors(), 'windows-run', 'Verify run N')

    def test_a_label_that_differs_from_the_link_fails(self):
        tree = self.tree()
        tree.replace('docs/QUALIFICATION.md', '[Verify run 36702475819]', '[Verify run 36702475818]')
        self.assertFails(tree.errors(), 'windows-run', 'matching actions/runs/N')

    def test_a_claim_without_the_commit_fails(self):
        tree = self.tree()
        tree.replace('docs/QUALIFICATION.md', 'dc23bc81bebacef95f2b84d29dfd20031fdcd44c', 'dc23bc8')
        self.assertFails(tree.errors(), 'windows-run', 'full commit')

    def test_no_windows_sentence_is_not_found(self):
        tree = self.tree()
        tree.replace('docs/QUALIFICATION.md', 'windows-engines', 'windows', count=0)
        self.assertFails(tree.errors(), 'windows-run', NOT_FOUND)


class ToolHeadings(Case):
    def test_a_tool_without_a_heading_fails(self):
        tree = self.tree()
        tree.replace('docs/TOOLS.md', '### `match_paths`', '#### `match_paths`')
        self.assertFails(tree.errors(), 'tools-headings', "'match_paths'", 'found 0')

    def test_a_duplicate_heading_fails(self):
        tree = self.tree()
        tree.append('docs/TOOLS.md', '\n### `read_file`\n\nAgain.\n')
        self.assertFails(tree.errors(), 'tools-headings', "'read_file'", 'found 2')

    def test_an_unknown_heading_fails(self):
        tree = self.tree()
        tree.append('docs/TOOLS.md', '\n### `read_files`\n\nA name that is not a tool.\n')
        self.assertFails(tree.errors(), 'tools-headings', "'read_files'", 'names no tool')

    def test_a_tool_added_to_the_block_needs_a_heading(self):
        tree = self.tree()
        tree.replace('docs/TOOLS.md', '<!-- tools-reference:end -->', '- **`new_tool`**: x\n<!-- tools-reference:end -->')
        self.assertFails(tree.errors(), 'tools-headings', "'new_tool'", 'found 0')

    def test_headings_inside_a_code_fence_do_not_count(self):
        tree = self.tree()
        tree.append('docs/TOOLS.md', '\n```\n### `not_a_tool`\n```\n')
        self.assertNoRule(tree.errors(), 'tools-headings')

    def test_a_missing_marker_fails(self):
        tree = self.tree()
        tree.replace('docs/TOOLS.md', '<!-- tools-reference:begin -->', '<!-- tools-reference:start -->')
        self.assertFails(tree.errors(), 'tools-headings', 'tools-reference:begin')

    def test_an_empty_block_is_not_found(self):
        tree = self.tree()
        tree.sub('docs/TOOLS.md', r'(?m)^- \*\*`', '- **no tool', count=0)
        self.assertFails(tree.errors(), 'tools-headings', NOT_FOUND)


class CrateMap(Case):
    def test_a_reordered_map_fails(self):
        tree = self.tree()
        rows = re.findall(r'^\| `purrassist-(?:syntax|derive)` \|[^\n]*\n', tree.read('README.md'), re.MULTILINE)
        tree.replace('README.md', rows[0] + rows[1], rows[1] + rows[0])
        self.assertFails(tree.errors(), 'crate-map', 'RELEASE_ORDER')

    def test_a_missing_crate_fails(self):
        tree = self.tree()
        tree.sub('README.md', r'(?m)^\| `purrassist-host` \|[^\n]*\n', '')
        self.assertFails(tree.errors(), 'crate-map', 'RELEASE_ORDER', 'purrassist-host')

    def test_an_extra_crate_fails(self):
        tree = self.tree()
        tree.replace('README.md', '| `purrassist-cli` |', '| `purrassist-extra` | x |\n| `purrassist-cli` |')
        self.assertFails(tree.errors(), 'crate-map', 'purrassist-extra')

    def test_an_internal_crate_must_be_named(self):
        tree = self.tree()
        tree.replace('README.md', '`wasm-harness`', 'the wasm harness')
        self.assertFails(tree.errors(), 'crate-map', "'wasm-harness'", 'internal')

    def test_the_order_comes_from_check_release(self):
        tree = self.tree()
        tree.replace('scripts/check-release.py', "    'purrassist-core',\n", "    'purrassist-core',\n    'purrassist-extra',\n")
        self.assertFails(tree.errors(), 'crate-map', 'purrassist-extra')

    def test_no_crate_map_is_not_found(self):
        tree = self.tree()
        tree.replace('README.md', '## Crate map', '## Crates')
        self.assertFails(tree.errors(), 'crate-map', NOT_FOUND)


class ThreeInterfaces(Case):
    def test_a_missing_interface_heading_fails(self):
        tree = self.tree()
        tree.replace('README.md', '### MCP server', '### Servers')
        self.assertFails(tree.errors(), 'three-interfaces', 'headings')

    def test_headings_out_of_order_fail(self):
        tree = self.tree()
        tree.replace('README.md', '### CLI', '### Rust crate again')
        self.assertFails(tree.errors(), 'three-interfaces', 'headings')

    def test_a_missing_marker_fails(self):
        tree = self.tree()
        tree.replace('README.md', '<!-- three-interfaces:end -->', '')
        self.assertFails(tree.errors(), 'three-interfaces', 'three-interfaces:end')

    def test_each_command_must_be_shown(self):
        for command in ('cargo add purrassist', 'cargo install purrassist-cli', 'purrassist mcp --root /path',
                        'purrassist decode', 'purrassist call'):
            with self.subTest(command=command):
                tree = self.tree()
                tree.replace('README.md', command, 'something else', count=0)
                self.assertFails(tree.errors(), 'three-interfaces', NOT_FOUND)

    def test_the_journal_directory_must_be_mentioned(self):
        tree = self.tree()
        tree.replace('README.md', '`.purrassist/`', 'a directory', count=0)
        self.assertFails(tree.errors(), 'three-interfaces', NOT_FOUND, '.purrassist/')

    def test_the_equivalence_sentence_must_name_its_test(self):
        tree = self.tree()
        tree.replace('README.md', '`every_registry_entry_has_a_fixture_validated_through_cli_and_mcp`', 'a test')
        self.assertFails(tree.errors(), 'three-interfaces', NOT_FOUND, 'every_registry_entry')

    def test_the_named_test_must_exist(self):
        tree = self.tree()
        tree.detach('crates/cli/tests/protocol.rs')
        tree.replace('crates/cli/tests/protocol.rs', 'fn every_registry_entry_has_a_fixture_validated_through_cli_and_mcp',
                     'fn renamed_test')
        self.assertFails(tree.errors(), 'three-interfaces', 'does not exist')


class McpConfig(Case):
    def with_config(self, config):
        tree = self.tree()
        tree.sub('README.md', r'```json\n.*?\n```', '```json\n' + config + '\n```')
        return tree

    def test_the_checkout_config_passes(self):
        self.assertNoRule(self.tree().errors(), 'mcp-config')

    def test_invalid_json_fails(self):
        self.assertFails(self.with_config('{"mcpServers": ').errors(), 'mcp-config', 'not valid JSON')

    def test_a_wrong_command_fails(self):
        config = '{"mcpServers": {"p": {"command": "purr", "args": ["mcp", "--root", "/w"]}}}'
        self.assertFails(self.with_config(config).errors(), 'mcp-config', "'purr'")

    def test_wrong_args_fail(self):
        for args in ('["mcp"]', '["--root", "/w", "mcp"]', '["mcp", "--root"]', '["mcp", "--root", ""]',
                     '["mcp", "--root", "/w", "--extra"]', '"mcp --root /w"'):
            with self.subTest(args=args):
                config = '{"mcpServers": {"p": {"command": "purrassist", "args": %s}}}' % args
                self.assertFails(self.with_config(config).errors(), 'mcp-config', 'args')

    def test_no_server_fails(self):
        self.assertFails(self.with_config('{"mcpServers": {}}').errors(), 'mcp-config', 'exactly one server')

    def test_no_configuration_fails(self):
        tree = self.tree()
        tree.sub('README.md', r'```json\n.*?\n```', '```text\nnone\n```')
        self.assertFails(tree.errors(), 'mcp-config', 'found 0')


class PackagedReadmes(Case):
    def test_the_packaged_readmes_come_from_cargo_metadata(self):
        packages = CD.publishable(CD.workspace_packages(ROOT))
        self.assertEqual([package['name'] for package in packages], list(CD.load_module(
            ROOT / 'scripts/check-release.py', 'purrassist_test_release').RELEASE_ORDER))
        self.assertEqual(sorted(package['readme'] for package in packages),
                         sorted(f'crates/{name}/README.md' for name in
                                ('core', 'derive', 'syntax', 'purrassist', 'host', 'cli')))

    def test_a_banned_phrase_in_a_packaged_readme_fails(self):
        tree = self.tree()
        tree.append('crates/host/README.md', '\nThis crate is on the issue branch.\n')
        self.assertFails(tree.errors(), 'packaged-readmes', 'crates/host/README.md', 'issue branch')

    def test_a_banned_phrase_wrapped_across_lines_fails(self):
        tree = self.tree()
        tree.append('crates/core/README.md', '\nNot yet\ndone.\n')
        self.assertFails(tree.errors(), 'packaged-readmes', 'crates/core/README.md')

    def test_a_latest_link_fails(self):
        tree = self.tree()
        tree.append('crates/derive/README.md', '\nSee https://docs.rs/purrassist-core/latest/purrassist_core/.\n')
        self.assertFails(tree.errors(), 'packaged-readmes', 'crates/derive/README.md', 'latest')

    def test_a_pinned_docs_rs_link_passes(self):
        tree = self.tree()
        tree.append('crates/derive/README.md', '\nSee https://docs.rs/purrassist-core/0.1.0/purrassist_core/.\n')
        self.assertNoRule(tree.errors(), 'packaged-readmes')

    def test_a_missing_readme_fails(self):
        tree = self.tree()
        tree.drop('crates/syntax/README.md')
        self.assertFails(tree.errors(), 'packaged-readmes', 'crates/syntax/README.md', 'missing')

    def test_a_package_that_becomes_publishable_is_found_by_metadata(self):
        tree = self.tree()
        tree.detach('crates/helper-census/Cargo.toml')
        tree.replace('crates/helper-census/Cargo.toml', 'publish = false\n', '')
        self.assertFails(tree.errors(), 'packaged-readmes', 'purrassist-helper-census', 'RELEASE_ORDER')


class PinnedLinks(Case):
    BASE = 'https://github.com/Blackcat-Informatics/purrassist'

    def test_the_checkout_passes(self):
        self.assertNoRule(self.tree().errors(), 'pinned-links')

    def test_a_pinned_link_to_an_existing_path_and_anchor_passes(self):
        tree = self.tree()
        tree.append('crates/host/README.md', f'\nSee {self.BASE}/blob/rust-v0.1.0/docs/QUALIFICATION.md#known-limitations,'
                                             f' {self.BASE}/tree/rust-v0.1.0/docs and {self.BASE}/issues/6 and {self.BASE}.\n')
        self.assertNoRule(tree.errors(), 'pinned-links')

    def test_a_main_link_in_a_crate_readme_fails(self):
        tree = self.tree()
        tree.append('crates/core/README.md', f'\nSee [x]({self.BASE}/blob/main/docs/QUALIFICATION.md).\n')
        self.assertFails(tree.errors(), 'pinned-links', 'crates/core/README.md', "'main'", 'rust-v0.1.0')

    def test_another_tag_fails(self):
        tree = self.tree()
        tree.append('crates/core/README.md', f'\nSee {self.BASE}/blob/rust-v0.0.9/docs/QUALIFICATION.md.\n')
        self.assertFails(tree.errors(), 'pinned-links', 'crates/core/README.md', "'rust-v0.0.9'")

    def test_a_pinned_link_to_a_missing_path_fails(self):
        tree = self.tree()
        tree.append('crates/syntax/README.md', f'\nSee {self.BASE}/blob/rust-v0.1.0/docs/NOPE.md.\n')
        self.assertFails(tree.errors(), 'pinned-links', 'crates/syntax/README.md', 'docs/NOPE.md', 'does not exist')

    def test_a_pinned_link_to_a_missing_anchor_fails(self):
        tree = self.tree()
        tree.append('crates/cli/README.md', f'\nSee {self.BASE}/blob/rust-v0.1.0/docs/QUALIFICATION.md#no-such-heading.\n')
        self.assertFails(tree.errors(), 'pinned-links', 'crates/cli/README.md', 'heading that does not exist')

    def test_a_main_documentation_link_fails(self):
        tree = self.tree()
        tree.detach('crates/cli/Cargo.toml')
        tree.replace('crates/cli/Cargo.toml', '/blob/rust-v0.1.0/docs/TOOLS.md', '/blob/main/docs/TOOLS.md')
        self.assertFails(tree.errors(), 'pinned-links', 'crates/cli/Cargo.toml', "'main'")

    def test_a_pinned_documentation_link_to_a_missing_path_fails(self):
        tree = self.tree()
        tree.detach('crates/cli/Cargo.toml')
        tree.replace('crates/cli/Cargo.toml', 'docs/TOOLS.md"', 'docs/NOPE.md"')
        self.assertFails(tree.errors(), 'pinned-links', 'crates/cli/Cargo.toml', 'docs/NOPE.md')

    def test_the_tag_follows_the_workspace_version(self):
        tree = self.tree()
        tree.replace('Cargo.toml', 'version = "0.1.0"', 'version = "0.2.0"', count=0)
        self.assertFails(tree.errors(), 'pinned-links', 'crates/cli/README.md', "'rust-v0.1.0'", 'rust-v0.2.0')

    def test_the_repository_owner_and_name_match_in_any_case(self):
        tree = self.tree()
        tree.append('crates/core/README.md',
                    '\nSee https://github.com/blackcat-informatics/PurRAssist/blob/main/docs/QUALIFICATION.md.\n')
        self.assertFails(tree.errors(), 'pinned-links', 'crates/core/README.md', "'main'")
        tree = self.tree()
        tree.append('crates/core/README.md',
                    '\nSee https://github.com/blackcat-informatics/PurRAssist/blob/rust-v0.1.0/docs/QUALIFICATION.md.\n')
        self.assertNoRule(tree.errors(), 'pinned-links')

    def test_the_root_readme_may_link_main(self):
        tree = self.tree()
        tree.append('README.md', f'\nSee {self.BASE}/blob/main/docs/QUALIFICATION.md.\n')
        self.assertNoRule(tree.errors(), 'pinned-links')


class WorkspaceBoundaries(Case):
    def test_a_missing_member_fails(self):
        tree = self.tree()
        tree.sub('docs/IMPLEMENTATION.md', r'- `purrassist-syntax`: [^\n]*\n', '')
        self.assertFails(tree.errors(), 'workspace-boundaries', 'purrassist-syntax')

    def test_an_unknown_crate_fails(self):
        tree = self.tree()
        tree.replace('docs/IMPLEMENTATION.md', '- `purrassist-cli`:', '- `purrassist-extra`: extra.\n- `purrassist-cli`:')
        self.assertFails(tree.errors(), 'workspace-boundaries', 'purrassist-extra')

    def test_an_unpublished_member_must_say_internal(self):
        tree = self.tree()
        tree.replace('docs/IMPLEMENTATION.md', '`wasm-harness`: internal, unpublished', '`wasm-harness`: unpublished')
        self.assertFails(tree.errors(), 'workspace-boundaries', 'wasm-harness', 'internal')

    def test_a_new_workspace_member_fails(self):
        tree = self.tree()
        (tree.root / 'crates/extra/src').mkdir(parents=True)
        tree.write('crates/extra/Cargo.toml', '[package]\nname = "purrassist-extra"\nversion = "0.1.0"\nedition = "2024"\npublish = false\n')
        tree.write('crates/extra/src/lib.rs', '')
        tree.replace('Cargo.toml', '"crates/helper-census"]', '"crates/helper-census", "crates/extra"]')
        self.assertFails(tree.errors(), 'workspace-boundaries', 'purrassist-extra')

    def test_no_boundaries_section_is_not_found(self):
        tree = self.tree()
        tree.replace('docs/IMPLEMENTATION.md', '## Workspace boundaries', '## Crates')
        self.assertFails(tree.errors(), 'workspace-boundaries', NOT_FOUND)


class RegistryName(Case):
    TOKEN = '`mcp-name: io.github.Blackcat-Informatics/purrassist`'

    def test_the_checkout_passes(self):
        errors = self.tree().errors()
        self.assertNoRule(errors, 'registry-name')
        self.assertNoRule(errors, 'cli-documentation')

    def test_a_missing_token_fails(self):
        tree = self.tree()
        tree.sub('crates/cli/README.md', r'- MCP Registry name:[^\n]*\n?', '')
        self.assertFails(tree.errors(), 'registry-name', 'crates/cli/README.md', 'visible')

    def test_a_commented_out_token_fails(self):
        tree = self.tree()
        tree.replace('crates/cli/README.md', '- MCP Registry name: ' + self.TOKEN,
                     '<!-- - MCP Registry name: ' + self.TOKEN + ' -->')
        self.assertFails(tree.errors(), 'registry-name', 'visible')

    def test_a_fenced_token_fails(self):
        tree = self.tree()
        tree.replace('crates/cli/README.md', '- MCP Registry name: ' + self.TOKEN,
                     '```\n- MCP Registry name: ' + self.TOKEN + '\n```')
        self.assertFails(tree.errors(), 'registry-name', 'visible')

    def test_a_token_outside_the_links_section_fails(self):
        tree = self.tree()
        tree.replace('crates/cli/README.md', '## Links', '## References')
        self.assertFails(tree.errors(), 'registry-name', 'visible')

    def test_the_owner_is_case_sensitive(self):
        tree = self.tree()
        tree.replace('crates/cli/README.md', 'io.github.Blackcat-Informatics/', 'io.github.blackcat-informatics/')
        self.assertFails(tree.errors(), 'registry-name', 'io.github.Blackcat-Informatics/purrassist')

    def test_the_owner_comes_from_the_repository_url(self):
        tree = self.tree()
        tree.replace('Cargo.toml', 'repository = "https://github.com/Blackcat-Informatics/purrassist"',
                     'repository = "https://github.com/Another-Owner/purrassist"')
        self.assertFails(tree.errors(), 'registry-name', 'io.github.Another-Owner/purrassist')
        tree.replace('crates/cli/README.md', 'io.github.Blackcat-Informatics/', 'io.github.Another-Owner/')
        self.assertNoRule(tree.errors(), 'registry-name')

    def test_a_docs_rs_documentation_link_fails(self):
        tree = self.tree()
        tree.detach('crates/cli/Cargo.toml')
        tree.sub('crates/cli/Cargo.toml', r'documentation = "[^"]*"', 'documentation = "https://docs.rs/purrassist-cli"')
        self.assertFails(tree.errors(), 'cli-documentation', 'docs.rs')

    def test_a_missing_documentation_link_fails(self):
        tree = self.tree()
        tree.detach('crates/cli/Cargo.toml')
        tree.sub('crates/cli/Cargo.toml', r'documentation = "[^"]*"\n', '')
        self.assertFails(tree.errors(), 'cli-documentation')


class DocumentHelpers(Case):
    def test_strip_hidden_blanks_comments_and_fences_only(self):
        text = 'a\n<!-- b\nc -->\n```\nd\n```\ne\n'
        stripped = CD.strip_hidden(text)
        self.assertEqual(stripped.count('\n'), text.count('\n'))
        self.assertEqual(stripped.split(), ['a', 'e'])

    def test_marked_block_needs_each_marker_once_in_order(self):
        self.assertEqual(CD.marked_block('x\n<b>\ny\n<e>\n', '<b>\n', '<e>\n'), 'y\n')
        self.assertIsNone(CD.marked_block('<e>\n<b>\n', '<b>\n', '<e>\n'))
        self.assertIsNone(CD.marked_block('<b>\n<b>\n<e>\n', '<b>\n', '<e>\n'))

    def test_tool_names_are_read_from_list_items(self):
        self.assertEqual(CD.tool_names('- **`a_b`**: x\n  - **`no`**\n- **`c`**: y\n'), ['a_b', 'c'])


class SocialPreview(Case):
    RULE = 'social-preview'

    def tree(self):
        tree = Tree(self)
        for name in ('docs/assets.json', 'docs/social-preview.svg', 'docs/social-preview.png'):
            tree.detach(name)
        return tree

    def registry(self, tree, mutate):
        data = json.loads(tree.read('docs/assets.json'))
        mutate(data['assets'][0])
        tree.write('docs/assets.json', json.dumps(data))

    def png(self, tree, data):
        (tree.root / 'docs/social-preview.png').write_bytes(data)

    def test_the_committed_asset_passes(self):
        self.assertNoRule(self.tree().errors(), self.RULE)

    def test_wrong_dimensions_fail(self):
        tree = self.tree()
        png = bytearray((tree.root / 'docs/social-preview.png').read_bytes())
        png[16:24] = (1200).to_bytes(4, 'big') + (630).to_bytes(4, 'big')
        self.png(tree, bytes(png))
        self.assertFails(tree.errors(), self.RULE, 'IHDR is (1200, 630)')

    def test_a_non_png_fails(self):
        tree = self.tree()
        self.png(tree, b'not a png at all, but long enough to hold a header')
        self.assertFails(tree.errors(), self.RULE, 'IHDR is None')

    def test_an_oversize_png_fails(self):
        tree = self.tree()
        png = (tree.root / 'docs/social-preview.png').read_bytes()
        self.png(tree, png + b'\0' * CD.SOCIAL_MAX_BYTES)
        self.assertFails(tree.errors(), self.RULE, 'is not under 1000000')

    def test_a_changed_svg_fails_the_registry_digest(self):
        tree = self.tree()
        tree.append('docs/social-preview.svg', '<!-- changed -->\n')
        self.assertFails(tree.errors(), self.RULE, 'docs/social-preview.svg', 'sha256 differs')

    def test_a_changed_png_fails_the_registry_digest(self):
        tree = self.tree()
        png = bytearray((tree.root / 'docs/social-preview.png').read_bytes())
        png[-1] ^= 1
        self.png(tree, bytes(png))
        self.assertFails(tree.errors(), self.RULE, 'docs/social-preview.png', 'sha256 differs')

    def test_a_fallback_font_list_fails(self):
        tree = self.tree()
        tree.replace('docs/social-preview.svg', 'font-family="Noto Sans"', 'font-family="Noto Sans, sans-serif"')
        self.assertFails(tree.errors(), self.RULE, 'no fallback list')

    def test_another_font_family_fails(self):
        tree = self.tree()
        tree.replace('docs/social-preview.svg', 'font-family="Noto Sans"', "font-family='Arial'")
        self.assertFails(tree.errors(), self.RULE, 'no fallback list')

    def test_a_css_font_family_with_a_fallback_fails(self):
        tree = self.tree()
        tree.append('docs/social-preview.svg', '<style>t { font-family: "Noto Sans", serif; }</style>\n')
        self.assertFails(tree.errors(), self.RULE, 'no fallback list')

    def test_a_missing_font_family_fails(self):
        tree = self.tree()
        tree.replace('docs/social-preview.svg', 'font-family="Noto Sans"', '')
        self.assertFails(tree.errors(), self.RULE, 'found []')

    def test_an_unregistered_asset_fails(self):
        tree = self.tree()
        self.registry(tree, lambda entry: entry.update(name='other'))
        self.assertFails(tree.errors(), self.RULE, 'no "social-preview" asset')


ABSENT = (
    # (rule, document, regex matching the text the rule reads, replacement without the fact)
    ('release-set', 'docs/RELEASE.md', r'The release set is six crates', 'The release set has crates'),
    ('release-set', 'docs/QUALIFICATION.md', r'Six Cargo-verified archives', 'Cargo-verified archives'),
    ('release-set', 'docs/QUALIFICATION.md', r'all six downloadable archives', 'the downloadable archives'),
    ('workspace-version', 'docs/RELEASE.md', r'PurRAssist 0\.1\.0 has an implemented scope', 'PurRAssist has an implemented scope'),
    ('workspace-version', 'docs/IMPLEMENTATION.md', r'Workspace version 0\.1\.0;', 'Workspace version:'),
    ('workspace-version', 'docs/QUALIFICATION.md', r'implemented 0\.1\.0 capability', 'implemented capability'),
    ('workspace-version', 'README.md', r'(?i:not yet tagged|published as)\s+`rust-v0\.1\.0`', 'untagged'),
    ('workspace-version', 'SECURITY.md', r'(?i:not yet tagged|published as)\s+`rust-v0\.1\.0`', 'untagged'),
    ('workspace-version', 'docs/IMPLEMENTATION.md', r'(?i:not yet tagged|published as)\s+`rust-v0\.1\.0`', 'untagged'),
    ('workspace-version', 'docs/QUALIFICATION.md', r'(?i:not yet tagged|published as)\s+`rust-v0\.1\.0`', 'untagged'),
    ('workspace-version', 'docs/RELEASE.md', r'(?i:not yet tagged|published as)\s+`rust-v0\.1\.0`', 'untagged'),
    ('fuzz-targets', 'docs/FUZZING.md', r'across\s+ten\s+`purrassist-core`\s+targets', 'across the targets'),
    ('fuzz-targets', 'docs/FUZZING.md', r'all\s+ten\s+targets', 'every target'),
    ('fuzz-targets', 'docs/QUALIFICATION.md', r'Ten targets \(including', 'Targets, including'),
    ('fuzz-targets', 'docs/QUALIFICATION.md', r'the ten mutation targets', 'the mutation targets'),
    ('fuzz-targets', 'README.md', r'ten coverage-guided targets', 'coverage-guided probes'),
    ('fuzz-targets', 'Makefile', r'for target in', 'for each in'),
    ('fuzz-targets', 'Makefile', r'TARGET selects one of', 'TARGET selects'),
    ('fuzz-targets', 'Makefile', r'across all ten targets', 'across the targets'),
    ('receipt-version', 'docs/QUALIFICATION.md', r'receipt wire v3', 'receipt wire format'),
    ('receipt-version', 'crates/core/src/contract.rs', r'RECEIPT_VERSION: u32 = 3;', 'RECEIPT_REVISION: u32 = 3;'),
    ('dialect-count', 'README.md', r'Twelve dialects ship', 'These dialects ship'),
    ('fuzz-baseline-table', 'docs/FUZZING.md', r'### Recorded 10,000-mutation baselines', '### Baselines'),
    ('fuzz-row-limits', 'docs/QUALIFICATION.md', r'`make fuzz`', '`make  fuzz`'),
)


class FactSentencesAreRequired(Case):
    def test_every_fact_fails_when_its_sentence_is_absent(self):
        for rule, name, pattern, replacement in ABSENT:
            with self.subTest(rule=rule, name=name, pattern=pattern):
                tree = self.tree()
                tree.sub(name, pattern, replacement, count=0)
                self.assertFails(tree.errors(), rule, NOT_FOUND)

    def test_a_fact_sentence_wrapped_across_lines_is_still_read(self):
        tree = self.tree()
        tree.sub('docs/RELEASE.md', r'The release set is six crates', 'The release set is\nfive\ncrates')
        self.assertFails(tree.errors(), 'release-set', "'five'")


if __name__ == '__main__':
    unittest.main()
