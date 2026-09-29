#!/usr/bin/env python3
# SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
# SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0
"""Package in a commit-scoped Cargo home so old same-version crates cannot mask changes."""

import json
import os
from pathlib import Path
import subprocess
import sys

ROOT = Path(__file__).resolve().parent.parent
revision = subprocess.check_output(
    ['git', 'rev-parse', '--verify', 'HEAD'], cwd=ROOT, text=True).strip()
environment = os.environ.copy()
environment['CARGO_HOME'] = str(ROOT / '.cache' / 'package-homes' / revision)
# Packaging does not publish and does not need registry credentials.
for name in tuple(environment):
    if name in {'CARGO_TOKEN', 'CARGO_REGISTRY_TOKEN'} or (
        name.startswith('CARGO_REGISTRIES_') and name.endswith('_TOKEN')
    ):
        del environment[name]

# Package exactly the publishable release set, not every workspace member.
# `cargo package --workspace` would also try to package and *verify-build*
# `publish = false` tooling crates (e.g. `crates/wasm-harness`), which exist
# to exercise a different target (wasm32-unknown-unknown) and are not part
# of the release this script audits; a build failure in one of those would
# otherwise block packaging the crates that actually ship. `check-release.py`
# already proves the publishable set is exactly the five release crates in
# dependency order, so this mirrors that filter directly from `cargo metadata`
# instead of hardcoding the crate list a second time.
metadata = json.loads(subprocess.check_output(
    ['cargo', 'metadata', '--no-deps', '--format-version', '1', '--locked'],
    cwd=ROOT, env=environment, text=True))
publishable = sorted(
    package['name'] for package in metadata['packages'] if package.get('publish') != [])
package_args = [flag for name in publishable for flag in ('-p', name)]

subprocess.run(['cargo', 'package', *package_args, '--locked'],
               cwd=ROOT, env=environment, check=True)
subprocess.run([sys.executable, 'scripts/check-packages.py'],
               cwd=ROOT, env=environment, check=True)
