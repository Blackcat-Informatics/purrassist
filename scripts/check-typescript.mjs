// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0
// Parse only: caller supplies the integrity-verified compiler and source file.
import { createRequire } from 'node:module';
import { readFileSync } from 'node:fs';
const require = createRequire(import.meta.url);
const ts = require(process.argv[2]);
if (ts.version !== '5.9.3') throw new Error('unexpected compiler version');
const path = process.argv[3];
const kind = path.endsWith('.tsx') ? ts.ScriptKind.TSX : path.endsWith('.jsx') ? ts.ScriptKind.JSX : ts.ScriptKind.TS;
const tree = ts.createSourceFile(path, readFileSync(path, 'utf8'), ts.ScriptTarget.Latest, true, kind);
for (const diagnostic of tree.parseDiagnostics) {
    process.stderr.write(`${diagnostic.start}: ${ts.flattenDiagnosticMessageText(diagnostic.messageText, '\n')}\n`);
}
process.exitCode = tree.parseDiagnostics.length ? 1 : 0;
