#!/usr/bin/env python3
# SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
# SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0
"""Check the release set, the publish workflow, gate identity and toolchain pins, offline.

Run with no argument (`make check`, and the release-qualification workflow), it reads
`cargo metadata` and the repository and checks, without publishing anything:

* the release set: `RELEASE_ORDER` is exactly the publishable workspace, all versions and
  internal version requirements match, and normal and dev dependencies precede their users
  (`--tag rust-vVERSION` also checks the tag against the version);
* the publish workflow: `validate_publish_workflow()` holds `.github/workflows/publish.yml` to a
  closed shape (tag trigger, `crates-io` environment, OIDC permission only in the publish job,
  SHA-pinned actions, the preflight step, the dependency-ordered publish loop);
* gate identity: `validate_release_gate()` holds the release-qualification `package` job and the
  pull-request `Verify` workflow's `release-gate` job to allowlists (workflow and job keys, an
  `env` of exactly `RUSTUP_TOOLCHAIN`, step keys, a SHA-pinned checkout, an unfiltered
  `pull_request:` trigger, no `GITHUB_ENV` or `GITHUB_PATH` write) so both run the same
  `make release-gate` on the same toolchains; it also checks the Makefile `release-gate`,
  `check` (depends on `lint` and `test`) and `lint` recipes, and the corpus-suite rule:
  `make lint` lints every `test = false` suite, named once in `CORPUS_SUITES`;
* toolchain pins: `validate_toolchain_pins()` reads every file in `.github/workflows/` (install
  lines, `rustup default`, `rustup override set`, `cargo +X`, inline `RUSTUP_TOOLCHAIN=` and
  `RUSTUP_TOOLCHAIN:` keys). Each pin must be a literal `nightly-YYYY-MM-DD` or `X.Y.Z`; every
  stable pin is identical and matches `rust-version`; every nightly pin matches
  `rust-toolchain.toml`.

`--preflight-owners --user-agent AGENT` is the one networked mode: the publish preflight.
"""

import http.client
import importlib.util
import json
from pathlib import Path
import re
import subprocess
import sys
import tomllib
import urllib.error
import urllib.request

ROOT = Path(__file__).resolve().parent.parent

# The sole release order. Both normal and dev dependencies must precede users:
# cargo publish verifies the packaged crate against the registry.
RELEASE_ORDER = (
    'purrassist-core',
    'purrassist-derive',
    'purrassist-syntax',
    'purrassist',
    'purrassist-host',
    'purrassist-cli',
)


API = 'https://crates.io/api/v1/crates'
PREFLIGHT_STEP = 'Preflight crates.io names and owners'
AUTH_STEP = 'Authenticate with crates.io trusted publishing'


def preflight_owners(fetch, names=RELEASE_ORDER):
    """Check crate names and owners on the registry before anything publishes.

    `fetch(url)` returns `(status, body_bytes)` and may raise `OSError` (the
    production fetcher raises it for network failures). Every crate in
    `names` must answer 200 at `/api/v1/crates/<name>`, and the set of owner
    logins at `/api/v1/crates/<name>/owners` must equal that of the first
    name (`purrassist-core`). No login is hard-coded: the release set must be
    owned identically, whoever owns it. Returns the shared owner set; raises
    `SystemExit` listing every failure.

    Trusted-publisher configuration is not visible to an unauthenticated
    request, so this does not and cannot check it; it stays the manual
    pre-tag step in docs/RELEASE.md.
    """
    errors = []
    owners = {}
    for name in names:
        try:
            status, _ = fetch(f'{API}/{name}')
        except OSError as error:
            errors.append(f'{name}: request for {API}/{name} failed: {error}')
            continue
        if status != 200:
            errors.append(f'{name}: {API}/{name} returned HTTP {status}, expected 200 '
                          '(the crate name must be reserved before tagging)')
            continue
        try:
            status, body = fetch(f'{API}/{name}/owners')
        except OSError as error:
            errors.append(f'{name}: request for {API}/{name}/owners failed: {error}')
            continue
        if status != 200:
            errors.append(f'{name}: {API}/{name}/owners returned HTTP {status}, expected 200')
            continue
        try:
            users = json.loads(body)['users']
            owners[name] = {user['login'] for user in users}
        except (ValueError, KeyError, TypeError) as error:
            errors.append(f'{name}: owners response is not the expected JSON '
                          f'({type(error).__name__}: {error})')
            continue
        if not owners[name]:
            errors.append(f'{name}: crates.io reports no owners')
    reference = owners.get(names[0])
    if reference is not None:
        for name in names[1:]:
            if name in owners and owners[name] != reference:
                errors.append(f'{name}: owners {sorted(owners[name])} differ from '
                              f'{names[0]} owners {sorted(reference)}')
    if errors:
        raise SystemExit('publish preflight failed:\n' + '\n'.join(errors))
    return reference


def registry_fetch(user_agent):
    """Return a `fetch(url)` that GETs from the real registry with `user_agent`."""
    def fetch(url):
        request = urllib.request.Request(url, headers={'User-Agent': user_agent})
        try:
            with urllib.request.urlopen(request, timeout=60) as response:
                return response.status, response.read()
        except urllib.error.HTTPError as error:
            return error.code, b''
        except urllib.error.URLError as error:
            raise OSError(str(error.reason)) from error
        except http.client.HTTPException as error:
            raise OSError(f'{type(error).__name__}: {error}') from error
    return fetch


def run_preflight(user_agent):
    owners = preflight_owners(registry_fetch(user_agent))
    print(f'Publish preflight passed: {len(RELEASE_ORDER)} crates exist and share owners '
          + ', '.join(sorted(owners)))


