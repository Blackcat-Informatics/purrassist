#!/usr/bin/env python3
# SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
# SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0
"""Offline tests for scripts/check-release.py.

Two groups, neither of which touches the network or this checkout:

* `validate_publish_workflow()` passes on the real publish.yml and fails, with
  a specific message, on modified copies (preflight removed, moved after auth
  and after `cargo publish`, owners check removed, user agent removed, comment
  removed).
* `preflight_owners()` is driven by fake fetchers and fixture registry
  responses: success, a 404 crate, mismatched owners, network errors,
  non-JSON and malformed responses.
"""
import importlib.util
import json
from pathlib import Path
import tempfile
import unittest

ROOT = Path(__file__).resolve().parent.parent
PUBLISH = ROOT / '.github/workflows/publish.yml'


def _load():
    path = ROOT / 'scripts/check-release.py'
    spec = importlib.util.spec_from_file_location('purrassist_check_release', path)
    if spec is None or spec.loader is None:
        raise SystemExit(f'could not load module spec for {path}')
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


CR = _load()
API = CR.API
STEP_START = f'      - name: {CR.PREFLIGHT_STEP}\n'


def _split_preflight(text):
    """Return (before, comment+step block, after) around the preflight step."""
    step_at = text.index(STEP_START)
    comment_at = step_at
    while text[:comment_at].rstrip(' ').endswith('\n') and \
            text[:comment_at].rstrip(' ').rsplit('\n', 2)[-2].lstrip().startswith('#'):
        comment_at = text[:comment_at].rstrip(' ').rstrip('\n').rfind('\n') + 1
    end = text.index('\n      - ', step_at + 1) + 1
    return text[:comment_at], text[comment_at:end], text[end:]


def owners_body(*logins):
    return json.dumps({'users': [{'login': login, 'kind': 'user'} for login in logins]}).encode()


def registry(owners=None, missing=(), raises=None, bodies=None):
    """Build a fake fetcher over a dict of crate -> owner logins."""
    owners = owners or {}
    seen = []

    def fetch(url):
        seen.append(url)
        if raises is not None:
            raise raises
        name = url[len(API) + 1:].split('/')[0]
        if name in missing:
            return 404, b''
        if url.endswith('/owners'):
            if bodies and name in bodies:
                return bodies[name]
            return 200, owners_body(*owners.get(name, ('alice', 'bob')))
        return 200, b'{"crate": {}}'

    fetch.seen = seen
    return fetch


