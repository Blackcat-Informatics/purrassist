#!/usr/bin/env python3
# SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
# SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0
"""Check the complete publish set and dependency order without publishing."""

import http.client
import json
from pathlib import Path
import re
import subprocess
import sys
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
    id_token = re.compile(r'^[^#\n]*["\']?\bid-token["\']?\s*:', re.M)
    if len(id_token.findall(text)) != len(id_token.findall(publish_job_body)):
        errors.append('only the publish job may declare an id-token permission')
    if re.search(r'^[^#\n]*\bpermissions:\s*["\']?write-all\b', text, re.M):
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


if __name__ == '__main__':
    main()
