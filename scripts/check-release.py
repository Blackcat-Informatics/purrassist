#!/usr/bin/env python3
# SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
# SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0
"""Check the complete publish set and dependency order without publishing."""

import json
import subprocess
import sys

# The sole release order. Both normal and dev dependencies must precede users:
# cargo publish verifies the packaged crate against the registry.
RELEASE_ORDER = (
    'purrassist-core',
    'purrassist-syntax',
    'purrassist',
    'purrassist-host',
    'purrassist-cli',
)


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


if __name__ == '__main__':
    main()
