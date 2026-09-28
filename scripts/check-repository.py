#!/usr/bin/env python3
# SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
# SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0
"""Verify attribution, frozen upstream data, and the zero-dependency core boundary."""
import hashlib
import json
from pathlib import Path
import subprocess
import tomllib

ROOT = Path(__file__).resolve().parent.parent
errors = []
core = tomllib.loads((ROOT / 'crates/core/Cargo.toml').read_text())
if core.get('dependencies'):
    errors.append('purrassist-core must have zero external or workspace runtime dependencies')
for target in core.get('target', {}).values():
    if target.get('dependencies'):
        errors.append('purrassist-core cannot hide dependencies behind target selection')

for source in ['crates', 'scripts']:
    for path in (ROOT / source).rglob('*'):
        if not path.is_file() or path.suffix not in {'.rs', '.py', '.js', '.toml', '.sh'}:
            continue
        if '/tests/data/' in str(path) or '/schema/metaschemas/' in str(path):
            continue
        header = '\n'.join(path.read_text().splitlines()[:5])
        if 'SPDX-License-Identifier:' not in header or 'SPDX-FileCopyrightText:' not in header:
            errors.append(f'{path.relative_to(ROOT)}: missing attribution')

for directory in ['crates/core/src/schema/metaschemas', 'crates/core/tests/data/json-schema', 'crates/core/tests/data/markdown', 'crates/core/tests/data/test262']:
    root = ROOT / directory
    manifest = json.loads((root / 'manifest.json').read_text())
    entries = manifest.get('files', manifest)
    for name, entry in entries.items():
        expected = entry['sha256'] if isinstance(entry, dict) else entry
        path = root / name
        if not path.is_file() or hashlib.sha256(path.read_bytes()).hexdigest() != expected:
            errors.append(f'{directory}/{name}: frozen source digest mismatch')
unicode_manifest = json.loads((ROOT / 'vendor/unicode/manifest.json').read_text())
fixture = ROOT / 'crates/core/tests/data/unicode/NormalizationTest.txt'
if hashlib.sha256(fixture.read_bytes()).hexdigest() != unicode_manifest['NormalizationTest.txt']['sha256']:
    errors.append('Unicode normalization fixture differs from its pinned upstream source')
confusables_fixture = ROOT / 'crates/core/tests/data/unicode/confusables.txt'
if hashlib.sha256(confusables_fixture.read_bytes()).hexdigest() != unicode_manifest['confusables.txt']['sha256']:
    errors.append('UTS #39 confusables fixture differs from its pinned upstream source')
generated = ROOT / 'vendor/unicode/generated.json'
for name, expected in json.loads(generated.read_text()).items():
    if hashlib.sha256((ROOT / name).read_bytes()).hexdigest() != expected:
        errors.append(f'{name}: generated Unicode artifact digest mismatch; regenerate and review')

entity_manifest = json.loads((ROOT / 'vendor/entities/manifest.json').read_text())
for name, entry in entity_manifest['files'].items():
    if hashlib.sha256((ROOT / name).read_bytes()).hexdigest() != entry['sha256']:
        errors.append(f'{name}: entity source or notice digest mismatch')
for name, expected in json.loads((ROOT / 'vendor/entities/generated.json').read_text()).items():
    if hashlib.sha256((ROOT / name).read_bytes()).hexdigest() != expected:
        errors.append(f'{name}: generated entity table digest mismatch')

metadata = json.loads(subprocess.check_output(
    ['cargo', 'metadata', '--format-version', '1', '--locked'], cwd=ROOT, text=True))
inventory = json.loads((ROOT / 'vendor/dependencies.json').read_text())
actual = {f"{package['name']}@{package['version']}" for package in metadata['packages'] if package['source'] is not None}
if actual != set(inventory['cargo_packages']):
    errors.append(f'dependency inventory differs: added={sorted(actual-set(inventory["cargo_packages"]))}, removed={sorted(set(inventory["cargo_packages"])-actual)}')
details = {f"{entry['name']}@{entry['version']}": entry for entry in inventory['cargo_details']}
if set(details) != actual:
    errors.append('dependency provenance records must cover every resolved external package')
for identity, entry in details.items():
    if not entry.get('notices'):
        errors.append(f'{identity}: missing license notices')
    for name in entry.get('notices', []):
        path = ROOT / name
        if not path.is_file() or hashlib.sha256(path.read_bytes()).hexdigest() != entry.get('notice_sha256', {}).get(name):
            errors.append(f'{identity}: license notice missing or changed: {name}')
for entry in json.loads((ROOT / 'vendor/tooling.json').read_text()):
    for name, expected in entry['notices'].items():
        if hashlib.sha256((ROOT / name).read_bytes()).hexdigest() != expected:
            errors.append(f'tooling notice changed: {name}')
for crate in (ROOT / 'crates').iterdir():
    if not (crate / 'Cargo.toml').exists():
        continue
    for name in ['LICENSE-MIT', 'LICENSE-APACHE', 'LICENSE-MULAN']:
        if not (crate / name).is_file() or (crate / name).read_bytes() != (ROOT / name).read_bytes():
            errors.append(f'{crate.name}: package license missing or differs: {name}')
if errors:
    raise SystemExit('\n'.join(errors))
print('Repository attribution, frozen schema data, dependency inventory, and core boundary verified.')