def validate_publish_workflow(path=None):
    """Validate `.github/workflows/publish.yml` without a YAML dependency.

    Checked by plain text/regex against the exact shape this project's
    single publish workflow uses, mirroring the text-based checks the rest
    of this repository's hygiene scripts already use instead of adding a
    YAML-parsing dependency for one file this script itself authors no
    freeform structure for.
    """
    path = Path(path) if path else ROOT / '.github/workflows/publish.yml'
    text = path.read_text(encoding='utf-8')
    errors = []

    if not re.search(r"^\s*tags:\s*\['?rust-v\*'?\]\s*$", text, re.M):
        errors.append("publish.yml must trigger on push of tags matching 'rust-v*'")
    if 'workflow_dispatch' in text.split('permissions:', 1)[0]:
        errors.append('publish.yml must not be manually dispatchable; tag push only')

    publish_job = text.split('\n  publish:\n', 1)
    if len(publish_job) != 2:
        errors.append('publish.yml must define a `publish` job')
        publish_job_body = ''
    else:
        # Bound the job body at the next non-comment line indented two spaces:
        # under `jobs:` that is the next job, however its key is spelled.
        match = re.search(r'\n  [^\s#]', publish_job[1])
        publish_job_body = publish_job[1][:match.start()] if match else publish_job[1]
        if not re.search(r'^\s*environment:\s*crates-io\s*$', publish_job_body, re.M):
            errors.append('publish job must run in the crates-io environment')
        if not re.search(r'^\s*id-token:\s*write\s*$', publish_job_body, re.M):
            errors.append('publish job must declare permissions: id-token: write')
        if not re.search(r'^\s*contents:\s*read\s*$', publish_job_body, re.M):
            errors.append('publish job must declare permissions: contents: read')
        if not re.search(r'^\s*needs:\s*gate\s*$', publish_job_body, re.M):
            errors.append('publish job must depend on a gate job that checks release qualification')

    # The purrassist-derive trusted publisher names no environment (an owner
    # decision recorded in docs/RELEASE.md), so the only thing keeping its
    # registry token inside the crates-io job is that no other job, and not
    # the workflow as a whole, may request an OIDC token. Text matching cannot
    # follow every YAML spelling, so the workflow is held to a closed shape:
    # exactly the `gate` and `publish` jobs, no anchors or aliases (which could
    # copy a permissions block between jobs), no write-all, and every id-token
    # key, in block, inline or quoted form, inside the publish job.
    jobs = text.split('\njobs:\n', 1)
    names = [re.sub(r'\s*#.*$', '', line[2:]).strip().strip('\'"').rstrip(':').strip('\'"')
             for line in (jobs[1].splitlines() if len(jobs) == 2 else [])
             if re.match(r'  [^\s#]', line)]
    if names != ['gate', 'publish']:
        errors.append(f'publish.yml must define exactly the jobs gate and publish, found {names}')
    if re.search(r'^[^#\n]*(?:^\s*|[:\[{,-]\s*)[&*][A-Za-z0-9_]', text, re.M):
        errors.append('publish.yml must not use YAML anchors or aliases')
    # Escapes, explicit tags and block scalars can spell a grant the patterns
    # above do not read; none has a use in this workflow.
    if re.search(r'\\|^[^#\n]*(?:^|[\s:\[{,-])!(?:!|<|[A-Za-z])|^[^#\n]*\bpermissions:\s*[|>]', text, re.M):
        errors.append('publish.yml must not use escapes, YAML tags or block-scalar permissions')
    id_token = re.compile(r'^[^#\n]*["\']?\bid-token["\']?\s*:', re.M)
    if len(id_token.findall(text)) != len(id_token.findall(publish_job_body)):
        errors.append('only the publish job may declare an id-token permission')
    # Any write-all outside a comment, whatever tag, quote or layout precedes it.
    if re.search(r'^[^#\n]*\bwrite-all\b', text, re.M):
        errors.append('publish.yml must not grant permissions: write-all')

    if 'release-qualification.yml' not in text:
        errors.append('publish.yml must gate on release-qualification.yml for the same commit')

    for match in re.finditer(r'uses:\s*(\S+)@(\S+?)(?:\s*#.*)?$', text, re.M):
        reference, ref = match[1], match[2]
        if not re.fullmatch(r'[0-9a-f]{40}', ref):
            errors.append(f'action {reference} must be pinned by full commit SHA, not {ref!r}')
    if not re.search(r'\bpersist-credentials:\s*false\b', text):
        errors.append('publish.yml checkout must set persist-credentials: false')

    auth_actions = re.findall(r'rust-lang/crates-io-auth-action@([0-9a-f]{40})', text)
    if len(auth_actions) != 1:
        errors.append('publish.yml must authenticate with exactly one pinned rust-lang/crates-io-auth-action step')
    else:
        tooling = json.loads((ROOT / 'vendor/tooling.json').read_text(encoding='utf-8'))
        registered = {entry['version'] for entry in tooling if entry['name'] == 'crates-io-auth-action'}
        if auth_actions[0] not in registered:
            errors.append('publish.yml crates-io-auth-action SHA is not registered in vendor/tooling.json')

    loop_match = re.search(r'for crate in ([^;\n]+); do', text)
    if not loop_match:
        errors.append('publish.yml must publish crates in a single explicit dependency-ordered sequence')
    elif tuple(loop_match[1].split()) != RELEASE_ORDER:
        errors.append(f'publish.yml publish order {tuple(loop_match[1].split())} does not match {RELEASE_ORDER}')

    # Step order: the preflight must precede auth and every cargo publish.
    preflight_at = text.find(f'- name: {PREFLIGHT_STEP}\n')
    auth_at = text.find(f'- name: {AUTH_STEP}\n')
    publish_at = text.find('cargo publish')
    if preflight_at < 0:
        errors.append(f'publish.yml must have a "{PREFLIGHT_STEP}" step')
    else:
        step = text[preflight_at:].split('\n      - ', 1)[0]
        if not re.search(r'scripts/check-release\.py --preflight-owners\b', step):
            errors.append('preflight step must run scripts/check-release.py --preflight-owners '
                          '(crate existence and owners check against '
                          'https://crates.io/api/v1/crates/<name>/owners)')
        if not re.search(r'--user-agent\s+"?\'?purrassist-release', step):
            errors.append('preflight step must send the purrassist-release user agent via --user-agent')
        comment = []
        for line in reversed(text[:preflight_at].rstrip(' ').split('\n')[:-1]):
            if not line.lstrip().startswith('#'):
                break
            comment.append(line)
        if 'trusted-publisher state' not in ' '.join(comment).lower().replace('#', ''):
            errors.append('preflight step comment must state that trusted-publisher state is '
                          'not verifiable unauthenticated and stays a manual pre-tag step')
        if auth_at >= 0 and preflight_at > auth_at:
            errors.append(f'preflight step must precede the "{AUTH_STEP}" step')
        if publish_at >= 0 and preflight_at > publish_at:
            errors.append('preflight step must precede the first cargo publish')

    if not re.search(r'cargo publish -p "\$crate" --locked', text):
        errors.append('publish.yml must run cargo publish --locked (full verification) per crate')

    if not re.search(r'scripts/verify-published\.py --version', text):
        errors.append('publish.yml must run scripts/verify-published.py --version after publishing')

    if errors:
        raise SystemExit('\n'.join(errors))
    print('publish.yml verified: tag trigger, crates-io environment, id-token permission, '
          'qualification gate, SHA-pinned actions, dependency order, and verify-published step.')


