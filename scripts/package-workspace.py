#!/usr/bin/env python3
# SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
# SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0
"""Verify release archives with fresh Cargo source/build caches, then retain them.

Resolve the selected toolchain's Cargo directly through rustup: a host launcher
can replace CARGO_HOME and target/build paths, defeating caller isolation. Native
build outputs stay in an owned temporary directory outside the checkout. The packaging orchestration regressions run before every package attempt. Only
archives that pass Cargo verification and check-packages.py are copied to a
unique ignored .cache/package-archives directory, whose path is printed.
"""
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile

ROOT = Path(__file__).resolve().parent.parent


def isolated_environment(cargo: Path, directory: Path) -> dict[str, str]:
    """Keep registry credentials out and isolate both sources and build outputs."""
    environment = os.environ.copy()
    environment['CARGO_HOME'] = str(directory / 'cargo-home')
    environment['CARGO_TARGET_DIR'] = str(directory / 'target')
    environment['CARGO_BUILD_BUILD_DIR'] = str(directory / 'build')
    # The archive auditor invokes `cargo metadata`; give it the same real tool
    # rather than allowing PATH to select a host launcher with different paths.
    environment['PATH'] = str(cargo.parent) + os.pathsep + environment.get('PATH', '')
    for name in ('RUSTC_WRAPPER', 'RUSTC_WORKSPACE_WRAPPER',
                 'CARGO_BUILD_RUSTC_WRAPPER', 'CARGO_BUILD_RUSTC_WORKSPACE_WRAPPER'):
        environment[name] = ''
    for name in tuple(environment):
        if name in {'CARGO_TOKEN', 'CARGO_REGISTRY_TOKEN'} or (
            name.startswith('CARGO_REGISTRIES_') and name.endswith('_TOKEN')
        ):
            del environment[name]
    return environment


def package_workspace() -> Path:
    """Run full Cargo package verification and source/license audit, or fail.

    Every invocation starts with fresh source, target and intermediate caches,
    including repeated runs at the same commit. No archive from an earlier run
    is admitted as current evidence. A failed Cargo command or archive audit
    propagates its error and publishes no verified archive directory.
    """
    revision = subprocess.check_output(
        ['git', 'rev-parse', '--verify', 'HEAD'], cwd=ROOT, text=True).strip()
    cargo = Path(subprocess.check_output(
        ['rustup', 'which', 'cargo'], cwd=ROOT, text=True).strip())
    if not cargo.is_absolute() or not cargo.is_file():
        raise ValueError('rustup did not resolve an installed toolchain Cargo executable')
    with tempfile.TemporaryDirectory(prefix='purrassist-package-') as temporary:
        directory = Path(temporary)
        if directory.resolve().is_relative_to(ROOT.resolve()):
            raise ValueError('packaging temporary build directory must be outside the checkout')
        environment = isolated_environment(cargo, directory)
        target = directory / 'target'
        configuration = [
            '--config', f'build.target-dir={json.dumps(str(target), ensure_ascii=False)}',
            '--config', f'build.build-dir={json.dumps(str(directory / "build"), ensure_ascii=False)}',
            '--config', 'build.rustc-wrapper=""',
            '--config', 'build.rustc-workspace-wrapper=""',
        ]
        print(f'Packaging source commit {revision}; real Cargo: {cargo}', flush=True)
        print(f'Fresh temporary Cargo home/build/target: {directory}', flush=True)
        metadata = json.loads(subprocess.check_output(
            [str(cargo), 'metadata', '--no-deps', '--format-version', '1', '--locked',
             *configuration], cwd=ROOT, env=environment, text=True))
        if Path(metadata['target_directory']).resolve() != target.resolve():
            raise ValueError('Cargo metadata did not honor the isolated package target directory')
        # check-release.py proves this is the complete six-crate release set.
        # Unpublished target-specific harnesses remain outside packaging.
        publishable = sorted(
            (package['name'], package['version']) for package in metadata['packages']
            if package.get('publish') != [])
        if not publishable:
            raise ValueError('workspace has no publishable release packages')
        package_args = [flag for name, _ in publishable for flag in ('-p', name)]
        subprocess.run([str(cargo), 'package', *package_args, '--locked',
                        '--target-dir', str(target), *configuration],
                       cwd=ROOT, env=environment, check=True)
        archives = target / 'package'
        subprocess.run([sys.executable, 'scripts/check-packages.py', str(archives)],
                       cwd=ROOT, env=environment, check=True)
        names = [f'{name}-{version}.crate' for name, version in publishable]
        if any(not (archives / name).is_file() for name in names):
            raise ValueError('verified archive inventory is incomplete')
        retained = ROOT / '.cache/package-archives'
        retained.mkdir(parents=True, exist_ok=True)
        output = Path(tempfile.mkdtemp(prefix=revision[:12] + '-', dir=retained))
        for name in names:
            shutil.copy2(archives / name, output / name)
        print(f'Verified {len(names)} release archives retained in {output}', flush=True)
        return output


if __name__ == '__main__':
    subprocess.run([sys.executable, 'scripts/test-package-workspace.py'],
                   cwd=ROOT, check=True)
    package_workspace()
