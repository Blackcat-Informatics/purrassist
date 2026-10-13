#!/usr/bin/env python3
# SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
# SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0
"""Offline tests for scripts/check-release.py.

Two groups, neither of which touches the network or this checkout:

* `validate_publish_workflow()` passes on the real publish.yml and fails, with
  a specific message, on modified copies (preflight removed, moved after auth
  and after `cargo publish`, owners check removed, user agent removed, comment
  removed).
* `validate_release_gate()` and `validate_toolchain_pins()` pass on the real
  Makefile and workflows and fail, naming their error, on copies in a scratch
  tree: a missing or reordered `release-gate` recipe line, an added
  prerequisite, an extra step beside the shared gate in release qualification,
  differing environments or installs, a weakened `lint` recipe, and every kind
  of toolchain pin drift (install line, env, publish.yml, rust-version, nightly).
  The gate jobs' allowlists (workflow, job, env, step, checkout and trigger shapes, each with
  its must-fail and must-pass fixtures) and the Makefile `check` prerequisites `lint` and `test`.
* `preflight_owners()` is driven by fake fetchers and fixture registry
  responses: success, a 404 crate, mismatched owners, network errors,
  non-JSON and malformed responses.
"""
import importlib.util
import json
from pathlib import Path
import re
import shutil
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


INSTALL = 'rustup toolchain install'
TREE_FILES = ('Makefile', 'Cargo.toml', 'rust-toolchain.toml', '.github/workflows/verify.yml',
              '.github/workflows/release-qualification.yml', '.github/workflows/publish.yml')
VERIFY = '.github/workflows/verify.yml'
RELEASE = '.github/workflows/release-qualification.yml'
PUBLISH_FILE = '.github/workflows/publish.yml'


class ScratchTree(unittest.TestCase):
    """The files the gate and pin rules read, copied to a scratch directory and edited there."""

    check = None

    def setUp(self):
        scratch = tempfile.TemporaryDirectory(prefix='test-check-release-')
        self.addCleanup(scratch.cleanup)
        self.root = Path(scratch.name)
        for name in TREE_FILES:
            (self.root / name).parent.mkdir(parents=True, exist_ok=True)
            shutil.copyfile(ROOT / name, self.root / name)

    def edit(self, name, old, new, count=1):
        path = self.root / name
        text = path.read_text(encoding='utf-8')
        self.assertIn(old, text, f'fixture precondition: {name} no longer contains {old!r}')
        path.write_text(text.replace(old, new, count), encoding='utf-8')

    def append(self, name, extra):
        path = self.root / name
        path.write_text(path.read_text(encoding='utf-8') + extra, encoding='utf-8')

    def fails(self, expect):
        with self.assertRaises(SystemExit) as raised:
            type(self).check(self.root)
        self.assertIn(expect, str(raised.exception))