RELEASE_STEPS = ('Install release toolchain', 'Install pinned coverage-feedback toolchain',
                 'Verify release set and version', 'Verify the release gate')
GATE_STEP = 'Verify the release gate'
GATE_RECIPE = ('$(MAKE) check', '$(MAKE) corpus', 'python3 scripts/test-fuzz-feedback.py',
               'python3 scripts/test-verify-published.py', '$(MAKE) package')
LINT_RECIPE = ('cargo clippy --workspace --all-targets --locked -- -D warnings',
               'cargo clippy -p purrassist-core --locked $(foreach suite,$(CORPUS_SUITES),--test $(suite)) -- -D warnings')
CORPUS_ASSIGNMENT = re.compile(r'^(?!\t)\s*(?:(?:override|export|private)\s+)*CORPUS_SUITES\s*[:+?!]*=')
CORPUS_DEFINITION = re.compile(r'^CORPUS_SUITES :=(.*)$')
WORKFLOW_KEYS = ('name', 'on', 'permissions', 'jobs', 'concurrency')
GATE_JOB_KEYS = ('runs-on', 'timeout-minutes', 'env', 'steps')
STEP_KEYS = ('name', 'uses', 'with', 'run')
SHELL_STEP = 'Verify release set and version'
CHECKOUT_USES = re.compile(r'actions/checkout@[0-9a-f]{40}')
PLAIN_INSTALL = re.compile(r'rustup\s+toolchain\s+install\s+[^\s;&|$`()<>]+(?:\s+--?[A-Za-z-]+\s+[^\s;&|$`()<>]+)*')
ENV_FILES = re.compile(r'\bGITHUB_(?:ENV|PATH)\b')
STABLE_COMPONENTS = 'clippy,rustfmt'
NIGHTLY_COMPONENTS = 'llvm-tools'
# The pin extractors: `RUSTUP_TOOLCHAIN:` keys, plus the commands `_rustup_pins` tokenises.
ENV_PIN = re.compile(r'^\s*RUSTUP_TOOLCHAIN:\s*(.*)$')
SHELL_TOKEN = re.compile(r'\$\{\{.*?\}\}|"[^"]*"|\'[^\']*\'|[;&|]+|[^\s;&|]+')
VALUE_FLAGS = ('--profile', '--component', '-c', '--target', '-t')
PIN_FORMAT = re.compile(r'nightly-\d{4}-\d{2}-\d{2}|\d+\.\d+\.\d+')


def _load_check_docs():
    """Import scripts/check-docs.py (its name is no module name) for its `workflow_steps` reader."""
    path = ROOT / 'scripts/check-docs.py'
    spec = importlib.util.spec_from_file_location('purrassist_check_docs', path)
    if spec is None or spec.loader is None:
        raise SystemExit(f'could not load module spec for {path}')
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def _code_lines(text):
    """The (line number, line) pairs of `text` that are not whole-line comments."""
    return [(number, line) for number, line in enumerate(text.splitlines(), 1)
            if not line.lstrip().startswith('#')]


def _job_text(text, job):
    """The lines of the job `job` of a workflow, from its key to the next two-space key, or None."""
    lines = text.splitlines()
    start = next((index for index, line in enumerate(lines) if line.rstrip() == f'  {job}:'), None)
    if start is None:
        return None
    end = next((index for index in range(start + 1, len(lines))
                if re.match(r'  [^\s#]', lines[index])), len(lines))
    return '\n'.join(lines[start:end]) + '\n'


def _job_env(job_text):
    """The `env:` mapping of a job as a dict, read by indentation."""
    lines = job_text.splitlines()
    env = {}
    for index, line in enumerate(lines):
        if re.fullmatch(r'    env:\s*', line):
            for entry in lines[index + 1:]:
                if entry.strip() and not entry.lstrip().startswith('#'):
                    if len(entry) - len(entry.lstrip()) <= 4:
                        break
                    key, _, value = entry.strip().partition(':')
                    env[key.strip()] = _plain_value(value)
            break
    return env