class WorkflowValidation(unittest.TestCase):
    def check_copy(self, text, expect=None):
        with tempfile.TemporaryDirectory(prefix='test-check-release-') as scratch:
            copy = Path(scratch) / 'publish.yml'
            copy.write_text(text, encoding='utf-8')
            if expect is None:
                CR.validate_publish_workflow(copy)
                return
            with self.assertRaises(SystemExit) as raised:
                CR.validate_publish_workflow(copy)
            self.assertIn(expect, str(raised.exception))

    def test_real_workflow_passes(self):
        CR.validate_publish_workflow()
        self.check_copy(PUBLISH.read_text(encoding='utf-8'))

    def test_id_token_in_another_job_fails(self):
        text = PUBLISH.read_text(encoding='utf-8')
        gate = '\n  gate:\n'
        self.assertIn(gate, text)
        head, _, rest = text.partition(gate)
        line, _, body = rest.partition('\n')
        self.check_copy(head + gate + line + '\n    permissions:\n      id-token: write\n' + body,
                        'only the publish job may declare an id-token permission')

    def test_workflow_wide_id_token_fails(self):
        text = PUBLISH.read_text(encoding='utf-8')
        self.assertIn('permissions:\n  contents: read\n', text)
        self.check_copy(text.replace('permissions:\n  contents: read\n',
                                     'permissions:\n  contents: read\n  id-token: write\n', 1),
                        'only the publish job may declare an id-token permission')

    def gate_with(self, permissions):
        text = PUBLISH.read_text(encoding='utf-8')
        head, gate, rest = text.partition('\n  gate:\n')
        self.assertTrue(gate)
        line, _, body = rest.partition('\n')
        return head + gate + line + '\n' + permissions + body

    def test_other_id_token_forms_fail(self):
        for permissions in ('    permissions:\n      "id-token": write\n',
                            "    permissions:\n      'id-token': write\n",
                            '    permissions: { contents: read, id-token: write }\n'):
            with self.subTest(permissions=permissions):
                self.check_copy(self.gate_with(permissions),
                                'only the publish job may declare an id-token permission')

    def test_write_all_fails(self):
        for permissions in ('    permissions: write-all\n', '    permissions: ! write-all\n',
                            '    permissions: !1 write-all\n'):
            with self.subTest(permissions=permissions):
                self.check_copy(self.gate_with(permissions), 'must not grant permissions: write-all')
        text = PUBLISH.read_text(encoding='utf-8')
        self.check_copy(text.replace('permissions:\n  contents: read\n', 'permissions: write-all\n', 1),
                        'must not grant permissions: write-all')

    def test_id_token_in_a_job_named_with_a_digit_after_publish_fails(self):
        text = PUBLISH.read_text(encoding='utf-8').rstrip('\n') + '\n'
        extra = ('  job2:\n    runs-on: ubuntu-24.04\n    permissions:\n      id-token: write\n'
                 '    steps:\n      - run: true\n')
        self.check_copy(text + extra, 'only the publish job may declare an id-token permission')

    def test_a_job_beyond_gate_and_publish_fails_however_it_is_spelled(self):
        text = PUBLISH.read_text(encoding='utf-8').rstrip('\n') + '\n'
        for header in ('  extra:', '  extra: # helper', '  extra: ', '  "extra":', "  'job2':"):
            with self.subTest(header=header):
                extra = header + '\n    runs-on: ubuntu-24.04\n    steps:\n      - run: true\n'
                self.check_copy(text + extra, 'must define exactly the jobs gate and publish')

    def test_an_id_token_in_a_trailing_commented_job_header_fails(self):
        text = PUBLISH.read_text(encoding='utf-8').rstrip('\n') + '\n'
        extra = ('  extra: # helper\n    runs-on: ubuntu-24.04\n    permissions:\n      id-token: write\n'
                 '    steps:\n      - run: true\n')
        self.check_copy(text + extra, 'only the publish job may declare an id-token permission')

    def test_anchors_and_aliases_fail(self):
        text = PUBLISH.read_text(encoding='utf-8')
        self.assertIn('    permissions:\n      id-token: write\n', text)
        anchored = text.replace('    permissions:\n      id-token: write\n',
                                '    permissions: &oidc\n      id-token: write\n', 1)
        self.check_copy(anchored, 'must not use YAML anchors or aliases')
        self.check_copy(self.gate_with('    permissions: *oidc\n'), 'must not use YAML anchors or aliases')

    def test_escapes_tags_and_block_scalar_permissions_fail(self):
        for permissions in ('    permissions:\n      "id\\x2dtoken": write\n',
                            '    permissions: !!str write-all\n',
                            '    permissions: !<tag:yaml.org,2002:str> write-all\n',
                            '    permissions: !str write-all\n',
                            '    permissions: >-\n      write-all\n'):
            with self.subTest(permissions=permissions):
                self.check_copy(self.gate_with(permissions),
                                'must not use escapes, YAML tags or block-scalar permissions')

    def test_shell_redirections_are_not_anchors(self):
        self.check_copy(self.gate_with('    # echo done >&2 && true\n'))

    def test_a_commented_id_token_is_ignored(self):
        self.check_copy(self.gate_with('    # id-token: write is reserved for the publish job\n'))

    def test_preflight_removed_fails(self):
        before, _, after = _split_preflight(PUBLISH.read_text(encoding='utf-8'))
        self.check_copy(before + after, f'must have a "{CR.PREFLIGHT_STEP}" step')

    def test_preflight_after_auth_fails(self):
        before, block, after = _split_preflight(PUBLISH.read_text(encoding='utf-8'))
        auth_end = after.index('      - name: Publish core')
        moved = before + after[:auth_end] + block + after[auth_end:]
        self.check_copy(moved, f'must precede the "{CR.AUTH_STEP}" step')

    def test_preflight_after_publish_fails(self):
        before, block, after = _split_preflight(PUBLISH.read_text(encoding='utf-8'))
        verify = after.index('      - name: Verify published packages')
        moved = before + after[:verify] + block + after[verify:]
        self.check_copy(moved, 'must precede the first cargo publish')
        self.assertIn(f'must precede the "{CR.AUTH_STEP}" step', self.failure_of(moved))

    def failure_of(self, text):
        with tempfile.TemporaryDirectory(prefix='test-check-release-') as scratch:
            copy = Path(scratch) / 'publish.yml'
            copy.write_text(text, encoding='utf-8')
            with self.assertRaises(SystemExit) as raised:
                CR.validate_publish_workflow(copy)
            return str(raised.exception)

    def test_owners_check_removed_fails(self):
        text = PUBLISH.read_text(encoding='utf-8').replace(' --preflight-owners', '', 1)
        self.check_copy(text, 'must run scripts/check-release.py --preflight-owners')

    def test_user_agent_removed_fails(self):
        text = PUBLISH.read_text(encoding='utf-8')
        start = text.index(' --user-agent', text.index(STEP_START))
        end = text.index('\n', start)
        self.check_copy(text[:start] + text[end:], 'purrassist-release user agent')

    def test_unverifiable_trusted_publisher_note_required(self):
        text = PUBLISH.read_text(encoding='utf-8').replace('Trusted-publisher state', 'Something else')
        self.check_copy(text, 'trusted-publisher state is not verifiable')


