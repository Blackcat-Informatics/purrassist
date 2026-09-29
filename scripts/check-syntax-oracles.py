#!/usr/bin/env python3
# SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
# SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0
"""Compare fixed grammar probes to independent compilers without executing source."""
import base64
import hashlib
import json
from pathlib import Path
import subprocess
import sys
import tarfile
import tempfile
import urllib.request

ROOT = Path(__file__).resolve().parent.parent
manifest = json.loads((ROOT / 'vendor/typescript.json').read_text())
cache = ROOT / '.cache/typescript'
cache.mkdir(parents=True, exist_ok=True)
archive = cache / 'typescript.tgz'
if not archive.exists():
    with urllib.request.urlopen(manifest['url'], timeout=60) as response:
        archive.write_bytes(response.read())
integrity = 'sha512-' + base64.b64encode(hashlib.sha512(archive.read_bytes()).digest()).decode()
if integrity != manifest['integrity']:
    raise SystemExit('TypeScript compiler archive integrity mismatch')
with tarfile.open(archive) as package:
    package.extractall(cache, filter='data')
metadata = json.loads((cache / 'package/package.json').read_text())
if metadata.get('dependencies'):
    raise SystemExit('compiler gained unregistered runtime dependencies')
build = subprocess.run(['cargo', 'build', '-p', 'purrassist-cli', '--locked', '--message-format=json'], cwd=ROOT, check=True, stdout=subprocess.PIPE, text=True)
binaries = [event['executable'] for line in build.stdout.splitlines()
            if (event := json.loads(line)).get('reason') == 'compiler-artifact'
            and event.get('executable') and event['target']['name'] == 'purrassist']
if len(binaries) != 1:
    raise SystemExit('expected exactly one CLI artifact')
cases = [
    ('python', '.py', 'def answer():\n    return 42\n', True),
    ('python', '.py', 'async def answer(value):\n    return await value\n', True),
    ('python', '.py', 'def answer(\n    return 42\n', False),
    ('rust', '.rs', 'pub fn answer() -> u32 { 42 }', True),
    ('rust', '.rs', 'pub fn answer() -> u32 { let x = 42; x }', True),
    ('rust', '.rs', 'pub fn answer() -> u32 { let x = 42 x }', False),
    ('javascript', '.js', 'function answer() { return 42; }', True),
    ('javascript', '.js', 'const answer = value => value?.name ?? "none";', True),
    ('javascript', '.js', 'function answer( { return 42; }', False),
    ('javascript', '.jsx', 'const answer = <div title="tea"/>;', True),
    ('javascript', '.jsx', 'const answer = <div></span>;', False),
    ('typescript', '.ts', 'interface Cup { temperature: number }\nfunction answer(x: Cup): number { return x.temperature; }', True),
    ('typescript', '.ts', 'function answer(x: ): number { return 42; }', False),
    ('tsx', '.tsx', 'const answer = (x: string) => <div>{x}</div>;', True),
    ('tsx', '.tsx', 'const answer = (x: string) => <div>{x}</span>;', False),
]
with tempfile.TemporaryDirectory(prefix='purrassist-oracles-') as directory:
    directory = Path(directory)
    for index, (language, extension, source, expected) in enumerate(cases):
        path = directory / f'probe{index}{extension}'
        path.write_text(source)
        inspected = subprocess.check_output([binaries[0], 'call', 'inspect_code', '--root', str(directory)], input=json.dumps({'source': source, 'language': language}), text=True)
        actual = json.loads(inspected)['value']['valid']
        if extension == '.py':
            command = [sys.executable, '-c', 'import ast,sys; ast.parse(sys.stdin.read())']
        elif extension == '.rs':
            command = ['rustc', '--edition=2024', '--crate-type=lib', '--emit=metadata', '-o', str(directory / 'probe.rmeta'), str(path)]
        elif extension == '.js':
            command = ['node', '--check', str(path)]
        else:
            command = ['node', str(ROOT / 'scripts/check-typescript.mjs'), str(cache / 'package/lib/typescript.js'), str(path)]
        oracle = subprocess.run(command, input=source, text=True, stdout=subprocess.PIPE, stderr=subprocess.PIPE, timeout=30)
        if actual != expected or (oracle.returncode == 0) != expected:
            raise SystemExit(f'{language} probe {index}: grammar={actual}, expected={expected}, oracle={oracle.returncode}\n{oracle.stderr}')
print(f'{len(cases)} syntax probes agree with Python, rustc, Node, and pinned TypeScript compilers; source was not executed.')