def _plain_value(value):
    """A YAML plain or quoted scalar without its trailing comment or quotes."""
    return re.sub(r'\s+#.*$', '', value.strip()).strip().strip('\'"')


def _job_keys(job_text):
    """The keys of a job at its own indentation, as {key: inline value}; unrecognised lines at
    that indentation appear as the key `<unparsed>`."""
    keys = {}
    for _, line in _code_lines(job_text):
        if re.match(r'    \S', line):
            match = re.match(r'    (["\']?)([A-Za-z_][\w-]*)\1\s*:\s*(.*)$', line)
            keys[match[2] if match else '<unparsed>'] = _plain_value(match[3]) if match else line.strip()
    return keys


def _job_steps(job_text):
    """The steps of a job's block `steps:` list, read by indentation, as dicts with `keys`
    ({key: inline value}) and `body` ({key: deeper lines}); None when the job has no `steps:`
    line. Unrecognised lines at a step's key column appear as the key `<unparsed>`."""
    lines = job_text.splitlines()
    start = next((index for index, line in enumerate(lines)
                  if re.fullmatch(r'    steps:\s*(?:#.*)?', line)), None)
    if start is None:
        return None
    steps, item_column, key_column, current, current_key = [], None, 0, None, None
    for line in lines[start + 1:]:
        stripped = line.strip()
        if not stripped or stripped.startswith('#'):
            continue
        indent = len(line) - len(line.lstrip())
        if indent <= 4:
            break
        opening = re.match(r'(\s*)-\s+', line)
        if opening and (item_column is None or len(opening.group(1)) == item_column):
            item_column, key_column = len(opening.group(1)), len(opening.group(0))
            current = {'keys': {}, 'body': {}}
            steps.append(current)
            current_key = None
            line = ' ' * key_column + line[key_column:]
            indent = key_column
        if current is None:
            continue
        key = re.match(rf' {{{key_column}}}(["\']?)([A-Za-z_][\w-]*)\1\s*:\s*(.*?)\s*$', line)
        if key and indent == key_column:
            current_key = key[2]
            current['keys'][current_key] = _plain_value(key[3])
            current['body'][current_key] = []
        elif indent > key_column and current_key is not None:
            current['body'][current_key].append(stripped)
        else:
            current['keys']['<unparsed>'] = stripped
    return steps


def _step_run(step):
    """The command text of a step's `run:`, or None."""
    inline = step['keys'].get('run')
    if inline is None:
        return None
    body = step['body'].get('run', [])
    return '\n'.join(body) if inline[:1] in ('|', '>') else ' '.join([inline, *body]).strip()


def _step_with(step):
    """The `with:` mapping of a step as {key: value}."""
    return {key.strip(): _plain_value(value)
            for key, _, value in (line.partition(':') for line in step['body'].get('with', []))}


def _workflow_shape_errors(name, text):
    """Errors for top-level keys of a workflow outside `WORKFLOW_KEYS`: a workflow-wide `env` or
    `defaults` reaches the gate job without appearing in it."""
    errors = []
    for _, line in _code_lines(text):
        if not line.strip() or line[0] in ' \t' or line.startswith('---'):
            continue
        match = re.match(r'(["\']?)([A-Za-z_][\w-]*)\1\s*:', line)
        key = match[2] if match else line.strip()
        if key not in WORKFLOW_KEYS:
            errors.append(f'{name} may not set the top-level key `{key}` (allowed: {list(WORKFLOW_KEYS)}); '
                          'a workflow-wide setting reaches the gate job without appearing in it')
    return errors


def _pull_request_errors(name, text):
    """Errors unless `on:` is a block mapping holding a bare `pull_request:`: nothing after the
    colon but a comment, and no filter keys under it. Flow spellings are unsupported."""
    lines = text.splitlines()
    start = next((index for index, line in enumerate(lines)
                  if re.fullmatch(r'(["\']?)on\1:\s*(?:#.*)?', line)), None)
    if start is None:
        return [f'{name} must have a block `on:` mapping (a flow spelling such as '
                '`on: [push, pull_request]` is unsupported)']
    block = []
    for line in lines[start + 1:]:
        if line.strip() and not line.lstrip().startswith('#'):
            if line[0] not in ' \t':
                break
            block.append(line)
    bare = [index for index, line in enumerate(block) if re.fullmatch(r'  pull_request:\s*(?:#.*)?', line)]
    if len(bare) != 1:
        return [f'{name} `on:` must contain a bare `pull_request:` trigger with nothing after the colon '
                '(a flow spelling such as `pull_request: {paths: [x]}` is unsupported)']
    if bare[0] + 1 < len(block) and len(block[bare[0] + 1]) - len(block[bare[0] + 1].lstrip()) > 2:
        return [f'{name} `pull_request:` trigger must have no filters (branches, paths, types ...): '
                'a filtered trigger skips the gate on some pull requests']
    return []


