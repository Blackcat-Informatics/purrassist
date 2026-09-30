#!/usr/bin/env python3
# SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
# SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0
"""Install one digest-pinned Linux qualification tool in the ignored cache.

Print only its bin directory on stdout, suitable for GitHub's GITHUB_PATH.
Downloads and build logs stay in .cache; nothing is installed system-wide.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import platform
import subprocess
import sys
import tarfile
import urllib.request

ROOT = Path(__file__).resolve().parent.parent


def install(name):
    if sys.platform != 'linux' or platform.machine() != 'x86_64':
        raise SystemExit('qualification installer requires Linux x86_64')
    inventory = json.loads((ROOT / 'vendor/tooling.json').read_text())
    entry = next(item for item in inventory if item['name'] == name)
    directory = ROOT / '.cache' / 'qualification-tools' / f"{name}-{entry['version']}"
    directory.mkdir(parents=True, exist_ok=True)
    url = entry.get('binary_source', entry['source'])
    expected = entry['binary_sha256'] if 'binary_source' in entry else entry['source_sha256']
    archive = directory / 'source.tar'
    if not archive.exists():
        with urllib.request.urlopen(url, timeout=60) as response:
            archive.write_bytes(response.read())
    if hashlib.sha256(archive.read_bytes()).hexdigest() != expected:
        raise SystemExit(f'{name}: archive digest mismatch')
    with tarfile.open(archive) as package:
        package.extractall(directory, filter='data')
    if name == 'node':
        binary_directory = directory / f"node-v{entry['version']}-linux-x64" / 'bin'
        expected_version = f"v{entry['version']}"
    else:
        source = directory / f"valgrind-{entry['version']}"
        prefix = directory / 'installed'
        with (directory / 'build.log').open('w') as log:
            commands = [
                ['./configure', f'--prefix={prefix}', '--enable-only64bit'],
                ['make', '-j', str(min(os.cpu_count() or 1, 4))],
                ['make', 'install'],
            ]
            for command in commands:
                result = subprocess.run(command, cwd=source, stdout=log, stderr=subprocess.STDOUT)
                if result.returncode:
                    raise SystemExit(f'{name}: {command[0]} failed; see {directory / "build.log"}')
        binary_directory = prefix / 'bin'
        expected_version = f"valgrind-{entry['version']}"
    version = subprocess.check_output([str(binary_directory / name), '--version'], text=True).strip()
    if version != expected_version:
        raise SystemExit(f'{name}: installed version mismatch: {version}')
    return binary_directory


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('tool', choices=('node', 'valgrind'))
    print(install(parser.parse_args().tool))


if __name__ == '__main__':
    main()