class ReleaseGateIdentity(ScratchTree):
    check = staticmethod(CR.validate_release_gate)

    def test_real_files_pass(self):
        CR.validate_release_gate()
        CR.validate_release_gate(self.root)

    def test_a_missing_package_line_fails(self):
        self.edit('Makefile', '\t$(MAKE) package\n', '')
        self.fails('Makefile release-gate recipe must be exactly')

    def test_a_reordered_recipe_fails(self):
        self.edit('Makefile', '\t$(MAKE) check\n\t$(MAKE) corpus\n', '\t$(MAKE) corpus\n\t$(MAKE) check\n')
        self.fails('Makefile release-gate recipe must be exactly')

    def test_an_added_prerequisite_fails(self):
        self.edit('Makefile', '\nrelease-gate:\n', '\nrelease-gate: lint\n')
        self.fails('Makefile release-gate must have no prerequisites')

    def test_a_missing_release_gate_target_fails(self):
        self.edit('Makefile', '\nrelease-gate:\n', '\nrelease-gates:\n')
        self.fails('Makefile must define the `release-gate` target exactly once')

    def test_a_weakened_lint_recipe_fails(self):
        self.edit('Makefile', 'all-targets --locked -- -D warnings', 'all-targets -- -D warnings')
        self.fails('Makefile lint recipe must be exactly')

    def test_an_extra_make_step_in_release_qualification_fails(self):
        self.append(RELEASE, '      - name: Verify the corpus again\n        run: make corpus\n')
        self.fails('must have exactly the named steps')
        self.fails('must run `make` exactly once, as `make release-gate`')

    def test_an_extra_non_make_named_step_in_release_qualification_fails(self):
        self.append(RELEASE, '      - name: Say hello\n        run: echo hello\n')
        self.fails('must have exactly the named steps')

    def test_an_extra_unnamed_step_in_release_qualification_fails(self):
        self.append(RELEASE, '      - uses: actions/cache@v4\n')
        self.fails('exactly one unnamed step')

    def test_the_old_split_steps_fail(self):
        self.edit(RELEASE, '      - name: Verify the release gate\n        run: make release-gate\n',
                  '      - name: Verify workspace\n        run: make check\n')
        self.fails('must have exactly the named steps')

    def test_the_gate_outside_its_step_fails(self):
        self.edit(RELEASE, '      - name: Verify the release gate\n', '      - name: Verify something\n')
        self.fails('must run in the `Verify the release gate` step')

    def test_a_verify_job_without_the_gate_fails(self):
        self.edit(VERIFY, '      - run: make release-gate\n', '      - run: make check\n')
        self.fails('verify.yml release-gate job must run `make release-gate`')

    def test_a_missing_verify_release_gate_job_fails(self):
        self.edit(VERIFY, '\n  release-gate:\n', '\n  msrv:\n')
        self.fails('verify.yml must define a `release-gate` job')

    def test_a_release_gate_env_difference_fails(self):
        text = (self.root / VERIFY).read_text(encoding='utf-8')
        job = text.index('\n  release-gate:\n')
        marker = 'RUSTUP_TOOLCHAIN: 1.98.0\n'
        at = text.index(marker, job) + len(marker)
        (self.root / VERIFY).write_text(text[:at] + '      RUSTFLAGS: -D warnings\n' + text[at:], encoding='utf-8')
        self.fails('must equal release qualification')

    def test_differing_components_fail(self):
        self.edit(VERIFY, '--component clippy,rustfmt\n', '--component clippy\n')
        self.fails('job must install one stable toolchain with --component clippy,rustfmt')

    def test_a_different_stable_install_in_the_pr_job_fails(self):
        self.edit(VERIFY, f'{INSTALL} 1.98.0 --profile minimal --component clippy,rustfmt\n',
                  f'{INSTALL} 1.98.1 --profile minimal --component clippy,rustfmt\n')
        self.fails('the release-gate job must install the same toolchains as release')

    def test_a_missing_release_package_job_fails(self):
        self.edit(RELEASE, '\n  package:\n', '\n  build:\n')
        self.fails('release-qualification.yml must define a `package` job')

    def test_a_missing_nightly_install_fails(self):
        self.edit(VERIFY, f'      - run: {INSTALL} nightly-2026-09-14 --profile minimal --component llvm-tools\n'
                  '      - run: make release-gate', '      - run: make release-gate')
        self.fails('job must install one nightly toolchain with --component llvm-tools')


GATES = ((VERIFY, 'release-gate'), (RELEASE, 'package'))
SHA = '11d5960a326750d5838078e36cf38b85af677262'
CHECKOUT_LINE = f'      - uses: actions/checkout@{SHA} # v4\n        with:\n          persist-credentials: false\n'