def _gate_job_errors(label, job_text):
    """Errors for a gate job that is not exactly the allowlisted shape: job keys, env, step keys,
    the checkout step, and the absence of `GITHUB_ENV` and `GITHUB_PATH` writes. Step order
    and names are checked by the caller."""
    errors = []
    keys = _job_keys(job_text)
    for key in sorted(set(keys) - set(GATE_JOB_KEYS)):
        errors.append(f'{label} job may not set `{key}` (allowed: {list(GATE_JOB_KEYS)})')
    for key in sorted(set(GATE_JOB_KEYS) - set(keys)):
        errors.append(f'{label} job must set `{key}`')
    if keys.get('env'):
        errors.append(f'{label} job `env:` must be a block mapping')
    env = _job_env(job_text)
    if set(env) != {'RUSTUP_TOOLCHAIN'}:
        errors.append(f'{label} job env must be exactly RUSTUP_TOOLCHAIN, found {sorted(env)}')
    else:
        stable = [i[0] for i in _job_installs(job_text) if not i[0].startswith('nightly')]
        if env['RUSTUP_TOOLCHAIN'].startswith('nightly'):
            errors.append(f'{label} job RUSTUP_TOOLCHAIN must be the stable release toolchain, '
                          f'not {env["RUSTUP_TOOLCHAIN"]}')
        elif len(stable) == 1 and env['RUSTUP_TOOLCHAIN'] != stable[0]:
            errors.append(f'{label} job RUSTUP_TOOLCHAIN {env["RUSTUP_TOOLCHAIN"]} must equal its '
                          f'stable install {stable[0]}')
    if ENV_FILES.search('\n'.join(line for _, line in _code_lines(job_text))):
        errors.append(f'{label} job must not write GITHUB_ENV or GITHUB_PATH')
    steps = _job_steps(job_text)
    if steps is None:
        errors.append(f'{label} job must have a block `steps:` list')
        return errors
    for index, step in enumerate(steps):
        where = f'{label} step {index + 1}'
        step_name = step['keys'].get('name')
        for key in step['keys']:
            if key == 'shell' and step_name == SHELL_STEP and step['keys']['shell'] == 'bash':
                continue
            if key == 'uses' and index == 0 or key == 'with' and index == 0:
                continue
            if key not in STEP_KEYS or key in ('uses', 'with'):
                errors.append(f'{where} may not set `{key}` (allowed: {list(STEP_KEYS)}, `uses` and '
                              f'`with` only on the checkout step, `shell: bash` only on `{SHELL_STEP}`)')
    if steps:
        first = steps[0]
        if not CHECKOUT_USES.fullmatch(first['keys'].get('uses', '').split(' ', 1)[0]):
            errors.append(f'{label} step 1 must `uses:` actions/checkout@<40-hex commit SHA>, '
                          f'found {first["keys"].get("uses")!r}')
        if _step_with(first) != {'persist-credentials': 'false'} or len(first['body'].get('with', [])) != 1:
            errors.append(f'{label} step 1 `with:` must be exactly persist-credentials: false, '
                          f'found {_step_with(first)}')
    return errors


def _job_installs(job_text):
    """Each toolchain install of a job as (toolchain name, components or None)."""
    return [(name, options.get('--component', options.get('-c')))
            for _, kind, name, options in _rustup_pins(job_text) if kind == 'install']


def _make_recipe(makefile, target):
    """(prerequisites, recipe lines) of the single Makefile rule for `target`, or None when it
    is absent or defined more than once."""
    lines = makefile.splitlines()
    heads = [index for index, line in enumerate(lines) if re.match(rf'{re.escape(target)}\s*:(?!=)', line)]
    if len(heads) != 1:
        return None
    prerequisites = lines[heads[0]].partition(':')[2].split('#', 1)[0].split()
    recipe = []
    for line in lines[heads[0] + 1:]:
        if line.startswith('\t'):
            recipe.append(line[1:].strip())
        elif line.strip() and not line.startswith('#'):
            break
    return prerequisites, tuple(recipe)


def corpus_suites(makefile):
    """(suite names, errors) of the Makefile's `CORPUS_SUITES`: exactly one plain
    `CORPUS_SUITES :=` definition and no other assignment, so the checked text is what Make
    expands."""
    assignments = [line for line in makefile.splitlines() if CORPUS_ASSIGNMENT.match(line)]
    definitions = [match for match in map(CORPUS_DEFINITION.match, assignments) if match]
    if len(assignments) != 1 or len(definitions) != 1:
        return [], [f'Makefile must define CORPUS_SUITES exactly once, as a plain `CORPUS_SUITES :=` line '
                    f'with no `+=`, `?=`, `override` or other assignment, found {assignments}']
    value = definitions[0][1].split('#', 1)[0]
    if value.rstrip().endswith('\\'):
        return [], ['Makefile CORPUS_SUITES must be a single line without a continuation']
    return value.split(), []


def corpus_targets(root):
    """The names of the `test = false` `[[test]]` targets of every workspace member's Cargo.toml."""
    members = tomllib.loads((root / 'Cargo.toml').read_text(encoding='utf-8'))['workspace']['members']
    names = set()
    for member in members:
        manifest = tomllib.loads((root / member / 'Cargo.toml').read_text(encoding='utf-8'))
        names.update(target['name'] for target in manifest.get('test', []) if target.get('test') is False)
    return names


