#!/usr/bin/env python3
# SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
# SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0
"""Audit locally built crate archives without claiming registry publication."""
import hashlib
import json
from pathlib import Path, PurePosixPath
import subprocess
import sys
import tarfile
import tomllib

ROOT = Path(__file__).resolve().parent.parent
metadata = json.loads(subprocess.check_output(['cargo', 'metadata', '--no-deps', '--format-version', '1'], cwd=ROOT))
directory = Path(sys.argv[1]) if len(sys.argv) > 1 else Path(metadata['target_directory']) / 'package'
report = []
for package in metadata['packages']:
    identity = f"{package['name']}-{package['version']}"
    archive = directory / f'{identity}.crate'
    crate_root = Path(package['manifest_path']).parent
    with tarfile.open(archive) as source:
        members = {}
        for member in source.getmembers():
            if not member.isfile() or not member.name.startswith(identity + '/'):
                raise SystemExit(f'{identity}: unexpected archive entry {member.name}')
            name = member.name.removeprefix(identity + '/')
            path = PurePosixPath(name)
            if (not name or path.is_absolute() or '..' in path.parts or '\\' in name
                    or path.as_posix() != name or name in members):
                raise SystemExit(f'{identity}: unsafe or repeated member path {name}')
            members[name] = member
        for required in ('README.md', 'LICENSE-MIT', 'LICENSE-APACHE', 'LICENSE-MULAN', 'Cargo.toml', 'Cargo.toml.orig'):
            if required not in members:
                raise SystemExit(f'{identity}: missing {required}')
        generated = {'Cargo.toml', 'Cargo.lock', '.cargo_vcs_info.json'}
        for name, member in members.items():
            local = crate_root / ('Cargo.toml' if name == 'Cargo.toml.orig' else name)
            if name not in generated:
                if not local.is_file():
                    raise SystemExit(f'{identity}: packaged file has no checkout source: {name}')
                if source.extractfile(member).read() != local.read_bytes():
                    raise SystemExit(f'{identity}: stale packaged source {name}')
        for directory_name in ('src', 'tests', 'examples', 'benches'):
            for local in (crate_root / directory_name).rglob('*'):
                if local.is_file() and str(local.relative_to(crate_root)) not in members:
                    raise SystemExit(f'{identity}: omitted {directory_name} file {local.relative_to(crate_root)}')
        manifest = tomllib.loads(source.extractfile(members['Cargo.toml']).read().decode())
        if manifest['package']['license'] != 'MIT OR Apache-2.0 OR MulanPSL-2.0':
            raise SystemExit(f'{identity}: unexpected first-party license')
        if package['name'] == 'purrassist-core':
            if manifest.get('dependencies') or any(target.get('dependencies') for target in manifest.get('target', {}).values()):
                raise SystemExit('packaged core gained runtime dependencies')
            for notice in ('src/text/LICENSE-UNICODE', 'src/markdown/inline/LICENSE-WHATWG', 'src/schema/metaschemas/LICENSE', 'tests/data/json-schema/LICENSE', 'tests/data/unicode/LICENSE', 'tests/data/markdown/LICENSE-commonmark', 'tests/data/markdown/LICENSE-gfm'):
                if notice not in members:
                    raise SystemExit(f'{identity}: missing upstream notice {notice}')
    report.append({'package': identity, 'bytes': archive.stat().st_size,
                   'sha256': hashlib.sha256(archive.read_bytes()).hexdigest(), 'files': len(members)})
print(json.dumps(report, indent=2))
