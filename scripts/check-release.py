#!/usr/bin/env python3
# SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
# SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0
"""Check the complete publish set and dependency order without publishing."""

import json
from pathlib import Path
import re
import subprocess
import sys

ROOT = Path(__file__).resolve().parent.parent

# The sole release order. Both normal and dev dependencies must precede users:
# cargo publish verifies the packaged crate against the registry.
RELEASE_ORDER = (
    'purrassist-core',
    'purrassist-syntax',
    'purrassist',
    'purrassist-host',
    'purrassist-cli',
)


def validate_publish_workflow():
    """Validate `.github/workflows/publish.yml` without a YAML dependency.

    Checked by plain text/regex against the exact shape this project's
    single publish workflow uses, mirroring the text-based checks the rest
    of this repository's hygiene scripts already use instead of adding a
    YAML-parsing dependency for one file this script itself authors no
    freeform structure for.
    """
    path = ROOT / '.github/workflows/publish.yml'
    text = path.read_text()
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
        # Bound the job body to the next top-level (2-space-indented) job key.
        match = re.search(r'\n  [A-Za-z_-]+:\n', publish_job[1])
        publish_job_body = publish_job[1][:match.start()] if match else publish_job[1]
        if not re.search(r'^\s*environment:\s*crates-io\s*$', publish_job_body, re.M):
            errors.append('publish job must run in the crates-io environment')
        if not re.search(r'^\s*id-token:\s*write\s*$', publish_job_body, re.M):
            errors.append('publish job must declare permissions: id-token: write')
        if not re.search(r'^\s*contents:\s*read\s*$', publish_job_body, re.M):
            errors.append('publish job must declare permissions: contents: read')
        if not re.search(r'^\s*needs:\s*gate\s*$', publish_job_body, re.M):
            errors.append('publish job must depend on a gate job that checks release qualification')

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
        tooling = json.loads((ROOT / 'vendor/tooling.json').read_text())
        registered = {entry['version'] for entry in tooling if entry['name'] == 'crates-io-auth-action'}
        if auth_actions[0] not in registered:
            errors.append('publish.yml crates-io-auth-action SHA is not registered in vendor/tooling.json')

    loop_match = re.search(r'for crate in ([^;\n]+); do', text)
    if not loop_match:
        errors.append('publish.yml must publish crates in a single explicit dependency-ordered sequence')
    elif tuple(loop_match[1].split()) != RELEASE_ORDER:
        errors.append(f'publish.yml publish order {tuple(loop_match[1].split())} does not match {RELEASE_ORDER}')

    if not re.search(r'cargo publish -p "\$crate" --locked', text):
        errors.append('publish.yml must run cargo publish --locked (full verification) per crate')

    if not re.search(r'scripts/verify-published\.py --version', text):
        errors.append('publish.yml must run scripts/verify-published.py --version after publishing')

    if errors:
        raise SystemExit('\n'.join(errors))
    print('publish.yml verified: tag trigger, crates-io environment, id-token permission, '
          'qualification gate, SHA-pinned actions, dependency order, and verify-published step.')


def main():
    metadata = json.loads(subprocess.check_output(
        ['cargo', 'metadata', '--no-deps', '--format-version', '1', '--locked', '--offline'],
        text=True))
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
        raise SystemExit('usage: check-release.py [--tag rust-vVERSION]')
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