class GateAllowlist(ScratchTree):
    """The gate jobs are allowlisted: anything not explicitly allowed fails with its own error."""

    check = staticmethod(CR.validate_release_gate)

    def edit_job(self, name, job, old, new):
        path = self.root / name
        text = path.read_text(encoding='utf-8')
        start = text.index(f'\n  {job}:\n') + 1
        following = re.search(r'\n  [^\s#]', text[start:])
        end = start + following.start() + 1 if following else len(text)
        self.assertIn(old, text[start:end], f'fixture precondition: job {job} lacks {old!r}')
        path.write_text(text[:start] + text[start:end].replace(old, new, 1) + text[end:], encoding='utf-8')

    def both(self, old, new):
        for name, job in GATES:
            self.edit_job(name, job, old, new)

    def passes(self):
        CR.validate_release_gate(self.root)

    # Env
    def test_env_removed_from_both_jobs_fails(self):
        self.both('    env:\n      RUSTUP_TOOLCHAIN: 1.98.0\n', '')
        self.fails('job must set `env`')
        self.fails('job env must be exactly RUSTUP_TOOLCHAIN, found []')

    def test_nightly_env_in_both_jobs_fails(self):
        self.both('RUSTUP_TOOLCHAIN: 1.98.0', 'RUSTUP_TOOLCHAIN: nightly-2026-09-14')
        self.fails('RUSTUP_TOOLCHAIN must be the stable release toolchain, not nightly-2026-09-14')

    def test_env_differing_from_the_stable_install_fails(self):
        for name, job in GATES:
            with self.subTest(job=job):
                self.setUp()
                self.edit_job(name, job, 'RUSTUP_TOOLCHAIN: 1.98.0', 'RUSTUP_TOOLCHAIN: 1.98.1')
                self.fails('RUSTUP_TOOLCHAIN 1.98.1 must equal its stable install 1.98.0')

    def test_job_level_rustflags_in_both_jobs_fails(self):
        self.both('RUSTUP_TOOLCHAIN: 1.98.0\n', 'RUSTUP_TOOLCHAIN: 1.98.0\n      RUSTFLAGS: --cap-lints=warn\n')
        self.fails("job env must be exactly RUSTUP_TOOLCHAIN, found ['RUSTFLAGS', 'RUSTUP_TOOLCHAIN']")

    def test_step_level_env_fails(self):
        for name, job in GATES:
            with self.subTest(job=job):
                self.setUp()
                self.edit_job(name, job, 'run: make release-gate\n',
                              'run: make release-gate\n        env:\n          CARGO_ENCODED_RUSTFLAGS: --cap-lints=warn\n')
                self.fails('may not set `env`')

    # Keys and modifiers
    def test_workflow_level_env_or_defaults_fails(self):
        for name in (VERIFY, RELEASE):
            for key, body in (('env', 'env:\n  RUSTFLAGS: --cap-lints=warn\n'),
                              ('defaults', 'defaults:\n  run:\n    shell: "true {0}"\n')):
                with self.subTest(workflow=name, key=key):
                    self.setUp()
                    self.edit(name, 'jobs:\n', body + 'jobs:\n')
                    self.fails(f'may not set the top-level key `{key}`')

    def test_job_level_defaults_fails(self):
        for name, job in GATES:
            with self.subTest(job=job):
                self.setUp()
                self.edit_job(name, job, '    steps:\n', '    defaults:\n      run:\n        shell: "true {0}"\n    steps:\n')
                self.fails('job may not set `defaults`')

    def test_continue_on_error_on_a_job_fails(self):
        for name, job in GATES:
            with self.subTest(job=job):
                self.setUp()
                self.edit_job(name, job, '    steps:\n', '    continue-on-error: true\n    steps:\n')
                self.fails('job may not set `continue-on-error`')

    def test_continue_on_error_on_a_step_fails(self):
        for name, job in GATES:
            with self.subTest(job=job):
                self.setUp()
                self.edit_job(name, job, 'run: make release-gate\n',
                              'run: make release-gate\n        continue-on-error: true\n')
                self.fails('may not set `continue-on-error`')

    def test_if_on_the_gate_step_fails(self):
        for name, job in GATES:
            with self.subTest(job=job):
                self.setUp()
                self.edit_job(name, job, '      - run: make release-gate\n' if job == 'release-gate'
                              else '        run: make release-gate\n',
                              '      - if: false\n        run: make release-gate\n' if job == 'release-gate'
                              else '        if: false\n        run: make release-gate\n')
                self.fails('may not set `if`')

    def test_shell_on_the_gate_step_fails(self):
        for name, job in GATES:
            with self.subTest(job=job):
                self.setUp()
                self.edit_job(name, job, 'run: make release-gate\n', 'run: make release-gate\n        shell: true {0}\n')
                self.fails('may not set `shell`')

    def test_shell_bash_is_allowed_only_on_the_version_step(self):
        self.edit_job(RELEASE, 'package', '        run: make release-gate\n',
                      '        shell: bash\n        run: make release-gate\n')
        self.fails('may not set `shell`')

    # Steps
    def test_an_extra_step_in_the_verify_gate_job_fails(self):
        self.edit(VERIFY, '      - run: make release-gate\n',
                  "      - run: printf '[build]\\nrustflags = [\"--cap-lints=warn\"]\\n' > .cargo/config.toml\n"
                  '      - run: make release-gate\n')
        self.fails('verify.yml release-gate job must have exactly the steps')

    def test_a_github_env_write_fails(self):
        self.edit(VERIFY, '      - run: make release-gate\n',
                  '      - run: echo RUSTFLAGS=--cap-lints=warn >> "$GITHUB_ENV"\n      - run: make release-gate\n')
        self.fails('verify.yml release-gate job must not write GITHUB_ENV or GITHUB_PATH')
        self.fails('verify.yml release-gate job must have exactly the steps')

    def test_a_github_path_write_in_the_version_step_fails(self):
        self.edit(RELEASE, '          set -euo pipefail\n', '          set -euo pipefail\n          echo . >> "$GITHUB_PATH"\n')
        self.fails('release-qualification.yml package job must not write GITHUB_ENV or GITHUB_PATH')

    # Trigger
    def test_the_pull_request_trigger_removed_fails(self):
        self.edit(VERIFY, '  pull_request:\n', '')
        self.fails('`on:` must contain a bare `pull_request:` trigger')

    def test_a_paths_filter_on_the_pull_request_trigger_fails(self):
        self.edit(VERIFY, '  pull_request:\n', "  pull_request:\n    paths: ['docs/**']\n")
        self.fails('`pull_request:` trigger must have no filters')

    def test_flow_spellings_of_the_trigger_fail(self):
        self.edit(VERIFY, '  pull_request:\n', '  pull_request: {paths: [x]}\n')
        self.fails('flow spelling such as `pull_request: {paths: [x]}` is unsupported')
        self.setUp()
        self.edit(VERIFY, 'on:\n  push:\n    branches: [main]\n  pull_request:\n  workflow_dispatch:\n',
                  'on: [push, pull_request]\n')
        self.fails('must have a block `on:` mapping')

    def test_a_bare_trigger_with_a_comment_passes(self):
        self.edit(VERIFY, '  pull_request:\n', '  pull_request: # every pull request\n')
        self.passes()

    # Checkout
    def test_a_ref_on_the_checkout_fails(self):
        for name, job in GATES:
            with self.subTest(job=job):
                self.setUp()
                self.edit_job(name, job, '          persist-credentials: false\n',
                              '          persist-credentials: false\n          ref: main\n')
                self.fails('step 1 `with:` must be exactly persist-credentials: false')

    def test_a_different_checkout_action_fails(self):
        for replacement in ('actions/checkout@v4', 'actions/cache@' + SHA, 'actions/checkout@' + SHA[:39]):
            for name, job in GATES:
                with self.subTest(job=job, uses=replacement):
                    self.setUp()
                    self.edit_job(name, job, f'actions/checkout@{SHA}', replacement)
                    self.fails('step 1 must `uses:` actions/checkout@<40-hex commit SHA>')

    def test_uses_or_with_on_a_later_step_fails(self):
        self.edit(VERIFY, '      - run: make release-gate\n',
                  '      - uses: actions/cache@' + SHA + '\n      - run: make release-gate\n')
        self.fails('may not set `uses`')

    # Must pass
    def test_a_named_verify_gate_step_passes(self):
        self.edit(VERIFY, '      - run: make release-gate\n', '      - name: Verify the release gate\n        run: make release-gate\n')
        self.passes()

    def test_a_quoted_env_value_passes(self):
        self.both('RUSTUP_TOOLCHAIN: 1.98.0', 'RUSTUP_TOOLCHAIN: "1.98.0"')
        self.passes()

    def test_a_trailing_comment_on_the_env_value_passes(self):
        self.both('RUSTUP_TOOLCHAIN: 1.98.0', 'RUSTUP_TOOLCHAIN: 1.98.0 # the release toolchain')
        self.passes()

    def test_swapped_install_order_passes(self):
        stable = f'      - run: {INSTALL} 1.98.0 --profile minimal --component clippy,rustfmt\n'
        nightly = f'      - run: {INSTALL} nightly-2026-09-14 --profile minimal --component llvm-tools\n'
        self.edit_job(VERIFY, 'release-gate', stable + nightly, nightly + stable)
        self.passes()

    # The Makefile check target
    def test_check_without_lint_fails(self):
        self.edit('Makefile', 'helper-check lint test doc', 'helper-check test doc')
        self.fails('Makefile check must depend on `lint`')

    def test_check_without_test_fails(self):
        self.edit('Makefile', 'helper-check lint test doc', 'helper-check lint doc')
        self.fails('Makefile check must depend on `test`')


