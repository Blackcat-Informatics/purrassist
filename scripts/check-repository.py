#!/usr/bin/env python3
# SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
# SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0
"""Verify attribution, frozen upstream data, and the zero-dependency core boundary."""
import hashlib
import json
from pathlib import Path
import re
import subprocess
import tomllib

ROOT = Path(__file__).resolve().parent.parent
BASELINE = 'scripts/data/dependency-baseline.json'
CORE_PACKAGE = 'purrassist-core'
NETWORK_DENYLIST = re.compile(
    r'(hyper.*|reqwest|h2|h3.*|http|http-body.*|tokio.*|mio|async-std|async-io|smol|socket2'
    r'|rustls.*|native-tls|openssl.*|ureq|curl.*|isahc|surf|quinn.*|tonic|axum|actix.*|warp)')
AMBIENT_TOKENS = [
    r'std::net\b', r'\bTcpStream\b', r'\bTcpListener\b', r'\bUdpSocket\b', r'\bToSocketAddrs\b',
    r'std::process\b', r'std::fs\b', r'std::env\b', r'\bSystemTime\b', r'\bInstant\b',
    r'\bRandomState\b', r'\bHashMap\b', r'\bHashSet\b', r'\bDefaultHasher\b', r'std::thread\b',
    r'\bstdin\s*\(', r'\bstdout\s*\(', r'\bstderr\s*\(',
]
AMBIENT_PATTERN = re.compile('|'.join(f'(?:{token})' for token in AMBIENT_TOKENS))
AMBIENT_GROUP = re.compile(r'std\s*::\s*\{([^{}]*(?:\{[^{}]*\}[^{}]*)*)\}')
AMBIENT_GROUP_MEMBER = re.compile(r'(?<![\w:])(net|process|fs|env|thread)\b')


def _package_names(metadata):
    return {package['id']: package['name'] for package in metadata['packages']}


def _closure_paths(metadata, root_id, kinds):
    """Map every package reachable from `root_id` over edges whose kind is in `kinds`
    ('normal', 'build', 'dev') to the shortest name path from the root."""
    names = _package_names(metadata)
    nodes = {node['id']: node for node in metadata['resolve']['nodes']}
    paths = {root_id: [names[root_id]]}
    queue = [root_id]
    while queue:
        current = queue.pop(0)
        for dep in nodes.get(current, {}).get('deps', []):
            edge_kinds = {kind.get('kind') or 'normal' for kind in dep.get('dep_kinds', [])}
            if dep['pkg'] not in paths and edge_kinds & kinds:
                paths[dep['pkg']] = paths[current] + [names[dep['pkg']]]
                queue.append(dep['pkg'])
    del paths[root_id]
    return paths


def _package_id(metadata, name):
    for package in metadata['packages']:
        if package['name'] == name and package['source'] is None:
            return package['id']
    return None


def check_core_closure(metadata):
    """The resolved closure of purrassist-core (normal, build and dev edges) must be empty."""
    root_id = _package_id(metadata, CORE_PACKAGE)
    if root_id is None:
        return [f'{CORE_PACKAGE}: package missing from cargo metadata']
    return [f'{CORE_PACKAGE} must have an empty dependency closure but reaches {" -> ".join(path)}'
            for path in _closure_paths(metadata, root_id, {'normal', 'build', 'dev'}).values()]


def publishable_workspace_packages(metadata):
    members = set(metadata['workspace_members'])
    return [package for package in metadata['packages']
            if package['id'] in members and package.get('publish') is not False
            and package.get('publish') != []]


def check_network_denylist(metadata):
    """No publishable crate may reach a networking or async-runtime package."""
    errors = []
    for package in publishable_workspace_packages(metadata):
        for path in _closure_paths(metadata, package['id'], {'normal', 'build'}).values():
            if NETWORK_DENYLIST.fullmatch(path[-1]):
                errors.append(f'{package["name"]}: network-capable package {path[-1]} is forbidden '
                              f'(path {" -> ".join(path)})')
    return errors