def validate_release_gate(root=None):
    """Check gate identity: release qualification and the pull-request `Verify` workflow run the
    same `make release-gate`, on the same two toolchains, in the same environment.

    * release-qualification.yml's `package` job has exactly the steps in `RELEASE_STEPS` plus
      the unnamed checkout, and only `Verify the release gate` runs `make`, as
      `make release-gate`, so no release-only step can sit beside the shared gate.
    * verify.yml's `release-gate` job runs `make release-gate`.
    * Both jobs install the same stable toolchain with `clippy,rustfmt` and the same nightly
      with `llvm-tools` (in either order), and their `env` blocks are identical.
    * Both are held to an allowlist: workflow top-level keys, job keys, an `env` of exactly
      `RUSTUP_TOOLCHAIN` equal to the job's stable install, step keys, an `actions/checkout`
      pinned by commit SHA with only `persist-credentials: false`, and no `GITHUB_ENV` or
      `GITHUB_PATH` write; verify.yml keeps a bare `pull_request:` trigger and a closed step list.
    * The Makefile `check` target depends on `lint` and `test`.
    * The Makefile `release-gate` target has no prerequisites and exactly the `GATE_RECIPE`
      lines in order, and `lint` is exactly the two `LINT_RECIPE` Clippy commands. Its second
      command lints the corpus suites, so `CORPUS_SUITES` must be defined once and equal the set
      of `test = false` test targets of the workspace members.

    verify.yml gets none of publish.yml's closed-shape rules. Toolchain pins across every
    workflow are checked separately by `validate_toolchain_pins`. Returns nothing; raises
    `SystemExit` listing every failure.
    """
    root = Path(root) if root else ROOT
    workflows = root / '.github/workflows'
    errors = []
    release_text = (workflows / 'release-qualification.yml').read_text(encoding='utf-8')
    verify_text = (workflows / 'verify.yml').read_text(encoding='utf-8')
    release = _job_text(release_text, 'package')
    verify = _job_text(verify_text, 'release-gate')
    if release is None:
        errors.append('release-qualification.yml must define a `package` job')
    if verify is None:
        errors.append('verify.yml must define a `release-gate` job')

    for name, text in (('release-qualification.yml', release_text), ('verify.yml', verify_text)):
        errors.extend(_workflow_shape_errors(name, text))
    errors.extend(_pull_request_errors('verify.yml', verify_text))
    if release is not None:
        errors.extend(_gate_job_errors('release-qualification.yml package', release))
    if verify is not None:
        errors.extend(_gate_job_errors('verify.yml release-gate', verify))

    if release is not None:
        steps = _load_check_docs().workflow_steps('jobs:\n' + release)
        for step in _job_steps(release) or []:
            if step['keys'].get('name') in RELEASE_STEPS[:2]:
                run = _step_run(step)
                if not (run and PLAIN_INSTALL.fullmatch(run)):
                    errors.append(f'release-qualification.yml step `{step["keys"]["name"]}` must run only '
                                  f'a toolchain install, found {run!r}')
        names = [name for _, name, _ in steps if name is not None]
        unnamed = [step for step in steps if step[1] is None]
        if names != list(RELEASE_STEPS):
            errors.append(f'release-qualification.yml package job must have exactly the named steps '
                          f'{list(RELEASE_STEPS)} (no release-only step outside the shared gate), found {names}')
        if len(unnamed) != 1 or unnamed[0][2] or steps[0][1] is not None:
            errors.append('release-qualification.yml package job must have exactly one unnamed step, '
                          'the checkout, first and running no command')
        code = [(number, line) for number, line in _code_lines(release) if re.search(r'\bmake\b', line)]
        if [line.strip() for _, line in code] != ['run: make release-gate']:
            errors.append('release-qualification.yml must run `make` exactly once, as `make release-gate`; '
                          f'found {[line.strip() for _, line in code]}')
        else:
            # `steps` count the prepended `jobs:` line, so job line N is step line N + 1.
            owner = [name for number, name, _ in steps if number <= code[0][0] + 1][-1:]
            if owner != [GATE_STEP]:
                errors.append(f'`make release-gate` must run in the `{GATE_STEP}` step, not {owner}')
    if verify is not None:
        runs = [re.sub(r'^-\s+', '', line.strip()) for _, line in _code_lines(verify)
                if re.search(r'\bmake\b', line)]
        if runs != ['run: make release-gate']:
            errors.append('verify.yml release-gate job must run `make release-gate` as its only make '
                          f'command; found {runs}')
        verify_steps = _job_steps(verify) or []
        runs = [_step_run(step) for step in verify_steps]
        if (len(verify_steps) != 4 or runs[0] is not None or runs[3] != 'make release-gate'
                or not all(run and PLAIN_INSTALL.fullmatch(run) for run in runs[1:3])):
            errors.append('verify.yml release-gate job must have exactly the steps: checkout, one stable '
                          'toolchain install, one nightly toolchain install, `make release-gate` '
                          f'(a gate step may be named or unnamed); found commands {runs}')

    if release is not None and verify is not None:
        if _job_env(release) != _job_env(verify):
            errors.append(f'the release-gate job env {_job_env(verify)} must equal release '
                          f'qualification\'s package job env {_job_env(release)}')
        for label, text in (('release-qualification.yml package', release), ('verify.yml release-gate', verify)):
            installs = _job_installs(text)
            stable = [i for i in installs if not i[0].startswith('nightly')]
            nightly = [i for i in installs if i[0].startswith('nightly')]
            if len(stable) != 1 or stable[0][1] != STABLE_COMPONENTS:
                errors.append(f'{label} job must install one stable toolchain with --component '
                              f'{STABLE_COMPONENTS}, found {stable}')
            if len(nightly) != 1 or nightly[0][1] != NIGHTLY_COMPONENTS:
                errors.append(f'{label} job must install one nightly toolchain with --component '
                              f'{NIGHTLY_COMPONENTS}, found {nightly}')
        def by_kind(text):
            installs = _job_installs(text)
            return {kind: sorted(i for i in installs if i[0].startswith('nightly') == (kind == 'nightly'))
                    for kind in ('stable', 'nightly')}
        if by_kind(release) != by_kind(verify):
            errors.append(f'the release-gate job must install the same toolchains as release '
                          f'qualification: {_job_installs(verify)} differs from {_job_installs(release)}')

    makefile = (root / 'Makefile').read_text(encoding='utf-8')
    gate_rule = _make_recipe(makefile, 'release-gate')
    if gate_rule is None:
        errors.append('Makefile must define the `release-gate` target exactly once')
    else:
        if gate_rule[0]:
            errors.append(f'Makefile release-gate must have no prerequisites, found {gate_rule[0]}')
        if gate_rule[1] != GATE_RECIPE:
            errors.append(f'Makefile release-gate recipe must be exactly {list(GATE_RECIPE)} in order, '
                          f'found {list(gate_rule[1])}')
    check_rule = _make_recipe(makefile, 'check')
    if check_rule is None:
        errors.append('Makefile must define the `check` target exactly once')
    else:
        for required in ('lint', 'test'):
            if required not in check_rule[0]:
                errors.append(f'Makefile check must depend on `{required}` (Clippy and the tests are '
                              f'part of the gate), found prerequisites {check_rule[0]}')
    # The Makefile must not change the toolchain or ignore failures behind the workflows'
    # backs: `unexport RUSTUP_TOOLCHAIN` would silently fall back to rust-toolchain.toml's
    # nightly in both lanes, and `.IGNORE` or `-i` in MAKEFLAGS would let a failing gate pass.
    for number, line in enumerate(makefile.splitlines(), 1):
        code = line.split('#', 1)[0]
        if 'RUSTUP_TOOLCHAIN' in code or re.search(r'\bunexport\b', code):
            errors.append(f'Makefile:{number}: must not set, export or unexport RUSTUP_TOOLCHAIN; the '
                          f'toolchain comes from the workflows')
        if re.match(r'\s*\.IGNORE\b', code):
            errors.append(f'Makefile:{number}: must not declare .IGNORE; a failing gate target must fail')
        if re.match(r'\s*(?:override\s+)?(?:export\s+)?MAKEFLAGS\b', code) and \
                re.search(r'(?:^|\s|=)(?:-[a-zA-Z]*i[a-zA-Z]*|--ignore-errors)\b', code.split('=', 1)[-1]):
            errors.append(f'Makefile:{number}: MAKEFLAGS must not ignore errors (-i/--ignore-errors)')
    lint_rule = _make_recipe(makefile, 'lint')
    if lint_rule is None or lint_rule[1] != LINT_RECIPE:
        errors.append(f'Makefile lint recipe must be exactly {list(LINT_RECIPE)}, '
                      f'found {list(lint_rule[1]) if lint_rule else None}')
    suites, suite_errors = corpus_suites(makefile)
    errors.extend(suite_errors)
    if not suite_errors:
        declared = corpus_targets(root)
        if len(suites) != len(set(suites)) or set(suites) != declared:
            errors.append(f'Makefile CORPUS_SUITES must equal the `test = false` test targets of the '
                          f'workspace crates (they are linted by `make lint` and run by `make corpus`): '
                          f'missing {sorted(declared - set(suites))}, unknown {sorted(set(suites) - declared)}, '
                          f'duplicated {sorted(s for s in set(suites) if suites.count(s) > 1)}')

    if errors:
        raise SystemExit('\n'.join(errors))
    print('Release gate identity verified: release qualification and the Verify release-gate job '
          'both run `make release-gate` on the same toolchains and environment.')


