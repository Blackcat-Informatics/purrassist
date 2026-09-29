#!/usr/bin/env python3
# SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
# SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0
"""Build and execute the same acceptance transcript natively, under WASI, and
under wasm32-unknown-unknown (no WASI, executed by Node's plain WebAssembly
API)."""
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

def harness_artifact():
    """The single wasm32-unknown-unknown `.wasm` file `wasm-harness` produces."""
    command = ['cargo', 'build', '-p', 'wasm-harness', '--target', 'wasm32-unknown-unknown',
               '--locked', '--message-format=json']
    result = subprocess.run(command, cwd=ROOT, text=True, stdout=subprocess.PIPE, check=True)
    paths = [filename for line in result.stdout.splitlines()
             if (event := json.loads(line)).get('reason') == 'compiler-artifact'
             for filename in (event.get('filenames') or [])
             if filename.endswith('.wasm')]
    if len(paths) != 1:
        raise SystemExit(f'expected one wasm-harness artifact, got {paths}')
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

# `purrassist-host` depends (through `purrassist` -> `purrassist-syntax`) on
# tree-sitter's C grammars, so it needs the same WASI C sysroot as syntax.
WASI_PACKAGES = {'purrassist-syntax', 'purrassist-host'}
core_native_output = None
for package in ('purrassist-core', 'purrassist-syntax', 'purrassist-host'):
    native_path = artifact(package)
    native = subprocess.check_output([native_path], cwd=ROOT)
    if package == 'purrassist-core':
        core_native_output = native
    environment = wasi_environment() if package in WASI_PACKAGES else None
    wasm = subprocess.check_output(['node', str(ROOT/'scripts/run-wasi.mjs'), artifact(package, 'wasm32-wasip1', environment)], cwd=ROOT)
    if native != wasm:
        raise SystemExit(f'{package} native/WASI mismatch\nnative={native!r}\nWASI={wasm!r}')
    print(f'{package}: executed native and WASI transcripts match.')

harness = harness_artifact()
unknown_unknown = subprocess.check_output(['node', str(ROOT/'scripts/run-wasm.mjs'), harness], cwd=ROOT)
if core_native_output != unknown_unknown:
    raise SystemExit(
        f'purrassist-core native/wasm32-unknown-unknown mismatch\n'
        f'native={core_native_output!r}\nunknown-unknown={unknown_unknown!r}')
print('purrassist-core: executed native and wasm32-unknown-unknown transcripts match.')
print('Native, WASI (core/syntax/host), and browser-WASM (core, executed) acceptance transcripts all match.')