def _code_only(text):
    """Return `text` with comments (line, doc and nested block) blanked, preserving line
    numbers; string and raw-string contents are kept because they are code."""
    out = []
    i, n, depth = 0, len(text), 0
    while i < n:
        two = text[i:i + 2]
        char = text[i]
        if depth:
            if two == '/*':
                depth += 1
                out.append('  ')
                i += 2
            elif two == '*/':
                depth -= 1
                out.append('  ')
                i += 2
            else:
                out.append('\n' if char == '\n' else ' ')
                i += 1
        elif two == '/*':
            depth = 1
            out.append('  ')
            i += 2
        elif two == '//':
            while i < n and text[i] != '\n':
                out.append(' ')
                i += 1
        elif char == 'r' and (i == 0 or not (text[i - 1].isalnum() or text[i - 1] == '_')) \
                and re.match(r'r#*"', text[i:i + 66]):
            hashes = re.match(r'r(#*)"', text[i:]).group(1)
            end = text.find('"' + hashes, i + 2 + len(hashes))
            end = n if end < 0 else end + 1 + len(hashes)
            out.append(text[i:end])
            i = end
        elif char == '"':
            j = i + 1
            while j < n and text[j] != '"':
                j += 2 if text[j] == '\\' else 1
            out.append(text[i:j + 1])
            i = j + 1
        elif text[i:i + 3] == "'\"'":
            out.append("'\"'")
            i += 3
        else:
            out.append(char)
            i += 1
    return ''.join(out)


def scan_ambient_source(label, text):
    """Report every ambient-access token in the code (non-comment) text of one source file."""
    code = _code_only(text)
    errors = []
    for number, line in enumerate(code.splitlines(), 1):
        match = AMBIENT_PATTERN.search(line)
        if match:
            errors.append(f'{label}:{number}: ambient access token {match.group(0)!r} is forbidden in purrassist-core')
    for group in AMBIENT_GROUP.finditer(code):
        member = AMBIENT_GROUP_MEMBER.search(group.group(1))
        if member:
            number = code.count('\n', 0, group.start()) + 1
            errors.append(f'{label}:{number}: ambient access import std::{{{member.group(1)}}} is forbidden in purrassist-core')
    return errors


def check_ambient_access(root):
    errors = []
    for path in sorted((root / 'crates/core/src').rglob('*.rs')):
        errors.extend(scan_ambient_source(str(path.relative_to(root)), path.read_text()))
    return errors


def check_dependency_baseline(metadata, baseline):
    """Every third-party package must be pinned in the reviewed baseline."""
    actual = {f"{package['name']}@{package['version']}" for package in metadata['packages']
              if package['source'] is not None}
    added = sorted(actual - set(baseline))
    if not added:
        return []
    return [f'new third-party dependencies {added} are not in {BASELINE}; adding dependencies '
            'requires explicit maintainer approval and an update to that baseline']


def check_core_dev_dependencies(manifest):
    """purrassist-core may declare no dev (or build) dependencies, including per target."""
    errors = []
    tables = [('', manifest)] + [(f'target.{name}.', value) for name, value in manifest.get('target', {}).items()]
    for prefix, table in tables:
        for key in ('dev-dependencies', 'dev_dependencies', 'build-dependencies', 'build_dependencies'):
            if table.get(key):
                errors.append(f'{CORE_PACKAGE}: [{prefix}{key}] must be empty')
    return errors


def _distinct_requests(cases):
    """Distinct cases after removing identity, category and truth metadata."""
    return len({json.dumps({key: value for key, value in case.items()
                            if key not in ('id', 'category', 'truth')}, sort_keys=True)
                for case in cases})


