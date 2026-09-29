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


def extract(source: tarfile.TarFile, member: tarfile.TarInfo, identity: str) -> bytes:
    """`TarFile.extractfile` returns `None` for non-regular members (already
    excluded above by the `member.isfile()` filter, but not to a type
    checker); fail with a precise, member-naming report instead of an
    `AttributeError` if that invariant is ever violated."""
    handle = source.extractfile(member)
    if handle is None:
        raise SystemExit(f'{identity}: archive member {member.name} is not a regular file')
    return handle.read()
metadata = json.loads(subprocess.check_output(['cargo', 'metadata', '--no-deps', '--format-version', '1'], cwd=ROOT))
directory = Path(sys.argv[1]) if len(sys.argv) > 1 else Path(metadata['target_directory']) / 'package'
report = []
for package in metadata['packages']:
    if package.get('publish') == []:
        # `publish = false` tooling crates (e.g. `crates/wasm-harness`) are
        # not part of the release this script audits and package-workspace.py
        # never packages them; mirrors the same filter check-release.py uses
        # to prove the publishable set.
        continue
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
                if extract(source, member, identity) != local.read_bytes():
                    raise SystemExit(f'{identity}: stale packaged source {name}')
        for directory_name in ('src', 'tests', 'examples', 'benches'):
            for local in (crate_root / directory_name).rglob('*'):
                if local.is_file() and str(local.relative_to(crate_root)) not in members:
                    raise SystemExit(f'{identity}: omitted {directory_name} file {local.relative_to(crate_root)}')
        manifest = tomllib.loads(extract(source, members['Cargo.toml'], identity).decode())
        if manifest['package']['license'] != 'MIT OR Apache-2.0 OR MulanPSL-2.0':
            raise SystemExit(f'{identity}: unexpected first-party license')
        if package['name'] == 'purrassist-core':
            if manifest.get('dependencies') or any(target.get('dependencies') for target in manifest.get('target', {}).values()):
                raise SystemExit('packaged core gained runtime dependencies')
        # The registered notice set for this crate is exactly the upstream
        # LICENSE* files checked into its own tree (excluding the top-level
        # first-party LICENSE-MIT/APACHE/MULAN triple, already required
        # above): these are the vendored data notices (Unicode, JSON Schema
        # meta-schemas/test suite, CommonMark/GFM, WHATWG, test262) recorded
        # in `vendor/dependencies.json` and `docs/DEPENDENCIES.md`, physically
        # bundled inside the crate directory that ships the data they cover.
        # Deriving this set from the checkout instead of a hand-maintained
        # literal list means an upstream notice added anywhere in the tree
        # (e.g. the test262 fixture's LICENSE, previously unchecked here)
        # is caught automatically, in every crate, not only in the one this
        # check happened to be written against.
        registered = {
            str(path.relative_to(crate_root))
            for path in crate_root.rglob('LICENSE*')
            if path.is_file() and path.parent != crate_root and 'target' not in path.parts
        }
        packaged = {
            name for name in members
            if PurePosixPath(name).name.startswith('LICENSE')
            and name not in {'LICENSE-MIT', 'LICENSE-APACHE', 'LICENSE-MULAN'}
        }
        if packaged != registered:
            raise SystemExit(
                f'{identity}: packaged notice set differs from the registered notice set: '
                f'missing={sorted(registered - packaged)}, extra={sorted(packaged - registered)}')
        for notice in registered:
            local = crate_root / notice
            if extract(source, members[notice], identity) != local.read_bytes():
                raise SystemExit(f'{identity}: packaged notice {notice} differs from its checkout source')
    report.append({'package': identity, 'bytes': archive.stat().st_size,
                   'sha256': hashlib.sha256(archive.read_bytes()).hexdigest(), 'files': len(members)})
print(json.dumps(report, indent=2))
