#!/usr/bin/env python3
# SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
# SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0
"""Packaging orchestration isolates same-version sources and fails closed."""
import contextlib
import importlib.util
import io
import json
import os
from pathlib import Path
import subprocess
import tempfile
import tomllib
import unittest
from unittest.mock import patch

SPEC = importlib.util.spec_from_file_location('package_workspace', Path(__file__).with_name('package-workspace.py'))
if SPEC is None or SPEC.loader is None:
    raise ImportError('cannot load package-workspace.py')
PACKAGE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(PACKAGE)
NAMES = ['purrassist-core', 'purrassist-derive', 'purrassist-syntax', 'purrassist', 'purrassist-host', 'purrassist-cli']


class Packaging(unittest.TestCase):
    def setUp(self):
        self.scratch = tempfile.TemporaryDirectory(prefix='purrassist-package-test-')
        self.addCleanup(self.scratch.cleanup)
        self.root = Path(self.scratch.name)
        self.cargo = self.root / 'toolchain/bin/cargo'
        self.cargo.parent.mkdir(parents=True)
        self.cargo.write_bytes(b'fake tool path; subprocesses are mocked')
        self.environments = []
        self.commands = []
        self.stage_error = None
        self.wrong_target = False
        self.omit_archive = False

    def check_output(self, command, **kwargs):
        if command[0] == 'git':
            return 'a' * 40 + '\n'
        if command[0] == 'rustup':
            self.assertEqual(command, ['rustup', 'which', 'cargo'])
            return str(self.cargo) + '\n'
        self.assertEqual(command[:2], [str(self.cargo), 'metadata'])
        self.assertIn('--locked', command)
        env = kwargs['env']
        self.assertFalse(Path(env['CARGO_HOME']).exists())
        self.assertFalse(Path(env['CARGO_TARGET_DIR']).exists())
        self.assertFalse(Path(env['CARGO_BUILD_BUILD_DIR']).exists())
        packages = [{'name': name, 'version': '0.1.0', 'publish': None} for name in NAMES]
        packages.append({'name': 'unpublished-harness', 'version': '0.1.0', 'publish': []})
        return json.dumps({'packages': packages, 'target_directory': '/wrong' if self.wrong_target else env['CARGO_TARGET_DIR']})

    def run_command(self, command, **kwargs):
        self.assertTrue(kwargs['check'])
        self.commands.append(command)
        env = kwargs['env']
        if command[0] == str(self.cargo):
            self.assertEqual(command[1], 'package')
            self.assertIn('--locked', command)
            self.assertNotIn('--no-verify', command)
            self.assertNotIn('--allow-dirty', command)
            selected = [command[i + 1] for i, arg in enumerate(command) if arg == '-p']
            self.assertEqual(selected, sorted(NAMES))
            self.assertEqual(command[command.index('--target-dir') + 1], env['CARGO_TARGET_DIR'])
            self.assertIn(f'build.build-dir={json.dumps(env["CARGO_BUILD_BUILD_DIR"], ensure_ascii=False)}', command)
            for i, arg in enumerate(command):
                if arg == '--config':
                    parsed = tomllib.loads(command[i + 1])
                    self.assertIn('build', parsed)
            for config in ('build.rustc-wrapper=""', 'build.rustc-workspace-wrapper=""'):
                self.assertIn(config, command)
            self.environments.append(env)
            if self.stage_error == 'package':
                raise subprocess.CalledProcessError(101, command)
            directory = Path(env['CARGO_TARGET_DIR']) / 'package'
            directory.mkdir(parents=True)
            for name in NAMES:
                if self.omit_archive and name == NAMES[-1]:
                    continue
                (directory / f'{name}-0.1.0.crate').write_bytes(name.encode())
        else:
            self.assertEqual(command[1], 'scripts/check-packages.py')
            self.assertEqual(command[2], str(Path(env['CARGO_TARGET_DIR']) / 'package'))
            self.assertEqual(Path(env['PATH'].split(os.pathsep)[0]) / 'cargo', self.cargo)
            if self.stage_error == 'audit':
                raise subprocess.CalledProcessError(1, command)
        return subprocess.CompletedProcess(command, 0)

    def invoke(self):
        with patch.object(PACKAGE, 'ROOT', self.root), \
             patch.object(PACKAGE.subprocess, 'check_output', side_effect=self.check_output), \
             patch.object(PACKAGE.subprocess, 'run', side_effect=self.run_command), \
             contextlib.redirect_stdout(io.StringIO()):
            return PACKAGE.package_workspace()

    def test_repeated_commit_uses_fresh_sources_and_builds_and_exact_verified_archives(self):
        ambient = dict(os.environ)
        contaminated = {'CARGO_HOME': '/old-source-cache', 'CARGO_TARGET_DIR': '/shared-target',
                        'CARGO_BUILD_BUILD_DIR': '/shared-build'}
        # Registry token variables carry a sentinel, never a real credential;
        # packaging must remove every one of them from the child environment.
        tokens = ('CARGO_TOKEN', 'CARGO_REGISTRY_TOKEN', 'CARGO_REGISTRIES_OTHER_TOKEN')
        contaminated.update({name: 'sentinel-not-a-credential' for name in tokens})
        wrappers = ('RUSTC_WRAPPER', 'RUSTC_WORKSPACE_WRAPPER',
                    'CARGO_BUILD_RUSTC_WRAPPER', 'CARGO_BUILD_RUSTC_WORKSPACE_WRAPPER')
        contaminated.update({name: 'host-wrapper' for name in wrappers})
        with patch.dict(os.environ, contaminated):
            first, second = self.invoke(), self.invoke()
        self.assertEqual(dict(os.environ), ambient)
        self.assertNotEqual(first, second)
        for env in self.environments:
            for name in tokens:
                self.assertNotIn(name, env)
            for name in wrappers:
                self.assertEqual(env[name], '')
            self.assertFalse(Path(env['CARGO_TARGET_DIR']).exists())
        self.assertNotEqual(self.environments[0]['CARGO_HOME'], self.environments[1]['CARGO_HOME'])
        for directory in (first, second):
            self.assertEqual({file.name for file in directory.iterdir()}, {f'{name}-0.1.0.crate' for name in NAMES})
            for name in NAMES:
                self.assertEqual((directory / f'{name}-0.1.0.crate').read_bytes(), name.encode())

    def test_quoted_temporary_parent_remains_valid_toml_and_exact_path(self):
        parent = self.root.parent / (self.root.name + '-quoted-"-é')
        parent.mkdir()
        self.addCleanup(parent.rmdir)
        original = tempfile.TemporaryDirectory
        def directory(**kwargs):
            return original(dir=parent, **kwargs)
        with patch.object(PACKAGE.tempfile, 'TemporaryDirectory', side_effect=directory):
            self.invoke()
        self.assertIn('"', self.environments[-1]['CARGO_BUILD_BUILD_DIR'])

    def test_cargo_verification_failure_prevents_audit_and_archive_publication(self):
        self.stage_error = 'package'
        with self.assertRaises(subprocess.CalledProcessError):
            self.invoke()
        self.assertEqual(len(self.commands), 1)
        self.assertFalse((self.root / '.cache/package-archives').exists())

    def test_archive_audit_failure_prevents_archive_publication(self):
        self.stage_error = 'audit'
        with self.assertRaises(subprocess.CalledProcessError):
            self.invoke()
        self.assertEqual(len(self.commands), 2)
        self.assertFalse((self.root / '.cache/package-archives').exists())

    def test_ignored_isolated_target_fails_before_package(self):
        self.wrong_target = True
        with self.assertRaisesRegex(ValueError, 'did not honor'):
            self.invoke()
        self.assertEqual(self.commands, [])

    def test_missing_expected_archive_fails_even_if_auditor_exits_success(self):
        self.omit_archive = True
        with self.assertRaisesRegex(ValueError, 'inventory is incomplete'):
            self.invoke()
        self.assertFalse((self.root / '.cache/package-archives').exists())

    def test_checkout_tmpdir_is_rejected_before_build_or_metadata(self):
        with patch.object(PACKAGE.tempfile, 'TemporaryDirectory',
                          return_value=contextlib.nullcontext(str(self.root / 'temporary-build'))):
            with self.assertRaisesRegex(ValueError, 'outside the checkout'):
                self.invoke()
        self.assertEqual(self.commands, [])
        self.assertEqual(self.environments, [])

    def test_uninstalled_resolved_cargo_fails_without_using_path_wrapper(self):
        self.cargo.unlink()
        with self.assertRaisesRegex(ValueError, 'installed toolchain Cargo'):
            self.invoke()
        self.assertEqual(self.commands, [])


if __name__ == '__main__':
    unittest.main()