def check_evaluation_manifest(root):
    """Verify every evaluation corpus against `manifest.json`.

    Python-generated outputs (JSON arrays) are checked for digest and case
    count, registered inputs and the Python generator for digest. Each
    `generators` entry (a corpus written by another generator, such as the
    Rust tool-call corpus example) is checked for its JSON Lines digest, case
    count and distinct-request count, and every listed generator source for
    its digest."""
    errors = []
    evaluation_root = root / 'crates/core/tests/data/evaluation'
    evaluation_manifest = json.loads((evaluation_root / 'manifest.json').read_text())
    for name, entry in evaluation_manifest['outputs'].items():
        path = evaluation_root / name
        if (hashlib.sha256(path.read_bytes()).hexdigest() != entry['sha256']
                or len(json.loads(path.read_text())) != entry['cases']):
            errors.append(f'evaluation/{name}: corpus digest or census mismatch')
    for name, value in evaluation_manifest['inputs'].items():
        if name.endswith('_sha256'):
            input_path = root / evaluation_manifest['inputs'][name.removesuffix('_sha256')]
            if hashlib.sha256(input_path.read_bytes()).hexdigest() != value:
                errors.append(f'evaluation: registered input digest mismatch: {input_path.relative_to(root)}')
    generator = root / evaluation_manifest['generator']
    if hashlib.sha256(generator.read_bytes()).hexdigest() != evaluation_manifest['generator_sha256']:
        errors.append('evaluation: generator digest mismatch')
    for name, entry in evaluation_manifest.get('generators', {}).items():
        path = evaluation_root / name
        if not path.is_file():
            errors.append(f'evaluation/{name}: registered corpus missing')
            continue
        data = path.read_bytes()
        try:
            cases = [json.loads(line) for line in data.decode('utf-8').splitlines()]
        except (UnicodeDecodeError, json.JSONDecodeError):
            errors.append(f'evaluation/{name}: not JSON Lines')
            continue
        if (hashlib.sha256(data).hexdigest() != entry['sha256'] or len(cases) != entry['cases']
                or _distinct_requests(cases) != entry['distinct_requests']):
            errors.append(f'evaluation/{name}: corpus digest or census mismatch')
        if not entry.get('sources'):
            errors.append(f'evaluation/{name}: no registered generator sources')
        for source, digest in entry.get('sources', {}).items():
            source_path = root / source
            if not source_path.is_file() or hashlib.sha256(source_path.read_bytes()).hexdigest() != digest:
                errors.append(f'evaluation/{name}: generator source digest mismatch: {source}')
    return errors


def main():
    errors = []
    core = tomllib.loads((ROOT / 'crates/core/Cargo.toml').read_text())
    if core.get('dependencies'):
        errors.append('purrassist-core must have zero external or workspace runtime dependencies')
    for target in core.get('target', {}).values():
        if target.get('dependencies'):
            errors.append('purrassist-core cannot hide dependencies behind target selection')

    for source in ['crates', 'scripts']:
        for path in (ROOT / source).rglob('*'):
            if not path.is_file() or path.suffix not in {'.rs', '.py', '.js', '.mjs', '.toml', '.sh'}:
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
    full_metadata = json.loads(subprocess.check_output(
        ['cargo', 'metadata', '--format-version', '1', '--locked', '--all-features'], cwd=ROOT, text=True))
    errors.extend(check_core_closure(full_metadata))
    errors.extend(check_network_denylist(full_metadata))
    errors.extend(check_ambient_access(ROOT))
    errors.extend(check_core_dev_dependencies(core))
    errors.extend(check_dependency_baseline(
        metadata, json.loads((ROOT / BASELINE).read_text())['third_party_packages']))
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
    errors.extend(check_evaluation_manifest(ROOT))
    for crate_name, notices in inventory['packaged_notices'].items():
        for name, expected in notices.items():
            path = ROOT / 'crates' / crate_name / name
            if not path.is_file() or hashlib.sha256(path.read_bytes()).hexdigest() != expected:
                errors.append(f'{crate_name}: registered package notice missing or changed: {name}')
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


if __name__ == '__main__':
    main()