def _logical_lines(text):
    """The (first line number, text) pairs of the non-comment lines of `text`, with backslash
    continuations joined and trailing ` #` comments removed."""
    logical, pending = [], None
    for number, line in enumerate(text.splitlines(), 1):
        if pending is None and line.lstrip().startswith('#'):
            continue
        line = re.sub(r'\s+#.*$', '', line)
        if pending is None:
            pending = (number, '')
        joined = pending[1] + ' ' + line.rstrip().removesuffix('\\').strip()
        if line.rstrip().endswith('\\'):
            pending = (pending[0], joined)
            continue
        logical.append((pending[0], joined.strip()))
        pending = None
    if pending is not None:
        logical.append((pending[0], pending[1].strip()))
    return logical


def _unquote(token):
    return token.strip('\'"')


def _rustup_pins(text):
    """The toolchain names that the commands of `text` select, as (line number, kind, name,
    options) with `kind` one of `install`, `default`, `override`, `cargo` or `inline` and
    `options` the flags of an install (`{'--component': 'clippy,rustfmt'}`).

    A toolchain install (`rustup toolchain install` or `rustup install`) is read only after the
    `rustup` token: `apt-get install` and other installers are not. A value-taking flag in
    `VALUE_FLAGS` consumes the next argument, `--opt=value` and boolean flags consume nothing,
    and every remaining positional argument of an install is a pin. Also read are
    `rustup default X`, `rustup override set X`, `cargo +X` and an inline `RUSTUP_TOOLCHAIN=X`."""
    pins = []
    for number, line in _logical_lines(text):
        tokens = SHELL_TOKEN.findall(line)
        for index, token in enumerate(tokens):
            rest = tokens[index + 1:]
            if token.startswith('RUSTUP_TOOLCHAIN='):
                pins.append((number, 'inline', _unquote(token.partition('=')[2]), {}))
            elif token == 'cargo' and rest[:1] and rest[0].startswith('+'):
                pins.append((number, 'cargo', _unquote(rest[0][1:]), {}))
            elif token == 'rustup':
                verb = rest[:2] if rest[:1] == ['toolchain'] else rest[:1]
                after = rest[len(verb):]
                if verb in (['toolchain', 'install'], ['install']):
                    options, position = {}, 0
                    while position < len(after) and not re.fullmatch(r'[;&|]+', after[position]):
                        argument = after[position]
                        if argument in VALUE_FLAGS:
                            options[argument] = _unquote(after[position + 1]) if position + 1 < len(after) else ''
                            position += 2
                            continue
                        if argument.startswith('-'):
                            flag, _, value = argument.partition('=')
                            if value:
                                options[flag] = _unquote(value)
                        else:
                            pins.append((number, 'install', _unquote(argument), options))
                        position += 1
                elif verb == ['default'] and after:
                    pins.append((number, 'default', _unquote(after[0]), {}))
                elif verb == ['override'] and after[:1] == ['set'] and after[1:2]:
                    pins.append((number, 'override', _unquote(after[1]), {}))
    return pins


