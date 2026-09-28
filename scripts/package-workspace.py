#!/usr/bin/env python3
# SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
# SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0
"""Package in a commit-scoped Cargo home so old same-version crates cannot mask changes."""

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

subprocess.run(['cargo', 'package', '--workspace', '--locked'],
               cwd=ROOT, env=environment, check=True)
subprocess.run([sys.executable, 'scripts/check-packages.py'],
               cwd=ROOT, env=environment, check=True)