class Preflight(unittest.TestCase):
    def test_all_present_equal_owners_pass(self):
        fetch = registry()
        self.assertEqual(CR.preflight_owners(fetch), {'alice', 'bob'})
        for name in CR.RELEASE_ORDER:
            self.assertIn(f'{API}/{name}', fetch.seen)
            self.assertIn(f'{API}/{name}/owners', fetch.seen)

    def test_owner_order_is_irrelevant(self):
        fetch = registry({'purrassist-cli': ('bob', 'alice')})
        self.assertEqual(CR.preflight_owners(fetch), {'alice', 'bob'})

    def test_missing_crate_fails_naming_it(self):
        with self.assertRaises(SystemExit) as raised:
            CR.preflight_owners(registry(missing={'purrassist-derive'}))
        message = str(raised.exception)
        self.assertIn('purrassist-derive', message)
        self.assertIn('returned HTTP 404', message)

    def test_mismatched_owners_fail(self):
        with self.assertRaises(SystemExit) as raised:
            CR.preflight_owners(registry({'purrassist-host': ('alice', 'mallory')}))
        message = str(raised.exception)
        self.assertIn('purrassist-host', message)
        self.assertIn('differ from purrassist-core', message)

    def test_missing_owner_subset_fails(self):
        with self.assertRaises(SystemExit):
            CR.preflight_owners(registry({'purrassist-syntax': ('alice',)}))

    def test_network_error_fails(self):
        with self.assertRaises(SystemExit) as raised:
            CR.preflight_owners(registry(raises=OSError('connection refused')))
        self.assertIn('connection refused', str(raised.exception))

    def test_non_json_owners_fail(self):
        with self.assertRaises(SystemExit) as raised:
            CR.preflight_owners(registry(bodies={'purrassist': (200, b'<html>oops</html>')}))
        self.assertIn('not the expected JSON', str(raised.exception))

    def test_unexpected_json_shape_fails(self):
        for body in (b'[]', b'{"users": null}', b'{"users": [{"id": 1}]}', b'{"users": []}'):
            with self.subTest(body=body):
                with self.assertRaises(SystemExit):
                    CR.preflight_owners(registry(bodies={'purrassist': (200, body)}))

    def test_owners_endpoint_error_fails(self):
        with self.assertRaises(SystemExit) as raised:
            CR.preflight_owners(registry(bodies={'purrassist-cli': (500, b'')}))
        self.assertIn('/owners returned HTTP 500', str(raised.exception))

    def test_reports_every_failure(self):
        with self.assertRaises(SystemExit) as raised:
            CR.preflight_owners(registry(missing={'purrassist-derive', 'purrassist-cli'}))
        message = str(raised.exception)
        self.assertIn('purrassist-derive', message)
        self.assertIn('purrassist-cli', message)

    def test_core_missing_fails_without_reference(self):
        with self.assertRaises(SystemExit):
            CR.preflight_owners(registry(missing={'purrassist-core'}))


if __name__ == '__main__':
    unittest.main()