def toolchain_pins(text):
    """Every toolchain pin of a workflow as (line number, toolchain name): each
    `RUSTUP_TOOLCHAIN:` value and every pin `_rustup_pins` reads, ignoring profile and
    components. The names are returned as written; `validate_toolchain_pins` judges them."""
    pins = []
    for number, line in _code_lines(text):
        match = ENV_PIN.search(line)
        if match:
            pins.append((number, _plain_value(match[1])))
    pins.extend((number, name) for number, _, name, _ in _rustup_pins(text))
    return sorted(pins, key=lambda pin: pin[0])


def stable_pins(text):
    """The distinct stable (not `nightly`) toolchain names of a workflow, sorted."""
    return sorted({name for _, name in toolchain_pins(text) if not name.startswith('nightly')})


def workflow_files(root):
    """Every workflow file of `root`, sorted: a new workflow cannot hide a pin."""
    directory = Path(root) / '.github/workflows'
    return sorted(path for path in directory.iterdir() if path.suffix in ('.yml', '.yaml'))


def validate_toolchain_pins(root=None):
    """Check that each toolchain pin has one source across every file in `.github/workflows/`.
    A pin must be literal (`${{ ... }}` and `$VAR` are errors) and match `PIN_FORMAT`. A name
    starting with `nightly` is a nightly occurrence; any other is stable. Every stable
    occurrence is identical and its major.minor equals Cargo.toml's `rust-version`; every
    nightly occurrence equals rust-toolchain.toml's `channel`. Raises `SystemExit` listing
    every failure."""
    root = Path(root) if root else ROOT
    errors = []
    rust_version = re.search(r'^rust-version\s*=\s*"([^"]+)"', (root / 'Cargo.toml').read_text(encoding='utf-8'), re.M)
    channel = re.search(r'^channel\s*=\s*"([^"]+)"', (root / 'rust-toolchain.toml').read_text(encoding='utf-8'), re.M)
    if rust_version is None:
        errors.append('Cargo.toml has no rust-version')
    if channel is None:
        errors.append('rust-toolchain.toml has no channel')
    stable, nightly = [], []
    for path in workflow_files(root):
        text = path.read_text(encoding='utf-8')
        for number, name in toolchain_pins(text):
            where = f'{path.name}:{number}'
            if '$' in name:
                errors.append(f'{where}: toolchain pins must be literal, found {name!r}')
            elif not PIN_FORMAT.fullmatch(name):
                errors.append(f'{where}: toolchain pin {name!r} must be nightly-YYYY-MM-DD or X.Y.Z')
            else:
                (nightly if name.startswith('nightly') else stable).append((where, name))
    if not stable:
        errors.append('no stable toolchain pin found in the workflows')
    distinct = sorted({name for _, name in stable})
    if len(distinct) > 1:
        errors.append(f'stable toolchain pins differ: {[f"{where} {name}" for where, name in stable]}')
    if rust_version is not None:
        for where, name in stable:
            major_minor = re.match(r'\d+\.\d+', name)
            if major_minor is None or major_minor[0] != rust_version[1]:
                errors.append(f'{where}: stable pin {name} does not match Cargo.toml rust-version {rust_version[1]}')
    if channel is not None:
        for where, name in nightly:
            if name != channel[1]:
                errors.append(f'{where}: nightly pin {name} does not match rust-toolchain.toml channel {channel[1]}')
    if errors:
        raise SystemExit('\n'.join(errors))
    print(f'Toolchain pins verified: {len(stable)} stable occurrences match rust-version {rust_version[1]}, '
          f'{len(nightly)} nightly occurrences match {channel[1]}.')


def main():
    if len(sys.argv) == 4 and sys.argv[1] == '--preflight-owners' and sys.argv[2] == '--user-agent':
        run_preflight(sys.argv[3])
        return
    metadata = json.loads(subprocess.check_output(
        ['cargo', 'metadata', '--no-deps', '--format-version', '1', '--locked', '--offline'],
        encoding='utf-8'))
    packages = {package['name']: package for package in metadata['packages']}
    publishable = {name for name, package in packages.items() if package.get('publish') != []}
    if publishable != set(RELEASE_ORDER):
        raise SystemExit(f'release set differs from publishable workspace: '
                         f'missing={sorted(publishable - set(RELEASE_ORDER))}, '
                         f'extra={sorted(set(RELEASE_ORDER) - publishable)}')
    if len(RELEASE_ORDER) != len(set(RELEASE_ORDER)):
        raise SystemExit('release order contains duplicate crates')
    versions = {package['version'] for package in packages.values()}
    if len(versions) != 1:
        raise SystemExit(f'workspace versions differ: {sorted(versions)}')
    version = versions.pop()
    if len(sys.argv) == 3 and sys.argv[1] == '--tag':
        if sys.argv[2] != f'rust-v{version}':
            raise SystemExit(f'tag {sys.argv[2]} does not match rust-v{version}')
    elif len(sys.argv) != 1:
        raise SystemExit('usage: check-release.py [--tag rust-vVERSION | --preflight-owners --user-agent AGENT]')
    position = {name: index for index, name in enumerate(RELEASE_ORDER)}
    for name in RELEASE_ORDER:
        for dependency in packages[name]['dependencies']:
            needed = dependency['name']
            if needed in packages:
                if position[needed] >= position[name]:
                    raise SystemExit(f'{name}: {dependency["kind"] or "normal"} dependency '
                                     f'{needed} does not precede it in release order')
                if dependency['req'] != f'^{version}':
                    raise SystemExit(f'{name}: {needed} has version requirement '
                                     f'{dependency["req"]}, expected ^{version}')
    print(f'Release set and dependency order verified for {version}: '
          + ', '.join(RELEASE_ORDER))
    validate_publish_workflow()
    validate_release_gate()
    validate_toolchain_pins()


if __name__ == '__main__':
    main()
