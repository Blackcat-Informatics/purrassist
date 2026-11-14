// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0
// Instantiate the wasm32-unknown-unknown `wasm-harness` module with Node's
// plain WebAssembly API (no WASI import object of any kind) and print the
// portable acceptance transcript its `portable_main` export writes into the
// module's own linear memory.
import { readFile } from 'node:fs/promises';

const filename = process.argv[2];
if (!filename) throw new Error('expected a wasm32-unknown-unknown artifact path');
const module = await WebAssembly.compile(await readFile(filename));
const instance = await WebAssembly.instantiate(module, {});
const { portable_main: portableMain, memory } = instance.exports;
if (typeof portableMain !== 'function' || !(memory instanceof WebAssembly.Memory)) {
  throw new Error('module does not export the expected portable_main()/memory ABI');
}
// `portable_main` packs a 32-bit pointer and a 32-bit length into one i64
// return value (high 32 bits: pointer, low 32 bits: length); Node exposes an
// exported i64 result as a BigInt.
const packed = portableMain();
const ptr = Number(packed >> 32n);
const len = Number(packed & 0xffffffffn);
const bytes = new Uint8Array(memory.buffer, ptr, len);
process.stdout.write(new TextDecoder().decode(bytes));
