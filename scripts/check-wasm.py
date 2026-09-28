#!/usr/bin/env python3
# SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
# SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0
"""Build and execute the same acceptance transcript natively and under WASI."""
import json
import hashlib
import os
from pathlib import Path
import subprocess
import tarfile
import urllib.request

ROOT = Path(__file__).resolve().parent.parent

def artifact(package, target=None, environment=None):
    example = 'portable-' + package.removeprefix('purrassist-')
    command = ['cargo', 'build', '-p', package, '--example', example, '--locked', '--message-format=json']
    if target:
        command += ['--target', target]
    result = subprocess.run(command, cwd=ROOT, text=True, stdout=subprocess.PIPE, check=True, env=environment)
    paths = [event['executable'] for line in result.stdout.splitlines()
             if (event := json.loads(line)).get('reason') == 'compiler-artifact'
             and event.get('executable') and event['target']['name'] == example]
    if len(paths) != 1:
        raise SystemExit(f'expected one portable artifact, got {paths}')
    return paths[0]

def wasi_environment():
    manifest = json.loads((ROOT / 'vendor/wasi.json').read_text())
    cache = ROOT / '.cache/wasi'
    cache.mkdir(parents=True, exist_ok=True)
    archive = cache / 'sysroot.tar.gz'
    if not archive.exists():
        with urllib.request.urlopen(manifest['url']) as response:
            archive.write_bytes(response.read())
    if hashlib.sha256(archive.read_bytes()).hexdigest() != manifest['sha256']:
        raise SystemExit('WASI sysroot archive digest mismatch')
    sysroot = cache / manifest['directory']
    # Re-extract verified bytes; never trust an old, independently modified tree.
    with tarfile.open(archive) as source:
        source.extractall(cache, filter='data')
    environment = os.environ.copy()
    for name in ('CFLAGS', 'CXXFLAGS', 'TARGET_CFLAGS', 'TARGET_CXXFLAGS',
                 'CFLAGS_wasm32-wasip1', 'CXXFLAGS_wasm32-wasip1'):
        environment.pop(name, None)
    for name in ('CFLAGS_wasm32_wasip1', 'CXXFLAGS_wasm32_wasip1'):
        environment[name] = f'--sysroot={sysroot}'
    return environment

for package in ('purrassist-core', 'purrassist-syntax'):
    native = subprocess.check_output([artifact(package)], cwd=ROOT)
    environment = wasi_environment() if package == 'purrassist-syntax' else None
    wasm = subprocess.check_output(['node', str(ROOT/'scripts/run-wasi.mjs'), artifact(package, 'wasm32-wasip1', environment)], cwd=ROOT)
    if native != wasm:
        raise SystemExit(f'{package} native/WASI mismatch\nnative={native!r}\nWASI={wasm!r}')
    print(f'{package}: executed native and WASI transcripts match.')
subprocess.run(['cargo', 'build', '-p', 'purrassist-core', '--target', 'wasm32-unknown-unknown', '--locked'], cwd=ROOT, check=True)
print('Browser-WASM core builds; native/WASI recovery and syntax acceptance passed.')