class ToolchainPins(ScratchTree):
    check = staticmethod(CR.validate_toolchain_pins)

    def test_real_files_pass(self):
        CR.validate_toolchain_pins()
        CR.validate_toolchain_pins(self.root)

    def test_publish_install_without_components_still_matches(self):
        text = PUBLISH.read_text(encoding='utf-8')
        self.assertIn(f'{INSTALL} 1.98.0 --profile minimal\n', text)
        self.assertEqual([name for _, name in CR.toolchain_pins(text)], ['1.98.0', '1.98.0'])
        CR.validate_toolchain_pins(self.root)

    def test_pins_ignore_profile_and_components(self):
        self.assertEqual(CR.toolchain_pins(f'  run: {INSTALL} nightly-x --profile minimal --component a,b'),
                         [(1, 'nightly-x')])

    def test_an_install_line_bumped_alone_fails(self):
        self.edit(VERIFY, f'{INSTALL} 1.98.0', f'{INSTALL} 1.98.1')
        self.fails('stable toolchain pins differ')

    def test_an_env_bumped_alone_fails(self):
        self.edit(RELEASE, 'RUSTUP_TOOLCHAIN: 1.98.0', 'RUSTUP_TOOLCHAIN: 1.98.1')
        self.fails('stable toolchain pins differ')

    def test_a_publish_only_bump_fails(self):
        self.edit(PUBLISH_FILE, '1.98.0', '1.98.1', count=2)
        self.fails('stable toolchain pins differ')

    def test_a_rust_version_mismatch_fails(self):
        self.edit('Cargo.toml', 'rust-version = "1.98"', 'rust-version = "1.99"')
        self.fails('does not match Cargo.toml rust-version 1.99')

    def test_a_nightly_drift_in_one_verify_job_fails(self):
        self.edit(VERIFY, 'RUSTUP_TOOLCHAIN: nightly-2026-09-14', 'RUSTUP_TOOLCHAIN: nightly-2026-09-15')
        self.fails('does not match rust-toolchain.toml channel nightly-2026-09-14')

    def test_a_channel_change_fails(self):
        self.edit('rust-toolchain.toml', 'nightly-2026-09-14', 'nightly-2026-09-15')
        self.fails('does not match rust-toolchain.toml channel nightly-2026-09-15')


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
