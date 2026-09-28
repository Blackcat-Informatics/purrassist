// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0
import { readFile } from 'node:fs/promises';
import { WASI } from 'node:wasi';
const filename = process.argv[2];
if (!filename) throw new Error('expected WASI artifact path');
const wasi = new WASI({ version: 'preview1', args: [filename], env: {}, preopens: {} });
const module = await WebAssembly.compile(await readFile(filename));
const instance = await WebAssembly.instantiate(module, wasi.getImportObject());
wasi.start(instance);
