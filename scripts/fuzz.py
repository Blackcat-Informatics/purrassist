#!/usr/bin/env python3
# SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
# SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0
"""Deterministic source-region coverage-guided qualification (LLVM, pinned
in vendor/tooling.json's `llvm-coverage` entry),
generalized across nine `purrassist-core` targets, with differential probes
against Node's `RegExp` (`pattern`) and `cmark-gfm` (`markdown`).

Replaces the single-purpose `scripts/fuzz-json.py`; see `docs/FUZZING.md`.
"""
import argparse
import hashlib
import json
import os
import re
import shutil
import subprocess
import tarfile
import tempfile
import urllib.request
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
RS = b'\x1e'  # ASCII record separator: multi-field probe input delimiter.

# --------------------------------------------------------------------------
# Seeds and mutation tokens, one set per target.
#
# `json`, `schema` (its instance field), and `pattern` (differentially
# checked against Node) reimplement the six corruption transformations
# tested against a known-origin witness in
# `crates/core/tests/corruption_oracle.rs` (Task 8: truncation, prose
# wrapping, key typo, stringification, type swap, duplicate keys), applied
# here to a small canonical object independent of that test code, purely to
# seed the campaign with the same corruption shapes. `markdown` seeds are
# adapted from examples already vendored at
# `crates/core/tests/data/markdown/{commonmark-0.31.2,gfm-0.29.0.gfm.13}.json`
# (headings, lists, block quotes, fenced code, tables, reference links, GFM
# strikethrough/autolink/tasklist/tagfilter, setext headings, hard breaks).
# --------------------------------------------------------------------------


def _json_corruption_seeds():
    canonical = '{"name":"tea","items":[1,2,3]}'
    return [
        canonical.rstrip('}]').encode(),  # Truncation
        (
            'Sure, here is the value you asked for:\n```json\n'
            + canonical
            + '\n```\nLet me know if you need anything else!'
        ).encode(),  # ProseWrapping
        canonical.replace('"name"', '"nmae"').encode(),  # KeyTypo
        json.dumps(canonical).encode(),  # Stringification
        b'"10"',
        b'10',  # TypeSwap (both directions)
        b'{"name":"tea","name":"tea","items":[1,2,3]}',  # DuplicateKeys
    ]


JSON_SEEDS = [
    b'',
    b'null',
    b'true',
    b'10',
    b'-1.25e+2',
    b'"\\uD83D\\uDE00"',
    b'{"name":"tea","items":[1,2,3]}',
    b'{"x":1,"x":2}',
    b'[[{"nmae":"tea","items":["1"]}]]',
    b'["Tea","coffee"]',
    b'```json\n{"x":1,}\n```',
    b'{name:\'tea\', items:[1,2',
    b'"{\\"x\\":1}"',
    b'\xff',
    *_json_corruption_seeds(),
]
JSON_TOKENS = [
    b'null', b'true', b'false', b'"', b'\\', b'\n', b'\t', b':', b',',
    b'{', b'}', b'[', b']', b'0', b'-1', b'1e9999', b'\\uD800',
    b'"name"', b'"items"', b'"x"', b'"tea"', 'é'.encode(), b'\xff',
]

MARKDOWN_SEEDS = [
    b'',
    b'# Tea\n\nHot **and** _iced_.\n',
    b'# Tea\n\n## Iced\n\nCold brew.\n\n## Hot\n\nKettle.\n',
    b'- one\n- two\n- three\n',
    b'1. one\n2. two\n3. three\n',
    b'> quoted\n> text\n',
    b'```rust\nfn main() {}\n```\n',
    b'| a | b |\n| - | - |\n| 1 | 2 |\n',
    b'[tea]: /tea "Tea"\n\n[tea]\n',
    b'~~strike~~ www.example.org user@example.com\n',
    b'- [ ] todo\n- [x] done\n',
    b'<script>alert(1)</script>\n',
    b'Setext\n======\n\nBody.\n',
    b'a\\\nb\n',
    b'> - nested\n>   continued\n',
]
MARKDOWN_TOKENS = [
    b'#', b'*', b'_', b'~', b'`', b'|', b'-', b'>', b'[', b']', b'(', b')',
    b'!', b':', b'.', b'\n', b'\t', b'\r\n', b'1.', b'    ', b'<', b'>',
    b'"', b'\\', 'café'.encode(), b'\xff',
]

SCHEMA_SEEDS = [
    b'{"type":"object"}' + RS + b'{"x":1}',
    b'{"type":"string"}' + RS + b'10',
    b'{"enum":["tea","coffee"]}' + RS + b'"cofee"',
    b'{"type":"object","required":["x"],"properties":{"x":{"type":"number"}}}'
    + RS
    + b'{"x":"1"}',
    b'{"allOf":[{"type":"string"},{"minLength":2}]}' + RS + b'"t"',
    b'{"not":{"type":"string"}}' + RS + b'"x"',
    b'{"type":"array","items":{"type":"integer"},"uniqueItems":true}'
    + RS
    + b'[1,1,2]',
    b'false' + RS + b'{}',
    b'{"pattern":"^[a-z]+$"}' + RS + b'"Tea1"',
    *(b'{"type":"object"}' + RS + corrupted for corrupted in _json_corruption_seeds()),
]
SCHEMA_TOKENS = JSON_TOKENS + [RS]

PATTERN_SEEDS = [
    b'^[a-z]+[0-9]{2}$' + RS + b'ab12',
    b'foo|bar' + RS + b'xxbarxx',
    b'(?<year>\\d{4})-(?<month>\\d{2})' + RS + b'2024-01',
    b'(a)\\1' + RS + b'aa',
    b'(?=abc)ab' + RS + b'abc',
    b'(?<=ab)c' + RS + b'abc',
    b'\\p{Script=Greek}+' + RS + 'Ελλάδα'.encode(),
    b'a+' + RS + b'aaaa',
    b'[^]' + RS + b'\n',
    b'(a+)+$' + RS + b'aaaaaaaaaaaa!',
    b'^\\d+$' + RS + '１２'.encode(),
]
PATTERN_TOKENS = [
    b'a', b'b', b'0', b'9', b'+', b'*', b'?', b'.', b'^', b'$', b'|', b'(',
    b')', b'[', b']', b'{', b'}', b'\\d', b'\\w', b'\\s', b'\\1', RS,
    b'\n', 'é'.encode(), b'\xff',
]

UNIFIED_SEEDS = [
    b'one\ntwo\n' + RS + b'--- a/x\n+++ b/x\n@@ -1,2 +1,2 @@\n one\n-two\n+three\n',
    b'a\nb\nc\n' + RS + b'--- a/x\n+++ b/x\n@@ -1,3 +1,3 @@\n a\n-b\n+B\n c\n',
    b'' + RS + b'--- a/x\n+++ b/x\n@@ -0,0 +1,1 @@\n+new\n',
]
UNIFIED_TOKENS = [
    b'@@', b'---', b'+++', b'+', b'-', b' ', b'\n', b'\\', RS, b'\t',
    'é'.encode(), b'\xff',
]

FUZZY_SEEDS = [
    b'hello world' + RS + b'wurld' + RS + b'earth' + RS + b'1',
    b'tea' + RS + b'tea' + RS + b'coffee' + RS + b'0',
    b'invoice-1001' + RS + b'invoice-1002' + RS + b'x' + RS + b'2',
]
FUZZY_TOKENS = JSON_TOKENS[:0] + [
    b'a', b'e', b'i', b'o', b'u', b' ', b'-', RS, b'0', b'1', b'2',
    'é'.encode(), b'\xff',
]

MATCHING_SEEDS = [
    b'tea' + RS + b'tea' + RS + b'coffee',
    b'raeder' + RS + b'reader',
    b'invoice-1001' + RS + b'invoice-1002',
    b'not ready' + RS + b'ready',
]
MATCHING_TOKENS = [
    b'a', b'e', b'i', b'o', b'u', b' ', b'-', RS, b'0', b'1', b'2', b'not ',
    'é'.encode(), b'\xff',
]

PATCH_SEEDS = [
    b'hot tea' + RS + b'hot coffee' + RS + b'cold tea',
    b'' + RS + b'grew' + RS + b'',
    b'shrank' + RS + b'' + RS + b'shrank!',
]
PATCH_TOKENS = [
    b'a', b'b', b' ', b'\n', b'-', RS, 'é'.encode(), b'\xff',
]

# `tool-call` input is raw bytes: byte 0 selects the request knobs (repair
# policy, generation stop, reasoning policy, tool choice), byte 1 the effort,
# the ambiguity policy, and the chunking split seed; the rest is one model
# turn; under the forced choice a trailing byte in 0xF8..=0xFF instead draws
# the strict-forced options (see `crates/core/examples/fuzz_tool_call.rs`).
# Seeds pair each turn
# with a header so the fixed seeds already span the request space.
TOOL_CALL_HEADERS = [
    b'\x00\x52',  # per tool, stop, separate reasoning, auto; standard effort
    b'\x05\x91',  # all structural, length stop; quick effort
    b'\x4a\x5e',  # all tolerant, tool_calls stop, tool choice none; reject ambiguity
    b'\x9c\xf0',  # unknown stop, reasoning open at start, required; exact effort
    b'\xe6\x23',  # all tolerant, length stop, reasoning as content, forced read_file; deep effort
]


def _tool_call_seed(index, turn):
    return TOOL_CALL_HEADERS[index % len(TOOL_CALL_HEADERS)] + turn


def _tool_call_conformance_markup():
    """The canonical markup of every built-in dialect's conformance fixture,
    read in sorted path order from
    `crates/core/tests/data/tool-calls/conformance/<dialect>/*.json`."""
    directory = ROOT / 'crates/core/tests/data/tool-calls/conformance'
    return [json.loads(path.read_text())['markup'].encode()
            for path in sorted(directory.glob('*/*.json'))]


# Well-formed calls printed by `purrassist_core::grammar::witness::generate`
# for every built-in dialect over the probe's own tool shapes, with the
# listed witness seeds (`JsonArguments::Object` for seeds 0-1,
# `Stringified` for seeds 2-3), embedded verbatim.
TOOL_CALL_WITNESSES = [
    # anthropic-tool-use.v1: read_file (seed 0)
    "{\"type\":\t\t\"tool_use\"\t,\t\"input\"\t:{\"path\":\"tab\\tseparated \\n  block\\n\"},\"name\"\t:\"read_file\"\t\t,\"id\":\"call_0f4d3872\"\t\t}".encode(),
    # anthropic-tool-use.v1: get_weather, list_dir (seed 1)
    "{\t\"type\":\t\"tool_use\",\"name\"\t:\t\t\"get_weather\",\"id\"\t\t:\t\"call_864a7135\"\t,\"input\":{\"city\":\"\\n  block\\n src/lib.rs\",\"unit\":\"fahrenheit\"}\t\t}\n{\"type\":\t\"tool_use\",\t\"id\":\t\t\"call_77ba0574\"\t,\"input\"\t:{\"path\":\"tab\\tseparated trailing  \"},\"name\":\t\t\"list_dir\"\t}".encode(),
    # anthropic-tool-use.v1: write_file (seed 2)
    "{\t\t\"type\":\"tool_use\",\"id\"\t\t:\t\"call_22bb690f\",\t\t\"name\"\t:\t\t\"write_file\",\t\"input\":\"{\\\"path\\\":\\\"  indented\\\",\\\"content\\\":\\\"tab\\\\tseparated beta gamma\\\",\\\"mode\\\":\\\"create\\\",\\\"ratio\\\":39.32,\\\"count\\\":-415,\\\"options\\\":{\\\"backup\\\":false}}\"\t}".encode(),
    # anthropic-tool-use.v1: delete_path, read_file (seed 3)
    "{\"type\":\"tool_use\"\t,\t\"id\":\t\t\"call_e8c2cf2d\",\"input\"\t\t:\t\"{\\\"path\\\":[\\\"beta gamma \\\\n  block\\\\n\\\",\\\"\\\\n  block\\\\n\\\"]}\"\t,\"name\"\t:\t\t\"delete_path\"\t\t}\n{\"type\":\"tool_use\"\t,\t\"input\":\t\"{\\\"path\\\":\\\"alpha tab\\\\tseparated\\\"}\"\t,\t\t\"name\":\"read_file\"\t\t,\t\t\"id\"\t\t:\"call_91b9c038\"\t\t}".encode(),
    # bare-json.v1: read_file (seed 0)
    "{\t\"name\"\t\t:\t\"read_file\"\t,\t\"arguments\"\t\t:{\"path\":\"tab\\tseparated \\n  block\\n\"},\t\"id\":\"call_f5407269\"\t\t}".encode(),
    # bare-json.v1: get_weather, list_dir (seed 1)
    "{\"arguments\":{\"city\":\"\\n  block\\n src/lib.rs\",\"unit\":\"fahrenheit\"}\t,\"name\":\"get_weather\"\t,\t\t\"id\":\"call_5c2a449c\"\t\t}\n{\t\"id\":\"call_5a072c6d\",\t\t\"arguments\"\t\t:{\"path\":\"\\n  block\\n trailing  \"},\t\"name\":\t\"list_dir\"}".encode(),
    # bare-json.v1: write_file (seed 2)
    "{\t\t\"arguments\":\"{\\\"path\\\":\\\"  indented\\\",\\\"content\\\":\\\"tab\\\\tseparated beta gamma\\\",\\\"mode\\\":\\\"create\\\",\\\"ratio\\\":39.32,\\\"count\\\":-415,\\\"options\\\":{\\\"backup\\\":false}}\",\"name\":\t\t\"write_file\"\t,\t\"id\"\t\t:\"call_861af7d1\"\t}".encode(),
    # bare-json.v1: delete_path, read_file (seed 3)
    "{\"id\":\"call_d7d47acc\"\t,\t\"name\":\t\t\"delete_path\"\t\t,\"arguments\"\t:\"{\\\"path\\\":[\\\"beta gamma \\\\n  block\\\\n\\\",\\\"\\\\n  block\\\\n\\\"]}\"\t}\n{\"name\"\t:\"read_file\",\t\"arguments\"\t\t:\"{\\\"path\\\":\\\"\\\\n  block\\\\n alpha\\\",\\\"limit\\\":-209}\"\t,\t\"id\":\"call_d72aeb2f\"\t}".encode(),
    # deepseek-v3.1.v1: read_file (seed 0)
    "<｜tool▁calls▁begin｜><｜tool▁call▁begin｜>\t\tread_file\t\t<｜tool▁sep｜>{\"path\":\"tab\\tseparated \\n  block\\n\"}\t<｜tool▁call▁end｜>\t<｜tool▁calls▁end｜>".encode(),
    # deepseek-v3.1.v1: get_weather, list_dir (seed 1)
    "<｜tool▁calls▁begin｜>\t<｜tool▁call▁begin｜>get_weather<｜tool▁sep｜>{\"city\":\"\\n  block\\n src/lib.rs\",\"unit\":\"fahrenheit\"}\t<｜tool▁call▁end｜><｜tool▁call▁begin｜>\tlist_dir\t\t<｜tool▁sep｜>{\"path\":\"alpha\"}<｜tool▁call▁end｜><｜tool▁calls▁end｜>".encode(),
    # deepseek-v3.1.v1: write_file (seed 2)
    "<｜tool▁calls▁begin｜>\t\t<｜tool▁call▁begin｜>\t\twrite_file<｜tool▁sep｜>\"{\\\"path\\\":\\\"  indented\\\",\\\"content\\\":\\\"tab\\\\tseparated beta gamma\\\",\\\"mode\\\":\\\"create\\\",\\\"ratio\\\":39.32,\\\"count\\\":-415,\\\"options\\\":{\\\"backup\\\":false}}\"<｜tool▁call▁end｜><｜tool▁calls▁end｜>".encode(),
    # deepseek-v3.1.v1: delete_path, read_file (seed 3)
    "<｜tool▁calls▁begin｜><｜tool▁call▁begin｜>delete_path<｜tool▁sep｜>\"{\\\"path\\\":[\\\"beta gamma \\\\n  block\\\\n\\\",\\\"\\\\n  block\\\\n\\\"]}\"<｜tool▁call▁end｜>\t<｜tool▁call▁begin｜>\t\tread_file\t\t<｜tool▁sep｜>\"{\\\"path\\\":\\\"tab\\\\tseparated   indented\\\"}\"<｜tool▁call▁end｜>\t<｜tool▁calls▁end｜>".encode(),
    # deepseek-v3.v1: read_file (seed 0)
    "<｜tool▁calls▁begin｜><｜tool▁call▁begin｜>\t\tfunction<｜tool▁sep｜>\tread_file\t\t```json\t{\"path\":\"tab\\tseparated \\n  block\\n\"}```<｜tool▁call▁end｜>\t<｜tool▁calls▁end｜>".encode(),
    # deepseek-v3.v1: get_weather, list_dir (seed 1)
    "<｜tool▁calls▁begin｜>\t<｜tool▁call▁begin｜>function\t<｜tool▁sep｜>get_weather```json\t{\"city\":\"\\n  block\\n src/lib.rs\",\"unit\":\"fahrenheit\"}```<｜tool▁call▁end｜><｜tool▁call▁begin｜>function\t<｜tool▁sep｜>\tlist_dir```json{\"path\":\"trailing  \"}```\t\t<｜tool▁call▁end｜>\t<｜tool▁calls▁end｜>".encode(),
    # deepseek-v3.v1: write_file (seed 2)
    "<｜tool▁calls▁begin｜>\t\t<｜tool▁call▁begin｜>\t\tfunction<｜tool▁sep｜>write_file\t\t```json\t\"{\\\"path\\\":\\\"  indented\\\",\\\"content\\\":\\\"tab\\\\tseparated beta gamma\\\",\\\"mode\\\":\\\"create\\\",\\\"ratio\\\":39.32,\\\"count\\\":-415,\\\"options\\\":{\\\"backup\\\":false}}\"```\t\t<｜tool▁call▁end｜><｜tool▁calls▁end｜>".encode(),
    # deepseek-v3.v1: delete_path, read_file (seed 3)
    "<｜tool▁calls▁begin｜><｜tool▁call▁begin｜>function<｜tool▁sep｜>\tdelete_path```json\"{\\\"path\\\":[\\\"beta gamma \\\\n  block\\\\n\\\",\\\"\\\\n  block\\\\n\\\"]}\"\t\t```\t\t<｜tool▁call▁end｜><｜tool▁call▁begin｜>\t\tfunction\t<｜tool▁sep｜>read_file```json\t\"{\\\"path\\\":\\\"beta gamma tab\\\\tseparated\\\",\\\"limit\\\":-604}\"```\t<｜tool▁call▁end｜>\t\t<｜tool▁calls▁end｜>".encode(),
    # harmony.v1: read_file (seed 0)
    "<|start|>assistant\t\t<|channel|>\tcommentary\t\tto=functions.read_file<|constrain|>json<|message|>\t{\"path\":\"tab\\tseparated \\n  block\\n\"}<|call|>".encode(),
    # harmony.v1: get_weather, list_dir (seed 1)
    "<|start|>\tassistantto=functions.get_weather<|channel|>analysis\t\t<|message|>{\"city\":\"\\n  block\\n src/lib.rs\",\"unit\":\"fahrenheit\"}<|call|>\n<|start|>assistant\tto=functions.list_dir<|channel|>commentary\t\tjson\t\t<|message|>\t{\"path\":\"tab\\tseparated\"}<|call|>".encode(),
    # harmony.v1: write_file (seed 2)
    "<|start|>\t\tassistant\t\t<|channel|>commentary\t\tto=functions.write_file\t\tjson<|message|>\t\t\"{\\\"path\\\":\\\"  indented\\\",\\\"content\\\":\\\"tab\\\\tseparated beta gamma\\\",\\\"mode\\\":\\\"create\\\",\\\"ratio\\\":39.32,\\\"count\\\":-415,\\\"options\\\":{\\\"backup\\\":false}}\"<|call|>".encode(),
    # harmony.v1: delete_path, read_file (seed 3)
    "<|start|>assistant<|channel|>\tanalysisto=functions.delete_path\tjson<|message|>\t\t\"{\\\"path\\\":[\\\"beta gamma \\\\n  block\\\\n\\\",\\\"\\\\n  block\\\\n\\\"]}\"<|call|>\n<|start|>assistant\t<|channel|>\t\tfinalto=functions.read_file\t<|constrain|>\t\tjson<|message|>\t\"{\\\"path\\\":\\\"beta gamma tab\\\\tseparated\\\",\\\"limit\\\":-604}\"\t<|call|>".encode(),
    # hermes.v1: read_file (seed 0)
    "<tool_call>\t{\t\t\"type\":\t\"tool_use\"\t\t,\t\"id\":\"call_5a4485ac\"\t,\"name\"\t\t:\"read_file\",\"arguments\":{\"path\":\"tab\\tseparated \\n  block\\n\"}\t\t}</tool_call>".encode(),
    # hermes.v1: get_weather, list_dir (seed 1)
    "<tool_call>{\t\"id\":\t\"call_4baa5dc0\",\"arguments\"\t\t:{\"city\":\"\\n  block\\n src/lib.rs\",\"unit\":\"fahrenheit\"},\"name\":\"get_weather\"\t\t}\t</tool_call>\n<tool_call>{\"id\"\t\t:\t\t\"call_5e4be0f5\"\t\t,\"name\"\t:\"list_dir\"\t,\"arguments\":{\"path\":\"line one\\nline two \\n  block\\n\"}\t\t}</tool_call>".encode(),
    # hermes.v1: write_file (seed 2)
    "<tool_call>\t\t{\t\"id\":\"call_e5ceef8e\",\t\t\"arguments\"\t\t:\"{\\\"path\\\":\\\"  indented\\\",\\\"content\\\":\\\"tab\\\\tseparated beta gamma\\\",\\\"mode\\\":\\\"create\\\",\\\"ratio\\\":39.32,\\\"count\\\":-415,\\\"options\\\":{\\\"backup\\\":false}}\",\t\t\"name\":\t\"write_file\"\t\t}</tool_call>".encode(),
    # hermes.v1: delete_path, read_file (seed 3)
    "<tool_call>{\"id\":\"call_305c487f\",\t\"name\":\t\t\"delete_path\"\t\t,\"arguments\"\t:\"{\\\"path\\\":[\\\"beta gamma \\\\n  block\\\\n\\\",\\\"\\\\n  block\\\\n\\\"]}\"}\t</tool_call>\n<tool_call>{\t\t\"type\":\"function\"\t,\t\t\"id\":\t\"call_07be3ff1\",\"name\"\t:\t\"read_file\"\t\t,\t\t\"arguments\":\"{\\\"path\\\":\\\"\\\\n  block\\\\n alpha\\\",\\\"limit\\\":-209}\"\t\t}\t\t</tool_call>".encode(),
    # kimi-k2.v1: read_file (seed 0)
    "<|tool_calls_section_begin|><|tool_call_begin|>\t\tfunctions.read_file:0<|tool_call_argument_begin|>\t{\"path\":\"tab\\tseparated \\n  block\\n\"}\t<|tool_call_end|>\t\t<|tool_calls_section_end|>".encode(),
    # kimi-k2.v1: get_weather, list_dir (seed 1)
    "<|tool_calls_section_begin|>\t<|tool_call_begin|>functions.get_weather:0<|tool_call_argument_begin|>{\"city\":\"\\n  block\\n src/lib.rs\",\"unit\":\"fahrenheit\"}<|tool_call_end|>\t<|tool_call_begin|>functions.list_dir:1\t<|tool_call_argument_begin|>\t\t{\"path\":\"src/lib.rs alpha\"}\t\t<|tool_call_end|><|tool_calls_section_end|>".encode(),
    # kimi-k2.v1: write_file (seed 2)
    "<|tool_calls_section_begin|>\t\t<|tool_call_begin|>\t\tfunctions.write_file:0<|tool_call_argument_begin|>\t\t\"{\\\"path\\\":\\\"  indented\\\",\\\"content\\\":\\\"tab\\\\tseparated beta gamma\\\",\\\"mode\\\":\\\"create\\\",\\\"ratio\\\":39.32,\\\"count\\\":-415,\\\"options\\\":{\\\"backup\\\":false}}\"\t<|tool_call_end|>\t<|tool_calls_section_end|>".encode(),
    # kimi-k2.v1: delete_path, read_file (seed 3)
    "<|tool_calls_section_begin|><|tool_call_begin|>functions.delete_path:0<|tool_call_argument_begin|>\t\"{\\\"path\\\":[\\\"beta gamma \\\\n  block\\\\n\\\",\\\"\\\\n  block\\\\n\\\"]}\"<|tool_call_end|><|tool_call_begin|>functions.read_file:1\t<|tool_call_argument_begin|>\t\"{\\\"path\\\":\\\"alpha trailing  \\\"}\"<|tool_call_end|>\t<|tool_calls_section_end|>".encode(),
    # llama3-json.v1: read_file (seed 0)
    "<|python_tag|>{\t\t\"type\":\t\"function\"\t,\t\"name\":\"read_file\"\t,\"parameters\"\t\t:{\"path\":\"tab\\tseparated \\n  block\\n\"},\"id\":\t\t\"call_4963bab0\"\t}".encode(),
    # llama3-json.v1: get_weather, list_dir (seed 1)
    "{\t\"id\"\t:\"call_90d7a28a\",\t\"parameters\":{\"city\":\"\\n  block\\n src/lib.rs\",\"unit\":\"fahrenheit\"},\"name\"\t\t:\t\"get_weather\"\t\t}\n{\t\t\"type\":\t\"function\",\"id\"\t:\"call_9fc66081\",\"name\"\t\t:\"list_dir\"\t,\"parameters\"\t:\t\t{\"path\":\"  indented tab\\tseparated\"}}".encode(),
    # llama3-json.v1: write_file (seed 2)
    "<|python_tag|>\t\t{\t\t\"id\":\"call_0fccd72d\",\t\"parameters\"\t\t:\"{\\\"path\\\":\\\"  indented\\\",\\\"content\\\":\\\"tab\\\\tseparated beta gamma\\\",\\\"mode\\\":\\\"create\\\",\\\"ratio\\\":39.32,\\\"count\\\":-415,\\\"options\\\":{\\\"backup\\\":false}}\"\t,\t\t\"name\":\t\"write_file\"}".encode(),
    # llama3-json.v1: delete_path, read_file (seed 3)
    "{\"id\":\t\"call_93349fc3\",\"name\"\t\t:\t\t\"delete_path\",\t\"parameters\":\t\"{\\\"path\\\":[\\\"beta gamma \\\\n  block\\\\n\\\",\\\"\\\\n  block\\\\n\\\"]}\"\t}\n<|python_tag|>{\t\"id\"\t:\t\t\"call_03bb1010\"\t,\t\"name\":\t\"read_file\"\t,\t\t\"arguments\":\"{\\\"path\\\":\\\"beta gamma\\\",\\\"limit\\\":557}\"\t\t}".encode(),
    # mistral-v11.v1: read_file (seed 0)
    b"[TOOL_CALLS]\tread_file\t\t[CALL_ID]\t\tcall_3cb13d09[ARGS]{\"path\":\"tab\\tseparated \\n  block\\n\"}",
    # mistral-v11.v1: get_weather, list_dir (seed 1)
    b"[TOOL_CALLS]get_weather\t[CALL_ID]call_74616796\t[ARGS]{\"city\":\"\\n  block\\n src/lib.rs\",\"unit\":\"fahrenheit\"}[TOOL_CALLS]\t\tlist_dir[CALL_ID]call_5c2a449c\t\t[ARGS]{\"path\":\"line one\\nline two\"}",
    # mistral-v11.v1: write_file (seed 2)
    b"[TOOL_CALLS]\t\twrite_file\t[CALL_ID]call_dcc507e4[ARGS]\"{\\\"path\\\":\\\"  indented\\\",\\\"content\\\":\\\"tab\\\\tseparated beta gamma\\\",\\\"mode\\\":\\\"create\\\",\\\"ratio\\\":39.32,\\\"count\\\":-415,\\\"options\\\":{\\\"backup\\\":false}}\"",
    # mistral-v11.v1: delete_path, read_file (seed 3)
    b"[TOOL_CALLS]delete_path[CALL_ID]call_a2036b72[ARGS]\"{\\\"path\\\":[\\\"beta gamma \\\\n  block\\\\n\\\",\\\"\\\\n  block\\\\n\\\"]}\"[TOOL_CALLS]read_file\t\t[CALL_ID]call_979d2193\t\t[ARGS]\"{\\\"path\\\":\\\"tab\\\\tseparated   indented\\\"}\"",
    # mistral-v3.v1: read_file (seed 0)
    b"[TOOL_CALLS]\t[{\t\t\"id\":\t\"call_00cc4d19\"\t,\"name\":\t\"read_file\",\"parameters\"\t\t:{\"path\":\"tab\\tseparated \\n  block\\n\"}}]",
    # mistral-v3.v1: get_weather, list_dir (seed 1)
    b"[TOOL_CALLS][\t{\"id\":\"call_90d7a28a\",\t\"arguments\":{\"city\":\"\\n  block\\n src/lib.rs\",\"unit\":\"fahrenheit\"},\t\t\"name\"\t:\t\t\"get_weather\"\t\t},{\"id\"\t\t:\"call_607e2c86\"\t,\"arguments\":{\"path\":\"tab\\tseparated \\n  block\\n\"}\t\t,\"name\"\t:\"list_dir\"\t}\t\t]",
    # mistral-v3.v1: write_file (seed 2)
    b"[TOOL_CALLS]\t\t[\t\t{\"name\":\t\t\"write_file\"\t,\t\"parameters\"\t:\t\"{\\\"path\\\":\\\"  indented\\\",\\\"content\\\":\\\"tab\\\\tseparated beta gamma\\\",\\\"mode\\\":\\\"create\\\",\\\"ratio\\\":39.32,\\\"count\\\":-415,\\\"options\\\":{\\\"backup\\\":false}}\"\t\t,\"id\"\t:\"call_b3ac305a\"\t}]",
    # mistral-v3.v1: delete_path, read_file (seed 3)
    b"[TOOL_CALLS][{\"arguments\"\t:\"{\\\"path\\\":[\\\"beta gamma \\\\n  block\\\\n\\\",\\\"\\\\n  block\\\\n\\\"]}\",\t\t\"name\"\t\t:\"delete_path\"\t,\"id\"\t:\t\"call_de98af50\"\t}\t\t,\t\t{\t\"id\":\"call_49f77a23\"\t,\"name\":\t\"read_file\"\t,\t\t\"parameters\":\"{\\\"path\\\":\\\"alpha tab\\\\tseparated\\\"}\"\t\t}\t\t]",
    # openai-tool-calls.v1: read_file (seed 0)
    b"{\"type\": \"function\",\t\t\"function\"\t:\t{\t\"arguments\":{\"path\":\"tab\\tseparated \\n  block\\n\"},\"name\"\t:\"read_file\"}\t\t,\"id\":\"call_0f4d3872\"\t\t}",
    # openai-tool-calls.v1: get_weather, list_dir (seed 1)
    b"{\n          \"type\":\"function\",\"function\"\t:{\"name\"\t:\t\t\"get_weather\",\"arguments\":{\"city\":\"\\n  block\\n src/lib.rs\",\"unit\":\"fahrenheit\"}\t\t}\t,\t\t\"id\"\t\t:\"call_c90789ba\"\t\t}\n{\n    \"type\": \"function\",\t\"id\":\t\"call_18e9685e\",\t\t\"function\":\t{\"arguments\"\t:{\"path\":\"tab\\tseparated trailing  \"},\"name\":\"list_dir\"\t\t}\t}",
    # openai-tool-calls.v1: write_file (seed 2)
    b"{\n\t\t\t\t\"type\":\"function\"\t\t,\"function\":{\"arguments\":\"{\\\"path\\\":\\\"  indented\\\",\\\"content\\\":\\\"tab\\\\tseparated beta gamma\\\",\\\"mode\\\":\\\"create\\\",\\\"ratio\\\":39.32,\\\"count\\\":-415,\\\"options\\\":{\\\"backup\\\":false}}\"\t,\t\"name\"\t\t:\"write_file\"\t}\t\t,\"id\"\t:\"call_b3ac305a\"\t}",
    # openai-tool-calls.v1: delete_path, read_file (seed 3)
    b"{\n      \"type\": \"function\",\"id\"\t:\t\"call_618293ea\",\t\t\"function\"\t\t:{\t\"name\"\t:\t\"delete_path\",\t\"arguments\"\t\t:\"{\\\"path\\\":[\\\"beta gamma \\\\n  block\\\\n\\\",\\\"\\\\n  block\\\\n\\\"]}\"\t\t}\t}\n{\n\t\t\t\t\"type\": \"function\"\t,\"function\":{\t\"arguments\":\"{\\\"path\\\":\\\"alpha\\\",\\\"limit\\\":-32}\"\t\t,\t\t\"name\":\t\t\"read_file\"\t\t}\t\t,\"id\":\t\t\"call_e427bb79\"\t\t}",
    # qwen3-coder-xml.v1: read_file (seed 0)
    b"<tool_call>\t<function=read_file>\n<parameter=path>\ntab\tseparated \n  block\n\n</parameter>\n</function>\t\t</tool_call>",
    # qwen3-coder-xml.v1: get_weather, list_dir (seed 1)
    b"<tool_call><function=get_weather>\n<parameter=city>\n\n  block\n src/lib.rs\n</parameter>\n<parameter=unit>\nfahrenheit\n</parameter>\n</function>\t</tool_call>\n<tool_call>\t<function=list_dir>\n<parameter=path>\nsrc/lib.rs\n</parameter>\n</function></tool_call>",
    # qwen3-coder-xml.v1: write_file (seed 2)
    b"<tool_call>\t\t<function=write_file>\n<parameter=path>\n  indented\n</parameter>\n<parameter=content>\ntab\tseparated beta gamma\n</parameter>\n<parameter=mode>\ncreate\n</parameter>\n<parameter=ratio>\n39.32\n</parameter>\n<parameter=count>\n-415\n</parameter>\n<parameter=options>\n{\"backup\":false}\n</parameter>\n</function>\t</tool_call>",
    # qwen3-coder-xml.v1: delete_path, read_file (seed 3)
    b"<tool_call><function=delete_path>\n<parameter=path>\nbeta gamma \n  block\n\n</parameter>\n<parameter=path>\n\n  block\n\n</parameter>\n</function></tool_call>\n<tool_call>\t<function=read_file>\n<parameter=path>\n  indented\n</parameter>\n</function>\t</tool_call>",
]

# The repair cases of issue #3, phrased against the probe's tool set.
TOOL_CALL_REPAIRS = [
    # The motivating case: a body of stray plural closers, no required
    # parameter (resolves to `{}`) and a required one (never invented).
    b'<tool_call>\n<function=current_time>\n</parameters>\n</parameters>\n</function>\n</tool_call>',
    b'<tool_call><function=list_dir></parameters></parameters></function></tool_call>',
    # Missing `</parameter>`, `</function>`, and `</tool_call>`.
    b'<tool_call>\n<function=read_file>\n<parameter=path>\nsrc/a.rs\n<parameter=limit>\n5\n</function>\n</tool_call>',
    b'<tool_call>\n<function=read_file>\n<parameter=path>\nsrc/a.rs\n</parameter>\n</tool_call>',
    b'<tool_call>\n<function=read_file>\n<parameter=path>\nsrc/a.rs\n</parameter>\n</function>',
    # A whole JSON body followed by one stray closer.
    b'<tool_call>\n<function=read_file>\n{"path": "a.rs", "limit": 2}\n</parameter>\n</function>\n</tool_call>',
    # Nested parameters, depth-matched, with an inline JSON object.
    b'<tool_call>\n<function=write_file>\n<parameter=path>\nx\n</parameter>\n<parameter=content>\ny\n</parameter>\n'
    b'<parameter=options>\n<parameter=owner>\n<parameter=name>\nci\n</parameter>\n</parameter>\n'
    b'<parameter=retries>\n3\n</parameter>\n</parameter>\n</function>\n</tool_call>',
    # Fenced and prose-wrapped calls; a Hermes body with a preamble.
    b'Sure, calling it now:\n```json\n{"name": "get_weather", "arguments": {"city": "Lima"}}\n```\nDone.',
    b'<tool_call>Here you go: {"name": "read_file", "arguments": {"path": "notes.txt"}}</tool_call>',
    # Stringified arguments, valid and invalid.
    b'<tool_call>{"name": "read_file", "arguments": "{\\"path\\": \\"a.rs\\", \\"limit\\": 3}"}</tool_call>',
    b'<tool_call>{"name": "read_file", "arguments": "{\\"path\\": \\"a.rs\\", \\"limit\\": }"}</tool_call>',
    # A dangling trailing opener after a complete call.
    b'<tool_call>{"name": "list_dir", "arguments": {"path": "."}}</tool_call>\n<tool_call>{"name": "rea',
    b'<tool_call>\n<function=read_file>\n<parameter=path>\nsrc/a.rs\n</parameter>\n</function>\n</tool_call>\n<tool_call>\n<function=wri',
    # Name resolution: near miss, alias, hallucination.
    b'<tool_call>{"name": "read_fiel", "arguments": {"path": "a.rs"}}</tool_call>',
    b'<tool_call>{"name": "rm", "arguments": {"path": "build"}}</tool_call>',
    b'<tool_call>{"name": "write_fiel", "arguments": {"path": "a", "content": "b"}}</tool_call>',
    b'<tool_call>{"name": "launch_rockets", "arguments": {}}</tool_call>',
    # Parameter names: explicit alias and typo.
    b'<tool_call>\n<function=read_file>\n<parameter=file>\na.rs\n</parameter>\n</function>\n</tool_call>',
    b'<tool_call>\n<function=read_file>\n<parameter=pth>\na.rs\n</parameter>\n</function>\n</tool_call>',
    # Schema-guided typing: strings stay strings, exact numbers, enums.
    b'<tool_call>\n<function=read_file>\n<parameter=path>\n12345\n</parameter>\n<parameter=limit>\n007\n</parameter>\n</function>\n</tool_call>',
    b'<tool_call>{"name": "get_weather", "arguments": {"city": "true", "unit": " Fahrenheit "}}</tool_call>',
    b'<tool_call>{"name": "write_file", "arguments": {"path": "a", "content": "b", "mode": "Overwrite", "ratio": "1.50", "count": 2.0}}</tool_call>',
    # Repeated parameters: a scalar (never last-wins) and a repeatable one.
    b'<tool_call>\n<function=read_file>\n<parameter=path>\na\n</parameter>\n<parameter=path>\nb\n</parameter>\n</function>\n</tool_call>',
    b'<tool_call>\n<function=delete_path>\n<parameter=path>\na\n</parameter>\n<parameter=path>\nb\n</parameter>\n</function>\n</tool_call>',
    # Reasoning: closed with an embedded call, unclosed, closed at a call opener.
    b'<think>maybe <tool_call>{"name": "list_dir", "arguments": {"path": "."}}</tool_call></think>No call needed.',
    b'<think>planning the edit\n<tool_call>{"name": "read_file", "arguments": {"path": "a.rs"}}</tool_call>',
    b'plan</think>\n<tool_call>{"name": "read_file", "arguments": {"path": "a.rs"}}</tool_call>',
    # Mixed dialects in one message, and invalid UTF-8 in content and arguments.
    b'<tool_call>{"name": "get_weather", "arguments": {"city": "Oslo"}}</tool_call>\n'
    b'<tool_call>\n<function=list_dir>\n<parameter=path>\nsrc\n</parameter>\n</function>\n</tool_call>',
    b'Reading \xff\xfe now.\n<tool_call>{"name": "read_file", "arguments": {"path": "a.rs"}}</tool_call>',
    b'<tool_call>\n<function=write_file>\n<parameter=path>\na\xc3\n</parameter>\n<parameter=content>\n\xe2\x82\n</parameter>\n</function>\n</tool_call>',
    # Llama 3 JSON: a missing final brace at the end and before the next
    # call, a repeated brace, stringified parameters, arguments cut before
    # an end-of-message token, a call after the tag with the `arguments`
    # key, a built-in Python-syntax call after the tag, and a Hermes
    # wrapper around a Llama 3 object.
    b'{"name": "get_weather", "parameters": {"city": "Paris", "unit": "celsius"}',
    b'{"name": "read_file", "parameters": {"path": "a.rs"}\n{"name": "list_dir", "parameters": {"path": "."}}',
    b'{"name": "list_dir", "parameters": {"path": "."}}}',
    b'{"name": "read_file", "parameters": "{\\"path\\": \\"a.rs\\"}"}; {"name": "current_time", "parameters": {}}',
    b'<|python_tag|>{"name": "read_file", "parameters": {"path": "a.rs"<|eom_id|>',
    b'<|python_tag|>{"name": "get_weather", "arguments": {"city": "Lima"}}<|eom_id|>',
    b'<|python_tag|>brave_search.call(query="weather")<|eom_id|>',
    b'<tool_call>\n{"name": "read_file", "parameters": {"path": "a.rs"}}\n</tool_call>',
    # Mistral v3: a missing closing bracket at the end and before the next
    # array, a truncated last object, a bare object, a repeated bracket,
    # duplicate model ids, the "parameters" key, and stringified arguments.
    b'[TOOL_CALLS] [{"name": "read_file", "arguments": {"path": "a.rs"}, "id": "a1B2c3D4e"}, {"name": "list_dir", "arguments": {"path": "."}}',
    b'[TOOL_CALLS] [{"name": "read_file", "arguments": {"path": "a.rs"}}\n[TOOL_CALLS] [{"name": "current_time", "arguments": {}}]',
    b'[TOOL_CALLS] [{"name": "get_weather", "arguments": {"city": "Oslo"}}, {"name": "read_file", "arguments": {"path": "src/ma',
    b'[TOOL_CALLS] {"name": "list_dir", "arguments": {"path": "src"}}',
    b'[TOOL_CALLS] {"name": "list_dir", "arguments": {"path": "src"}}, {"name": "read_file", "arguments": {"path": "a.rs"}}',
    b'[TOOL_CALLS] {"name": "list_dir", "arguments": {"path": "src"}}\n{"name": "current_time", "arguments": {}}\nDone.',
    b'[TOOL_CALLS] {"name": "list_dir", "arguments": {"path": "src"}}\nLike {"path": "src", "depth": 1}.',
    b'[TOOL_CALLS] {"name": "list_dir", "arguments": {"path": "src"}}, {"name": "read_file", "argu',
    b'[TOOL_CALLS][{"name": "list_dir", "arguments": {"path": "."}, "id": "Zz9Yy8Xx7"}]]</s>',
    b'[TOOL_CALLS] [{"name": "read_file", "arguments": {"path": "a"}, "id": "same12345"}, {"name": "read_file", "arguments": {"path": "b"}, "id": "same12345"}]',
    b'[TOOL_CALLS] [{"name": "get_weather", "parameters": {"city": "Lima"}}]',
    b'[TOOL_CALLS] [{"name": "read_file", "arguments": "{\\"path\\": \\"a.rs\\"}"}]',
    # Mistral v11: a missing [ARGS], whitespace around the name, truncated
    # and unclosed arguments, a second call without its [TOOL_CALLS], call
    # ids, prose after a call, reasoning, and a v3 array beside a v11 call.
    b'[TOOL_CALLS]read_file{"path": "a.rs"}',
    b'[TOOL_CALLS] list_dir \n[ARGS] {"path": "."}',
    b'[TOOL_CALLS]read_file[ARGS]{"path": "src/ma',
    b'[TOOL_CALLS]read_file[ARGS]{"path": "a.rs"',
    b'[TOOL_CALLS]read_file[ARGS]{"path": "a.rs"}\nlist_dir[ARGS]{"path": "."}\nDone.',
    b'[TOOL_CALLS]read_file[CALL_ID]a1B2c3D4e[ARGS]{"path": "a"}[TOOL_CALLS]current_time[CALL_ID]a1B2c3D4e[ARGS]{}</s>',
    b'[TOOL_CALLS]read_file[ARGS]{"path": "a.rs"}\nDone. Like {"x": 1}',
    b'[THINK]plan the read[/THINK]Reading.[TOOL_CALLS]read_file[ARGS]{"path": "a.rs"}',
    b'[TOOL_CALLS] [{"name": "list_dir", "arguments": {"path": "."}}]\n[TOOL_CALLS]read_file[ARGS]{"path": "b"}',
    b'[TOOL_RESULTS]a1B2c3D4e[TOOL_CONTENT]ok[/TOOL_RESULTS]',
    # DeepSeek-V3.1: missing section and call closers, truncated arguments,
    # reasoning before the section, ASCII and space renderings of the
    # tokens, an omitted section opener, prose after the section, and a
    # tool output.
    "<｜tool▁calls▁begin｜><｜tool▁call▁begin｜>read_file<｜tool▁sep｜>{\"path\": \"a.rs\"}<｜tool▁call▁end｜><｜tool▁call▁begin｜>list_dir<｜tool▁sep｜>{\"path\": \".\"}<｜tool▁call▁end｜>".encode(),
    "<｜tool▁calls▁begin｜><｜tool▁call▁begin｜>read_file<｜tool▁sep｜>{\"path\": \"a.rs\"}<｜tool▁call▁begin｜>list_dir<｜tool▁sep｜>{\"path\": \".\"}<｜tool▁call▁end｜><｜tool▁calls▁end｜>".encode(),
    "<｜tool▁calls▁begin｜><｜tool▁call▁begin｜>read_file<｜tool▁sep｜>{\"path\": \"src/ma".encode(),
    "<think>plan the read</think><｜tool▁calls▁begin｜><｜tool▁call▁begin｜>read_file<｜tool▁sep｜>{\"path\": \"a.rs\"}<｜tool▁call▁end｜><｜tool▁calls▁end｜>".encode(),
    "<|tool_calls_begin|><|tool_call_begin|>read_file<|tool_sep|>{\"path\": \"a.rs\"}<|tool_call_end|><|tool_calls_end|>".encode(),
    "<|tool calls begin|><|tool call begin|>list_dir<|tool sep|>{\"path\": \".\"}<|tool call end|><|tool calls end|>".encode(),
    "Now: <｜tool▁call▁begin｜>read_file<｜tool▁sep｜>{\"path\": \"a.rs\"}<｜tool▁call▁end｜><｜tool▁calls▁end｜>".encode(),
    "<｜tool▁calls▁begin｜><｜tool▁call▁begin｜>read_file<｜tool▁sep｜>{\"path\": \"a.rs\"}<｜tool▁call▁end｜><｜tool▁calls▁end｜>\nDone.".encode(),
    "<｜tool▁output▁begin｜>ok<｜tool▁output▁end｜>".encode(),
    # DeepSeek-V3 / R1: a missing, bare, repeated or foreign-language fence
    # line, a missing or other type, R1 reasoning, a missing call closer
    # between calls, truncated fenced arguments, a fence inside a string
    # value, and a V3.1 section and a V3 section in one turn.
    "<｜tool▁calls▁begin｜><｜tool▁call▁begin｜>function<｜tool▁sep｜>read_file\n{\"path\": \"a.rs\"}\n```<｜tool▁call▁end｜><｜tool▁calls▁end｜>".encode(),
    "<｜tool▁calls▁begin｜><｜tool▁call▁begin｜>function<｜tool▁sep｜>read_file\n```\n{\"path\": \"a.rs\"}\n```<｜tool▁call▁end｜><｜tool▁calls▁end｜>".encode(),
    "<｜tool▁calls▁begin｜><｜tool▁call▁begin｜>function<｜tool▁sep｜>read_file\n```json\n{\"path\": \"a.rs\"}\n<｜tool▁call▁end｜>".encode(),
    "<｜tool▁calls▁begin｜><｜tool▁call▁begin｜>function<｜tool▁sep｜>read_file\n```json\n```json\n{\"path\": \"a.rs\"}\n```\n```<｜tool▁call▁end｜><｜tool▁calls▁end｜>".encode(),
    "<｜tool▁calls▁begin｜><｜tool▁call▁begin｜>function<｜tool▁sep｜>read_file```javascript{\"path\": \"a.rs\"}```<｜tool▁call▁end｜><｜tool▁calls▁end｜>".encode(),
    "<｜tool▁calls▁begin｜><｜tool▁call▁begin｜>tool<｜tool▁sep｜>read_file\n```json\n{\"path\": \"a.rs\"}\n```<｜tool▁call▁end｜><｜tool▁calls▁end｜>".encode(),
    "<｜tool▁calls▁begin｜><｜tool▁call▁begin｜><｜tool▁sep｜>read_file\n```json\n{\"path\": \"a.rs\"}\n```<｜tool▁call▁end｜><｜tool▁calls▁end｜>".encode(),
    "<think>\nplan the read\n</think>\n\n<｜tool▁calls▁begin｜><｜tool▁call▁begin｜>function<｜tool▁sep｜>read_file\n```json\n{\"path\": \"a.rs\"}\n```\n<｜tool▁call▁begin｜>function<｜tool▁sep｜>list_dir\n```json\n{\"path\": \".\"}\n```<｜tool▁call▁end｜><｜tool▁calls▁end｜>".encode(),
    "<｜tool▁calls▁begin｜><｜tool▁call▁begin｜>function<｜tool▁sep｜>read_file\n```json\n{\"path\": \"src/ma".encode(),
    "<｜tool▁calls▁begin｜><｜tool▁call▁begin｜>function<｜tool▁sep｜>write_file\n```json\n{\"path\": \"R.md\", \"content\": \"```sh\\nls\\n```\"}\n```<｜tool▁call▁end｜><｜tool▁calls▁end｜>".encode(),
    "<｜tool▁calls▁begin｜><｜tool▁call▁begin｜>read_file<｜tool▁sep｜>{\"path\": \"a\"}<｜tool▁call▁end｜><｜tool▁calls▁end｜>\n<｜tool▁calls▁begin｜><｜tool▁call▁begin｜>function<｜tool▁sep｜>list_dir\n```json\n{\"path\": \".\"}\n```<｜tool▁call▁end｜><｜tool▁calls▁end｜>".encode(),
    # Kimi-K2: missing section and call closers, an omitted section opener,
    # the singular section tokens, ids without `:IDX` or the `functions.`
    # prefix, a dotted name, a colon in the name, duplicate ids, Thinking
    # reasoning, truncated arguments, and a tool message.
    b'<|tool_calls_section_begin|><|tool_call_begin|>functions.read_file:0<|tool_call_argument_begin|>{"path": "a.rs"}<|tool_call_end|><|tool_call_begin|>functions.list_dir:1<|tool_call_argument_begin|>{"path": "."}<|tool_call_end|>',
    b'<|tool_calls_section_begin|><|tool_call_begin|>functions.read_file:0<|tool_call_argument_begin|>{"path": "a.rs"}<|tool_call_begin|>functions.list_dir:1<|tool_call_argument_begin|>{"path": "."}<|tool_call_end|><|tool_calls_section_end|>',
    b'Now: <|tool_call_begin|>functions.read_file:0<|tool_call_argument_begin|>{"path": "a.rs"}<|tool_call_end|><|tool_calls_section_end|>',
    b'<|tool_call_section_begin|><|tool_call_begin|>functions.read_file:0<|tool_call_argument_begin|>{"path": "a.rs"}<|tool_call_end|><|tool_call_section_end|>',
    b'<|tool_calls_section_begin|><|tool_call_begin|>functions.read_file<|tool_call_argument_begin|>{"path": "a.rs"}<|tool_call_end|><|tool_call_begin|>list_dir:1<|tool_call_argument_begin|>{"path": "."}<|tool_call_end|><|tool_calls_section_end|>',
    b'<|tool_calls_section_begin|><|tool_call_begin|>functions.mcp.read_file:0<|tool_call_argument_begin|>{"path": "a"}<|tool_call_end|><|tool_call_begin|>functions.ns:read_file:1<|tool_call_argument_begin|>{"path": "b"}<|tool_call_end|><|tool_calls_section_end|>',
    b'<|tool_calls_section_begin|><|tool_call_begin|>functions.read_file:0<|tool_call_argument_begin|>{"path": "a"}<|tool_call_end|><|tool_call_begin|>functions.read_file:0<|tool_call_argument_begin|>{"path": "b"}<|tool_call_end|><|tool_calls_section_end|>',
    b'<think>plan the read</think>Reading.<|tool_calls_section_begin|><|tool_call_begin|>functions.read_file:0<|tool_call_argument_begin|>{"path": "src/ma',
    b'<|im_system|>tool<|im_middle|>## Return of functions.read_file:0\nok<|im_end|>',
    # A colon-namespaced name, and calls naming no tool (a Kimi-K2 bare call
    # counter, a Hermes object without "name") read by their arguments.
    b'<|tool_calls_section_begin|><|tool_call_begin|>functions.mcp:read_file:0<|tool_call_argument_begin|>{"path": "a"}<|tool_call_end|><|tool_calls_section_end|>',
    b'<|tool_calls_section_begin|><|tool_call_begin|>3<|tool_call_argument_begin|>{"city": "Oslo"}<|tool_call_end|><|tool_call_begin|>4<|tool_call_argument_begin|>{"path": "a"}<|tool_call_end|><|tool_calls_section_end|>',
    b'<tool_call>\n{"arguments": {"path": "a.rs", "limit": 5}}\n</tool_call>',
    # Anthropic tool_use blocks: in a message with a text block, in a whole
    # response with a thinking block, reordered, without a type but with a
    # toolu_ id, with "arguments" for "input", stringified input, cut off,
    # without the closing brace, and streaming events.
    b'{"role":"assistant","content":[{"type":"text","text":"ok"},{"type":"tool_use","id":"toolu_01","name":"read_file","input":{"path":"a.rs"}}]}',
    b'{"id":"msg_01","type":"message","role":"assistant","content":[{"type":"thinking","thinking":"plan","signature":"x"},{"type":"tool_use","id":"toolu_01","name":"list_dir","input":{"path":"."}}],"stop_reason":"tool_use"}',
    b'{"name":"read_file","input":{"path":"a"},"id":"toolu_01","type":"tool_use"}',
    b'{"id":"toolu_01","name":"read_file","input":{"path":"a"}}',
    b'{"type":"tool_use","id":"toolu_01","name":"read_file","arguments":{"path":"a"}}',
    b'{"type":"tool_use","id":"toolu_01","name":"read_file","input":"{\\"path\\":\\"a\\"}"}',
    b'{"type":"tool_use","id":"toolu_01","name":"read_file","input":{"path":"src/ma',
    b'{"type":"tool_use","id":"toolu_01","name":"read_file","input":{"path":"a"}',
    b'data: {"type":"content_block_start","index":1,"content_block":{"type":"tool_use","id":"toolu_01","name":"read_file","input":{}}}\n',
    b'{"type":"tool_result","tool_use_id":"toolu_01","content":"ok"}',
    # A Llama 3 JSON object with an id member first or last, and an
    # Anthropic-shaped object inside a Hermes wrapper.
    b'{"id": "call_7", "name": "read_file", "parameters": {"path": "a"}}',
    b'{"name": "read_file", "parameters": {"path": "a"}, "id": "toolu_01"}',
    b'<tool_call>{"type":"tool_use","id":"toolu_01","name":"read_file","input":{"path":"a"}}</tool_call>',
    # Harmony: analysis then a call, the renderer layout, no start prefix, a
    # preamble, <|end|> for <|call|>, a missing end, <|channel>, a recipient
    # without functions., after <|message|>, trailing text, cut-off
    # arguments, built-in browser and python recipients, a tool message.
    b'<|channel|>analysis<|message|>plan<|end|><|start|>assistant<|channel|>commentary to=functions.read_file <|constrain|>json<|message|>{"path":"a"}<|call|>',
    b'<|start|>assistant to=functions.read_file<|channel|>commentary <|constrain|>json<|message|>{"path":"a"}<|call|>',
    b' to=functions.list_dir<|channel|>commentary json<|message|>{"path":"."}<|call|>',
    b'<|channel|>commentary<|message|>Checking.<|end|><|start|>assistant<|channel|>commentary to=functions.read_file<|message|>{"path":"a"}<|end|>',
    b'<|channel>commentary to=read_file <|constrain|>json<|message|>{"path":"a"}',
    b'<|channel|>commentary<|message|>to=functions.read_file {"path":"a"} thanks<|call|>',
    b'<|channel|>commentary to=functions.read_file <|constrain|>json<|message|>{"path":"src/ma',
    b'<|start|>assistant<|channel|>analysis to=browser.search <|constrain|>json<|message|>{"query":"x"}<|call|><|start|>assistant<|channel|>analysis to=python<|message|>print(1)<|call|>',
    b'<|start|>functions.read_file to=assistant<|channel|>commentary<|message|>ok<|end|>',
    # Bare JSON: a call alone, in prose, in a fence, in an array, arguments
    # first with a trailing id, stringified arguments, a call object nested
    # in another's arguments, a bare tool result.
    b'{"name": "read_file", "arguments": {"path": "a"}}',
    b'Reading it.\n{"name": "read_file", "arguments": {"path": "a"}}\nDone.',
    b'Example:\n```json\n{"name": "list_dir", "arguments": {"path": "."}}\n```\n',
    b'[{"name": "read_file", "arguments": {"path": "a"}}, {"name": "list_dir", "arguments": {"path": "b"}}]',
    b'{"arguments": {"path": "a"}, "name": "read_file", "id": "call_9"}',
    b'{"name": "read_file", "arguments": "{\\"path\\": \\"a\\"}"}',
    b'{"name": "write_file", "arguments": {"path": "a", "content": "{\\"name\\": \\"rm\\", \\"arguments\\": {}}"}}',
    b'{"tool_call_id": "call_9", "content": "ok"}',
    # Unparsed markup (reported, never silent content): Pythonic lists, a
    # Gemma 3 tool_code block, a GLM-4.5 call, stripped renderings, and
    # call markup of a built-in dialect outside the allow-list.
    b'[read_file(path="a"), list_dir(path=".")]',
    b'Checking.\n[read_file(path="a")]',
    b'<|python_start|>[read_file(path="a")]<|python_end|>',
    b'```tool_code\nread_file(path="a")\n```',
    b'<tool_call>read_file\n<arg_key>path</arg_key>\n<arg_value>a</arg_value>\n</tool_call>',
    b'functions.read_file:0{"path": "a"}',
    b'commentary to=functions.read_file json{"path": "a"}',
    b'functionread_file\n```json\n{"path": "a"}\n```',
    b'<function=read_file>\n<parameter=path>\na\n</parameter>\n</function>',
    b'read_file{"path": "a"}',
]

# Inputs that hold no recoverable call.
TOOL_CALL_NEGATIVES = [
    b'',
    b'Just prose, no call.',
    b'<tool_call>',
    b'</tool_call></function></parameter>',
    b'<tool_call></tool_call>',
    b'<tool_call>{"name": 5, "arguments": []}</tool_call>',
    b'<tool_call>\n<function=>\n</function>\n</tool_call>',
    b'<parameter=path>\na.rs\n</parameter>',
    b'{"role": "assistant", "content": "hi", "tool_calls": []}',
    b'[' * 64 + b'{"name": "read_file"' + b']' * 64,
    b'\xff\xfe\xfd',
    b'<think></think><think>',
    # JSON content that is not call-shaped stays content.
    b'Record: {"name": "Ada", "age": 36} and {"user": {"name": "Bob"}}',
    b'{"name": "read_file"' + b'{' * 64,
    b'[TOOL_CALLS]',
    b'[TOOL_CALLS]read_file',
    b'[TOOL_CALLS]read_file[CALL_ID][ARGS][ARGS]',
    b'[THINK][TOOL_CALLS][/THINK]',
    "<｜tool▁calls▁begin｜>".encode(),
    "<|tool calls begin|> nothing".encode(),
    "<｜tool▁call▁begin｜>read_file<｜tool▁sep｜>".encode(),
    "<｜tool▁calls▁begin｜><｜tool▁call▁begin｜>function<｜tool▁sep｜>\n```json\n```".encode(),
    b'<|tool_calls_section_begin|>',
    b'<|tool_call_begin|>functions.:0<|tool_call_argument_begin|>',
    b'{"name":"read_file","input":{"path":"a"}}',
    b'{"type":"tool_use"}',
    b'<|start|>assistant<|channel|>final<|message|>Done.<|return|>',
    b'<|channel|>commentary to=<|message|>',
    b'{"name": "read_file", "arguments": 3} {"name": "Ada", "arguments": "she argued"}',
    b'{"arguments": {"path": "a"}} {"name": "read_file", "arguments": {"path": "a"}, "mode": 1}',
    b'{"name": "read_file", "arguments": {"type": "object", "properties": {"path": {"type": "string"}}}}',
    b'{"name": "read_file", "description": "Read", "parameters": {"type": "object", "properties": {}}}',
    b'{"name": "read_file", "arguments": {"path": "a"',
    b'f(x) = 2x, see [1]; [a(b)] is math and `g(a=1)` inline code',
    b'```python\nprint(x)\n```',
    "```json\n{\"path\": \"a.rs\"}\n```".encode(),
    b'[TOOL_CALLS] [' + b'{"name": "read_file", "arguments": ' * 16 + b']',
    b'[TOOL_RESULTS] {"content": "x", "call_id": "a1B2c3D4e"}[/TOOL_RESULTS]',
]

# Exact probe inputs (header included) kept as seeds: inputs that once
# failed an invariant, and the exit-gate states the invariants must keep
# guarding (a usable call beside a failed tool choice, a guess on a mutating
# tool, envelope syntax slips, bare Qwen3-Coder elements, invalid UTF-8
# inside a call).
TOOL_CALL_REGRESSIONS = [
    # A forced read_file choice violated beside a read_file call: the call
    # is skipped as tool_choice_failed (fuzz-tool-call-u8m4d7hy, F7).
    b'\xe6#Let me check both.\n[{"name": "get_weather", "arguments": {"city": "Oslo"}}, {"name": "read_file", "arguments": {"path": "notes.txt", "limit": 10}}]\nAnd Bergen:\n{"arguments": {"city": "Bergen", "unit": "celsius"}, "name": "get_weather"}',
    # Forced read_file, Resolved read_file beside an extra call.
    b'\xc0\x52<tool_call>{"name": "read_file", "arguments": {"path": "a"}}</tool_call>\n'
    b'<tool_call>{"name": "current_time", "arguments": {}}</tool_call>',
    # A guess on a mutating tool: invalid UTF-8 in write_file's argument.
    b'\x00\x52<tool_call>{"name": "write_file", "arguments": {"path": "a", "content": "x\xffy"}}</tool_call>',
    # The same under a length stop, and a near-miss mutating name under Tolerant.
    b'\x04\x52<tool_call>{"name": "write_file", "arguments": {"path": "a", "content": "x\xffy"}}</tool_call>',
    b'\x02\x52<tool_call>{"name": "write_fil", "arguments": {"path": "a", "content": "x"}}</tool_call>',
    # JSON envelope slips (Round B): single quotes, bare keys, a trailing
    # comma and a comment.
    b"\x00\x52<tool_call>{'name': 'read_file', 'arguments': {'path': 'a'},}</tool_call>",
    b'\x00\x52{/* c */ name: "read_file", arguments: {"path": "a"}}',
    # A bare Qwen3-Coder element (Round A), and invalid UTF-8 in a tool name.
    b'\x00\x52<function=read_file>\n<parameter=path>\na\n</parameter>\n</function>',
    b'\x00\x52<tool_call>\n<function=read_fil\xe9>\n<parameter=path>\na\n</parameter>\n</function>\n</tool_call>',
    # Anthropic's spaced redacted_thinking opener ends in the quote that
    # begins the closer `"}`: the closer overlaps the opener, so it does not
    # close the block (its text must never be a reversed range), under the
    # default and the open-at-start reasoning modes.
    b'\x00\x52{"type": "redacted_thinking", "data": "}x',
    b'\x10\x52{"type": "redacted_thinking", "data": "}x',
    # S1: a turn that starts inside a reasoning block (control bits 4-5 = 1)
    # must not let Anthropic's explicit closer `"}` inside a call's JSON end
    # the block and swallow the call; nor end it inside the call before its
    # own `</think>`.
    b'\x10\x52Let me look. <tool_call>{"name":"read_file","arguments":{"path":"x"}}</tool_call>',
    b'\x10\x52<think>hmm {"name":"read_file","arguments":{"path":"x"}}</think>done',
    b'\x10\x52hmm [TOOL_CALLS] [{"name": "read_file", "arguments": {"path": "x"}}] done</think>'
    b'<|python_tag|>{"name": "read_file", "parameters": {"path": "y"}}',
    # S3: a rejected attempt keeps its own closing markup (trailing text in
    # the wrapper, an empty name, a nested function, stray closers after it,
    # and a broken wrapper around a bare JSON call under a length stop).
    b'\x00\x52<tool_call>\n<function=read_file>\n<parameter=path>\na\n</parameter>\nand more</function>\n</tool_call>',
    b'\x00\x52<function=>x</function> after',
    b'\x00\x52<tool_call>\n<function=read_file>\n<function=read_file>\n</function>\n</tool_call>',
    b'\x00\x52<tool_call>\n<function=read_file>\nhi</function>\n</tool_call>\n</tool_call> tail',
    b'\x04\x52<tool_call>{"name": "read_file", "arguments": {"path": "a"}} junk junk</tool_call>',
    # F4: without its closer a rejected attempt keeps only what its grammar
    # read; the prose after a broken opener stays content.
    b'\x00\x52Sure, I could call <function=read_file but actually no tool is needed. The answer is 42.',
    # A wrapper broken at its tool name whose own closer is cut before the
    # next wrapper opens: it keeps only what it read, and the inner
    # `</function>` left as content is not a closer of the wrapper.
    b'\x00R<tool_call>\n<function=read_fil\xe9>\n<parameter=path>\na\n</parameter>\n</function>\n</tooR<tool_call>\n<function=read_fil\xe9>\n<parameter=path>\na\n</parameter>\n</funcfunctiontion>\n</tool_call>',
    # A section-less Kimi-K2 attempt chaining calls, broken in its second:
    # a repeated <|tool_call_begin|> continues it, so it runs to the
    # section closer rather than leaving it as content.
    b'\x00\x52Let me check. <|tool_call_begin|>functions.read_file:0<|tool_call_argument_begin|>{"path": "a"}<|tool_call_end|>'
    b'<|tool_call_begin|>functions.read_file:1 oops<|tool_call_argument_begin|>{"path": "b"}<|tool_call_end|><|tool_calls_section_end|> done',
    # T1/F5: two attempts read the same call inside a starts_open block; it
    # is embedded once, in span order.
    b'\x10\x52[TOOL_CALLS]read_file{"path":"a"}[TOOL_CALLS]current_time{}[/THINK]Answer.',
    # T1/F6: a call's own closer (Kimi-K2 <|tool_call_end|>, DeepSeek
    # <｜tool▁call▁end｜>) ends a rejected attempt with no section closer;
    # the prose after it stays content.
    b'\x00\x52Hi <|tool_calls_section_begin|><|tool_call_begin|>functions.read_file:0 oops<|tool_call_argument_begin|>{"path":"a"}<|tool_call_end|> The answer is 42.',
    'Hi <｜tool▁calls▁begin｜><｜tool▁call▁begin｜>function<｜tool▁sep｜>read_file oops\n```json\n{"path":"a"}\n```<｜tool▁call▁end｜> The answer is 42.'.encode().join([b'\x00\x52', b'']),
    b'\x00\x52<|tool_call_begin|>functions.read_file:0 oops<|tool_call_argument_begin|>{"path":"a"}<|tool_call_end|><|tool_call_begin|>functions.read_file:1<|tool_call_argument_begin|>{"path":"b"}<|tool_call_end|> done',
    # T1/F7: a rejected span never ends inside a literal (`</function` of
    # `</function>`), bare, with newlines, and wrapped.
    b'\x00\x52pre <function=read_file><function=read_file><parameter=path>1</parameter></function> post',
    b'\x00\x52pre <function=read_file>\n<function=read_file>\n<parameter=path>\n1\n</parameter>\n</function> post',
    b'\x00\x52pre <tool_call><function=read_file><function=read_file><parameter=path>1</parameter></function></tool_call> post',
    # T1/F8: a reading matches `{` through a deleted stray `/` before it
    # fails, so `at` lies one byte past the span, inside that literal.
    b'\xe6#[THINK][TOOL_CALLS][/TH"[/TH"id"',
    # T2: a malformed call in the middle of an attempt reading several
    # calls; the calls before it are recovered (group-split), the third
    # where the grammar resynchronizes.
    b'\x00\x52Hi <|tool_calls_section_begin|><|tool_call_begin|>functions.read_file:0<|tool_call_argument_begin|>{"path":"a"}<|tool_call_end|>'
    b'<|tool_call_begin|>functions.read_file:1 oops<|tool_call_argument_begin|>{"path":"b"}<|tool_call_end|>'
    b'<|tool_call_begin|>functions.read_file:2<|tool_call_argument_begin|>{"path":"c"}<|tool_call_end|><|tool_calls_section_end|> done',
    'Hi <｜tool▁calls▁begin｜><｜tool▁call▁begin｜>read_file<｜tool▁sep｜>{"path":"a"}<｜tool▁call▁end｜><｜tool▁call▁begin｜>read_file oops<｜tool▁sep｜>{"path":"b"}<｜tool▁call▁end｜><｜tool▁call▁begin｜>read_file<｜tool▁sep｜>{"path":"c"}<｜tool▁call▁end｜><｜tool▁calls▁end｜> done'.encode().join([b'\x00\x52', b'']),
    b'\x00\x52Hi [TOOL_CALLS] [{"name": "read_file", "arguments": {"path": "a"}}, {"name": "read_file" oops, "arguments": {"path": "b"}}, {"name": "read_file", "arguments": {"path": "c"}}] done',
    b'\x04\x52Hi [TOOL_CALLS] [{"name": "read_file", "arguments": {"path": "a"}}, {"name": "read_file" oops, "arguments": {"path": "b"}}, {"name": "read_file", "argum',
    # T2: misspelled closers read as the one closer they misspell, and a
    # token one edit from two closers left alone.
    b'\x00\x52<tool_call>\n<function=read_file>\n<parameter=path>\na\n</paramete>\n</functio>\n</tool_call> The answer.',
    b'\x00\x52<tool_call>{"name": "read_file", "arguments": {"path": "a"}}</tool_cal> The answer.',
    'Hi <｜tool▁calls▁begin｜><｜tool▁call▁begin｜>read_file<｜tool▁sep｜>{"path":"a"}<｜tool▁callz▁end｜><｜tool▁calls▁end｜> The answer.'.encode().join([b'\x00\x52', b'']),
    # T2: a reasoning marker is never read as a call's arguments.
    b'\x10\x52[TOOL_CALLS]read_file[TOOL_CALLS]current_time[/THINK]Answer.',
    # T3: a resynchronizing Mistral v3 list (the v11 rejection at the shared
    # opener outread), written [ARGS] never inserted, an unreadable JSON
    # value rejected to its balancing bracket, fences inside rejected markup.
    b'\x00\x52Hi [TOOL_CALLS] [{"name": "read_file", "arguments": {"path": "a"}}, {"name": "read_file" oops, "arguments": {"path": "b"}}, {"name": "read_file", "arguments": {"path": "c"}}] done',
    b'\x00\x52Hi [TOOL_CALLS]read_file[ARGS]{"path":"a",} The answer.',
    b'\x00\x52Hi [TOOL_CALLS]read_file[ARGS]{"path": ::: } The answer.',
    b'\x00\x52<tool_call>{"name": "read_file", "arguments": {"path": "a"}}</tool_call>\n<tool_call>\n```json\n{"name": "read_file" oops "arguments": {"path": "b"}}\n```\n</tool_call>\n<tool_call>{"name": "read_file", "arguments": {"path": "c"}}</tool_call>',
    b'\x00\x52<tool_call>{"name": "read_file", "arguments": {"path": "a"}}</tool_call>\n<tool_call>\n```json\n{"name": "read_file" oops "arguments": {"path": "b"}}\n</tool_call>\n<tool_call>{"name": "read_file", "arguments": {"path": "c"}}</tool_call>',
    # Mistral v11 [CALL_ID] written where [ARGS] could be read as omitted.
    b'\x00\x52[TOOL_CALLS]read_file[CALL_ID]call_1[ARGS]{"path":{"a":',
    # An OpenAI object whose name string is cut: its rejection runs to the
    # bracket balancing the value it opened (the F4 oracle admits it).
    b'\x05\x91[\n  {\n    "id": "call_a1",\n    "type": "function",\n    "function": {\n      "name": "write_fi  "arguments": "{\\"path\\": \\"src\\"}"\n    }\n  }\n]',
    # A Hermes attempt stops inside the key literal `"id"` with Harmony
    # markup after it: the attempt owns the literal before unparsed markup
    # is scanned, so the report never starts inside it.
    b'\x00R<tool_call>{"id":"call_305c487f",\t"name":\t\t"delete_pa"id":th"\t\t,"arguments"\t:"{\\"path\\":[\\"beta gamma \\\\n  block\\\\n\\",\\"\\\\n  block\\\\n\\"]}"}\t< to=functions./tool_call>\n<tool_call>{\t\t"type":"function"\t,\t\t"id":\t"call_07be3ff1","name"\t:\t"read_file"\t\t,\t\t"arguments":"{\\"path\\":\\"\\\\n  block\\\\n alpha\\",\\"limit\\":-209}"\t\t}\t\t</tool_call>',
    # T4: bad-first groups (every dialect with `next`), all-bad, a bad call
    # without its closer or with unbalanced JSON, a section left open before
    # another, misspelled in-group tokens, and opening reasoning markers
    # that cut a call or a rejected attempt.
    b'\x00RHi [TOOL_CALLS] [{"name": "f" oops, "arguments": {"a": "x"}}, {"name": "f", "arguments": {"a": "y"}}, {"name": "f", "arguments": {"a": "z"}}] done',
    b'\x00RHi <|tool_calls_section_begin|><|tool_call_begin|>functions.f:0 oops<|tool_call_argument_begin|>{"a":"x"}<|tool_call_end|><|tool_call_begin|>functions.f:1<|tool_call_argument_begin|>{"a":"y"}<|tool_call_end|><|tool_call_begin|>functions.f:2<|tool_call_argument_begin|>{"a":"z"}<|tool_call_end|><|tool_calls_section_end|> done',
    b'\x00R<|tool_call_begin|>functions.f:0 oops<|tool_call_argument_begin|>{"a":"x"}<|tool_call_end|><|tool_call_begin|>functions.f:1<|tool_call_argument_begin|>{"a":"y"}<|tool_call_end|><|tool_call_begin|>functions.f:2<|tool_call_argument_begin|>{"a":"z"}<|tool_call_end|><|tool_calls_section_end|> done',
    'Hi <｜tool▁calls▁begin｜><｜tool▁call▁begin｜>function<｜tool▁sep｜>f oops\n```json\n{"a":"x"}\n```<｜tool▁call▁end｜>\n<｜tool▁call▁begin｜>function<｜tool▁sep｜>f\n```json\n{"a":"y"}\n```<｜tool▁call▁end｜>\n<｜tool▁call▁begin｜>function<｜tool▁sep｜>f\n```json\n{"a":"z"}\n```<｜tool▁call▁end｜><｜tool▁calls▁end｜> done'.encode().join([b'\x00\x52', b'']),
    'Hi <｜tool▁calls▁begin｜><｜tool▁call▁begin｜>f oops<｜tool▁sep｜>{"a":"x"}<｜tool▁call▁end｜><｜tool▁call▁begin｜>f<｜tool▁sep｜>{"a":"y"}<｜tool▁call▁end｜><｜tool▁call▁begin｜>f oops<｜tool▁sep｜>{"a":"z"}<｜tool▁call▁end｜><｜tool▁call▁begin｜>f<｜tool▁sep｜>{"a":"w"}<｜tool▁call▁end｜><｜tool▁calls▁end｜> done'.encode().join([b'\x00\x52', b'']),
    b'\x00RHi [TOOL_CALLS]f[ARGS]{"a": ::: }[TOOL_CALLS]f[ARGS]{"a": ::: }[TOOL_CALLS]f[ARGS]{"a":"z"} done',
    b'\x00R<|tool_call_begin|>functions.f:0 oops<|tool_call_argument_begin|>{"a":"x"}<|tool_call_end|><|tool_call_begin|>functions.f:1 oops<|tool_call_argument_begin|>{"a":"y"}<|tool_call_end|><|tool_call_begin|>functions.f:2 oops<|tool_call_argument_begin|>{"a":"z"}<|tool_call_end|><|tool_calls_section_end|> done',
    b'\x00RHi <|tool_calls_section_begin|><|tool_call_begin|>functions.f:0<|tool_call_argument_begin|>{"a":"x"}<|tool_call_end|><|tool_call_begin|>functions.f:1 oops<|tool_call_begin|>functions.f:2<|tool_call_argument_begin|>{"a":"z"}<|tool_call_end|><|tool_calls_section_end|> done',
    b'\x00RHi <|tool_calls_section_begin|><|tool_call_begin|>functions.f:0<|tool_call_argument_begin|>{"a":"x"}<|tool_call_end|><|tool_call_begin|>functions.f:1<|tool_call_argument_begin|>{"a": ::: <|tool_call_end|><|tool_call_begin|>functions.f:2<|tool_call_argument_begin|>{"a":"z"}<|tool_call_end|><|tool_calls_section_end|> done',
    b'\x00RHi [TOOL_CALLS] [{"name": "f", "arguments": {"a": "x"}}, {"name": "f" oops, {"name": "f", "arguments": {"a": "z"}}] done',
    '<｜tool▁calls▁begin｜><｜tool▁call▁begin｜>\tf\t<｜tool▁sep｜>{"a":"x"}\t<｜tool▁call▁end｜>\t\t\n<｜tool▁calls▁begin｜>\t<｜tool▁call▁begin｜>\tf\t<｜tool▁sep｜>{"a":"y"}\t<｜tool▁call▁end｜><｜tool▁calls▁end｜>'.encode().join([b'\x00\x52', b'']),
    b'\x00RHi <|tool_calls_section_begin|><|tool_call_begin|>functions.f:0<|tool_call_argument_begin|>{"a":"x"}<|tool_call_end|><|tool_call_begn|>functions.f:1<|tool_call_argument_begin|>{"a":"y"}<|tool_call_end|><|tool_calls_section_end|> done',
    b'\x00R<tool_call>\n<function=read_file>\n<paramter=path>\na\n</parameter>\n</function>\n</tool_call> done',
    b'\x00R[TOOL_CALLS]read_file[THINK]thinking[/THINK]Answer.',
    b'\x00R[TOOL_CALLS]read_file[ARGS]{"path":"a"[THINK]thinking[/THINK]Answer.',
    b'\x00RHi <tool_call>{"name": "read_file", "arguments": {"path": "a<think>hmm</think> answer',
    # T4: closers written after a reasoning block a call ran into, a
    # rejected attempt's misspelled closer, and more misspelled tokens.
    b'\x04RHi [TOOL_CALLS] [{"name": "f" oops, "arguments": {"a": "x"}}, {"name": "f", "arguments": {"a": "y"}}, {"name": "f", "argum',
    b'\x00RHi <tool_call>{"name": "f" oops "arguments": {"a": "x"}}</tool_cal> The answer.',
    b'\x00RHi <tool_call>{"name": "f", "arguments": {"a": "x"}}<think>hmm</think></tool_call> done',
    b'\x00R<think>x <tool_call>{"name": "f", "arguments": {"a": "x"}}</think></tool_call> done',
    b'\x00R<think>talking about </tool_call> tags</think></tool_call> done',
    b'\x00RHi [TOOL_CALLS]f[ARGS]{"a":"x"}[TOOL_CALS]f[ARGS]{"a":"y"} done',
    b'\x00R<tool_call>\n<functon=read_file>\n<parameter=path>\na\n</parameter>\n</function>\n</tool_call> done',
    b'\x00RHi <|tool_calls_section_begin|><|tool_call_begin|>functions.f:0<|tool_call_argument_begn|>{"a":"x"}<|tool_call_end|><|tool_calls_section_end|> done',
    b'\x00RHi <tool_call>{"name": "f" oops, "arguments": {"a": "x"}}<think>hmm</think></tool_call> done',
    b'\x00R<|tool_call_begin|>functions.f:0 oops<|tool_call_argument_begin|>{"a":"x"}<|tool_call_end|><|tool_calls_section_end|> done',
    b'\x00R[TOOL_CALLS]read_file[ARGS]{"path":"[THINK]"}[THINK]x[/THINK]Answer.',
    # T5: canonical values holding near-closer text (a repair never
    # overrides a reading that needs none), prose after unclosed arguments
    # and stray words (no invented keys), rejections that stop before prose,
    # misspelled first openers, string-aware Kimi resynchronization.
    b'\x00R<tool_call>\n<function=write_file>\n<parameter=path>\nx.xml\n</parameter>\n<parameter=content>\n<Parameter name="a">1</Parameter>\n</parameter>\n</function>\n</tool_call>',
    b'\x00R<tool_call>\n<function=write_file>\n<parameter=path>\nx.xml\n</parameter>\n<parameter=content>\n<paramete>1</paramete>\n<parameter_>2</parameter_>\n</parameter>\n</function>\n</tool_call>',
    b'\x00R<tool_call>\n<function=write_file>\n<parameter=path>\nx.xml\n</parameter>\n<parameter=content>\nWrap in <tool_calls> and </tool_calls> blocks.\n</parameter>\n</function>\n</tool_call>',
    b'\x00R<tool_call>{"name":"write_file","arguments":{"path":"a","content":"x </tool_call> and </tool_cal> y"}}</tool_call>',
    b'\x00R<|tool_calls_section_begin|><|tool_call_begin|>functions.write_file:0<|tool_call_argument_begin|>{"path":"a","content":"x <|tool_call_end|> and <|tool_call_ed|> y"}<|tool_call_end|><|tool_calls_section_end|>',
    b'\x00RChecking. [TOOL_CALLS]read_file[ARGS]{"path":"a.txt" oops. Sorry, my answer is 42 (see {1,2} set).',
    b'\x00RChecking. [TOOL_CALLS] [{"name":"read_file","arguments":{"path":"a.txt" oops. Sorry, my answer is 42 (see {1,2} set).',
    b'\x00R<tool_call>{"name": "f", "arguments": {"a": "y" oops}}</tool_call>',
    b'\x00R[TOOL_CALLS] [{"name": "read_file" oops. Sorry, I cannot call tools here. Options were {a, b}, final answer: 42.',
    b'\x00R[TOOL_CALLS]read_file[ARGS]{"path":"x" oops. Sorry, my answer: use a mapping like m[k] = v} and done.',
    b'\x00R[TOOL_CALL]f[ARGS]{"a":"x"}',
    b'\x00R<\xef\xbd\x9ctool\xe2\x96\x81call\xe2\x96\x81begn\xef\xbd\x9c>f<\xef\xbd\x9ctool\xe2\x96\x81sep\xef\xbd\x9c>{"a":"x"}<\xef\xbd\x9ctool\xe2\x96\x81call\xe2\x96\x81end\xef\xbd\x9c>',
    b'\x00R<tool_cal>{"name": "f", "arguments": {"a": "x"}}</tool_call>',
    b'\x00RThe tool_call field, the TOOL_CALLS header and <tool_calling> docs say: call tools with care.',
    b'\x00R<|tool_calls_section_begin|><|tool_call_begin|>functions.f:0 oops<|tool_call_argument_begin|>{"a":"x<|tool_call_end|><|tool_call_begin|>functions.g:1<|tool_call_argument_begin|>{}<|tool_call_end|>"}<|tool_call_end|><|tool_calls_section_end|>',
    b'\x00R<tool_call>\n<function=f>\n<parameter=a>\n1\n</parameter>\n</functions> bye',
    b'\x00R<tool_call>\n<function=f>\n<parameter=a>\n1\n</parameter>\n</functions>',
    b'\x04R<tool_call>\n<function=f>\n<parameter=a>\n1\n</parameter>\n</functions>',
    b'\x00R[TOOL_CALLS]f[ARGS]{a: "x"}',
    # An Anthropic attempt at `{`, the opener a tool_use block and a
    # streaming event share: it ends at the block's `}` as well as the
    # event's NUL (a campaign failure; the golden
    # anthropic_rejection_shares_opener_closers).
    b'\x05\x91{"type":"tool_result","tool_use_id":"toolu_0functionontent":"ok"\x00}',
    # A Mistral v3 group whose bad first element's bracket scan passes over
    # the calls read after it: the last one closes the array, so the stray
    # `]` after the text closes nothing of the attempt (a campaign failure;
    # the golden mistral3_group_bad_good_good_then_bracket).
    b'[[\x00RHi [TOOL_CALLS] [{"name": "f" oops, "arguments": {"a": "x"}}, {"name": "f", "arguments": {"a": "y"}}, {"name": "f", "arguments": {"a": "z"}}] done]]',
    # A Llama 3 attempt inside a JSON string: a bracket closing nothing it
    # opened closes the structure around it and ends no part of it (a
    # campaign failure; the golden llama3_rejection_stops_at_enclosing_bracket).
    b'[\xe6#{\t\t"type":"tool_use","id"<|tool_call_argument_begin|>\t\t:\t"call_22bb690f",\t\t"name"\t:\t\t"write_file",\t"input":"{\\"pat<|python_tag|>\\"ratio\\":39.32,\\"count\\":-415,\\"options\\":{\\"backup\\":false}}"\t}]',
    # A DeepSeek v3.1 attempt read on past its failure to the section
    # closer: brackets are balanced only over the bytes its grammar read to
    # where it stopped, so the `]` after the closer stays content (a
    # campaign failure; the golden deepseek31_rejection_balances_only_read_brackets).
    b'[[\x9c\xf0<\xef\xbd\x9ctool\xe2\x96\x81calls\xe2\x96\x81begin\xef\xbd\x9c><\xef\xbd\x9ctool\xe2\x96\x81call\xe2\x96\x81begin\xef\xbd\x9c>delete_path<\xef\xbd\x9ctool\xe2\x96\x81sep\xef\xbd\x9c>"{\\"pat\x9c\xf0<\xef\xbd\x9ctool\xe2\x96\x81calls\xe2\x96\x81begin\xef\xbd\x9c><\xef\xbd\x9ctool\xe2\x96\x81call\xe2\x96\x81begin\xef\xbd\x9c>delete_path<\xef\xbd\x9ctool\xe2\x96\x81sep\xef\xbd\x9c>"{\\"path\\":[\\"beta gamma \\\\n  block\\\\n\\""parameters":call\xe2\x96\x81begin\xef\xbd\x9c>\t\tread_file\t\t<\xef\xbd\x9ctool\xe2\x96\x81sep\xef\xbd\x9c>"{\\"path\\":\\"tab\\\\tseparated   indented\\"}"<\xef\xbd\x9ctool\xe2\x96\x81call\xe2\x96\x81end\xef\xbd\x9c>\t<\xef\xbd\x9ctool\xe2\x96\x81calls\xe2\x96\x81end\xef\xbd\x9c>]]',
    # A Llama 3 attempt stopped by prose after its opener: its bracket scan
    # ends at the prose, so the JSON after it is no part of the rejection
    # (a campaign failure; the golden llama3_rejection_stops_before_prose).
    b'[\x05\x91{<<|python_tag|>t_begin|>"name": "get_weather", "arguments": {"city": "Oslo", "unit": "celsius"}}]',
    # A Llama 3 attempt whose markup runs over a JSON object opened after its
    # failure (no prose between): F4 admits the extension.
    b'\x00RHi <tool_cal</tool_call>l<|python_tag|>>{"name": "f" oops, "arguments": {"a": "x"}}<think>hmm</think></tool_cal></tool_call> done',
    # An attempt reading several calls begins inside a starts_open block
    # and ends after its closer: the calls it read inside stay embedded.
    b'\x5b\x9c[TOOL_CALLS]t[TOOL_CALLS]S[/THINK][TOOL_CALLS]g',
    b'\x00\x52<function=read_file<oops. Now the real one: <tool_call>\n<function=read_file>\n<parameter=path>\na\n</parameter>\n</function>\n</tool_call> bye',
    # T6: closer text a value holds: `<parameter>v</parameter>` (the codec weighs every closer).
    b'\\x00R<tool_call>\n<function=write_file>\n<parameter=path>\na.md\n</parameter>\n<parameter=content>\nx <parameter>v</parameter> y\n</parameter>\n</function>\n</tool_call>',
    # T6: the call's closers inside a value (the engine reads on past each closer).
    b'\\x00R<tool_call>\n<function=write_file>\n<parameter=path>\na.md\n</parameter>\n<parameter=content>\nEnd with </function></tool_call> then stop.\n</parameter>\n</function>\n</tool_call>',
    # T6: a value ending exactly like a call's closers: Ambiguous.
    b'\\x00R<tool_call>\n<function=write_file>\n<parameter=path>\na.md\n</parameter>\n<parameter=content>\nx\n</parameter>\n</function>\n</tool_call>\n</parameter>\n</function>\n</tool_call>',
    # T6: closer text and a missing `</tool_call>` before text: the stalled fork's repair.
    b'\\x00R<tool_call>\n<function=write_file>\n<parameter=path>\na.md\n</parameter>\n<parameter=content>\nx </function></tool_call> y\n</parameter>\n</function>\nDone.',
    # T6: closer text and a missing `</tool_call>` at the end.
    b'\\x00R<tool_call>\n<function=write_file>\n<parameter=path>\na.md\n</parameter>\n<parameter=content>\nx </function></tool_call> y\n</parameter>\n</function>',
    # T6: a doubled `</parameter>` and text between closers.
    b'\\x00R<tool_call>\n<function=write_file>\n<parameter=content>\nx\n</parameter>\nmore text\n</parameter>\n</parameter>\n<parameter=path>\na.md\n</parameter>\n</function>\n</tool_call>',
    # T6: two calls of different tools: a value never runs past another call's opener.
    b'\\x00R<tool_call>\n<function=write_file>\n<parameter=path>\na.md\n</parameter>\n<parameter=content>\nx\n</parameter>\n</function>\n</tool_call>\n<tool_call>\n<function=read_file>\n<parameter=path>\nb.md\n</parameter>\n</function>\n</tool_call>',
    # T6: a bare function whose value holds `</function>`.
    b'\\x00R<function=write_file>\n<parameter=path>\na.md\n</parameter>\n<parameter=content>\nsee </function> here\n</parameter>\n</function>',
    # T6: a reasoning block then stray closers: a value never runs past its marker.
    b'\\x00R<tool_call>\n<function=write_file>\n<parameter=path>\na.md\n</parameter>\n<parameter=content>\nx\n</parameter>\n</function>\n</tool_call>\n<think>plan</think>\n</parameter>\n</function>\n</tool_call>',
    # T6: Harmony Python code holding `<|call|>`.
    b'\\x00R<|start|>assistant<|channel|>analysis to=python<|message|>print("<|call|>")\nprint(1)<|call|>',
    # A Kimi-K2 attempt that skipped several calls runs to where the next
    # call's opener begins, that call itself rejected (a campaign failure).
    b'\x00R<|tool_call_begin|>functions.f:0 oops<|tool_call_argument_begin|>{"a":"x"}<|tool_call_end|><|tool_call_begin|>functions.f:1<|tool_call_argument_begin|>{"a":"y"}<|tool_call_end|><|tool_call_begin|>functi<|channeall_end|><|tool_call_begin|>functions.f:1<|tool_call_argument_begin|>{"a":"y"}<|tool_call_end|><|tool_call_begin|>functi<|channel|>one',
    # The last part of a split Mistral v3 list runs to the list's `]`,
    # which closes the `[` its attempt opened before the part (a campaign
    # failure).
    b'\xe6#[TOOL_CALLS][\t{"id":"call_90d7a28a",\t"arguments":{"city":"\\n  block\\n src/lib.rs","unit":"fahrenheit"},\t\t"name"\t:\t\t"get_weather"\t\t},{"id"\t\t:"call_607e2c86"\t,"argum<\xef\xbd\x9ctool\xe2\x96\x81call\xe2\x96\x81end\xef\xbd\x9c>\\tseparated \\n  block\\n"}\t\t,"name"\t:"list_dir"\t}\t\t]',
    # A split Mistral v3 list whose parts write stray `}`s: a `}` never
    # closes the list's `[`, so the last part ends at the `]` that does
    # (a campaign failure).
    b'[\xe6#[TOOL_CALLS][\t{"id":"call_90d7a28a",\t"arguments":{"city":"\\n  block\\n src/lib.rs","unit":"fahrenheit"},\t\t"name"\t:\t\t"get_weather"\t\t},{"id"\t\t:"call_607e2c86"\t,"argum<\xef\xbd\x9ctool\xe2\x96\x81call\xe2\x96\x81end\xef\xbd\x9c>\\tseparated \\n  block\\n"}\t\t,"name"\t:"list_t"},\t\t"name"\t:\t\t"get_weather"\t\t},{"id"\t\t:"call_607e2c86"\t,"argum<\xef\xbd\x9ctool\xe2\x96\x81call\xe2\x96\x81end\xef\xbd\x9c>\\tseparated \\n  block\\n"}\t\t,"name"\t:"list_dir"\t}\t\t]]',
    # A rejected Mistral v3 list part followed by a stray `}`: brackets
    # pair by kind, so the stray `}` never ends the markup and the part
    # runs to the list's `]` (a campaign failure).
    b'[\xe6#[TOOL_CALLS][\t{"id":"call_90d71.50ated \\n  block\\n"}\t\t,"name"\t:"list_t"},\t\t"name"\t:\t\t"get_weathe<\xef\xbd\x9ctool\xe2\x96\x81call\xe2\x96\x81end\xef\xbd\x9c>\x96ed \\n  block\\n"}\t\t,"name"\t:"list_dir"\t}\t\t]]',
    # A rejected Llama 3 attempt whose markup covers a Kimi-K2 attempt
    # written inside a broken key (`"in<|tool_calls_section_begin|>put"`):
    # the nested attempt is markup, so the word before it is no prose and
    # the markup runs to the bracket that closes it (a campaign failure).
    b'[\xe6#{\t\t"type":"tool_use","id"<|tool_call_argument_begin|>\t\t:\t"call_22bb690f",\t\t"name"\t:\t\t"write_file",\t"input":"{\\"pat<|python_tag|>\\"ratio\\":39.32,\\"count\\":-415,\t\t:\t"call_22bb690f",\t\t"name"\t:\t\t"write_file",\t"in<|tool_calls_section_begin|>put":"{\\"pat<|python_tag|>\\"ratio\\":39.32,\\"count\\":-415,\\"options\\":{\\"backup\\":false}}"\t}]',
    # A rejected Qwen attempt whose `</function>` closer an opening
    # `<think>` cuts (`</functio<think>`): it ends at the marker (a
    # campaign failure).
    b'\x00R<function=read_file>\n<parameter=path>\na\n</parameter>\n</functio<think>ameter>\n</function>',
    # A rejected DeepSeek attempt failing on invalid UTF-8 (`\xe2\x82_`):
    # its span covers the maximal ill-formed subpart it failed on (a
    # campaign failure).
    b'[[\x00RHi <\xef\xbd\x9ctool\xe2\x96\x81calls\xe2\x96\x81begin\xef\xbd\x9c><\xef\xbd\x9ctool\xe2\x96\x81call\xe2\x96\x81begin\xef\xbd\x9c>read_file<\xef\xbd\x9ctool\xe2\x96\x81sep\xef\xbd\x9c>{"path":"a"}<\xef\xbd\x9ctool\xe2\x96\x81call\xe2\x96\x81end\xef\xbd\x9c><\xef\xbd\x9ctool\xe2\x96\x81call\xe2\x96\x81begin\xef\xbd\x9c>read\xe2\x82_file oops<\xef\xbd\x9ctool\xe2\x96\x81sep\xef\xbd\x9c<|tool_call_begin|>ead_file<\xef\xbd\x9ctool\xe2\x96\x81sep\xef\xbd\x9c>{"path":"c"}<\xef\xbd\x9ct\xef\xbd\x9ctool\xe2\x96\x81call\xe2\x96\x81begin\xef\xbd\x9c>read_file oops<\xef\xbd\x9ctool\xe2\x96\x81sep\xef\xbd\x9c<|tool_call_begin|>ead_file<\xef\xbd\x9ctool\xe2\x96\x81sep\xef\xbd\x9c>{"path":"c"}<\xef\xbd\x9ctool\xe2\x96\x81call\xe2\x96\x81end\xef\xbd\x9c><\xef\xbd\x9ctool\xe2\x96\x81calls\xe2\x96\x81end\xef\xbd\x9c> done]]',
    # Under a length stop, a Kimi-K2 section's skipped last call the output
    # cut off runs to the end of output: an open call is truncation (a
    # campaign failure).
    b'\xe6#<|tool_calls_section_begin|><|tool_call_begin|>functions.rtions.read_file<|tool_call_argument_begin|>{"path": "a.rs"}<|tool_call_end|><|tool_call_begin|>lnd|><|tool_calls_secnd|>',
    # A rejected Mistral v11 call failing inside a JSON string: the rest of
    # the string is text, never prose, so its markup runs to the bracket
    # that balances its arguments (a campaign failure).
    b'\x00R[TOOL_CALLS]get_weather\t[CALL_ID]call_74616796\t[ARGS]{"city":"\\n  block\\n src/lib.rs","unit":"fahrenheit"}[TOOL_CALLS]\t\tlist_dir[CALL_ID]call_5c2a449c\t\t[ARGS]{"path":<|tool_call_end|>"line one\\nline two"}',
    # T6: a rejected Hermes call whose JSON strings hold `</tool_call>`
    # text: a JSON value is self-delimiting, so where it failed is where
    # its reading as written stopped, and its markup runs to its own closer
    # (a campaign failure).
    b'\x00R<tool_call>{"name":"write_file","arguments":{"path":"a","content":"x </tool_call> and </tool_cal> y":{"path":"a","content":"x </tool_call> and </tool_cal> y"t":""arguments":x </tool_call> and </tool_cal> y"}}</tool_call>',
    # A rejected Hermes attempt ending at a closer written with one slip
    # (`</tool_cal>`), an exact closer later in prose: the slipped closer is
    # the attempt's own (T4), so the later one stays content.
    b'\x00R<tool_call>{"name": oops</tool_cal> and then </tool_call>',
    # A rejected Mistral v11 call closing an array with `}`: the `}` balances
    # the object (brackets pair by kind), so the attempt runs to it (T3).
    b'\x00RHi [TOOL_CALLS]f[ARGS]{"a": [1, 2} done.',
    # The same in a DeepSeek V3.1 call, whose closers are no single bracket.
    b'\x00RHi <\xef\xbd\x9ctool\xe2\x96\x81calls\xe2\x96\x81begin\xef\xbd\x9c><\xef\xbd\x9ctool\xe2\x96\x81call\xe2\x96\x81begin\xef\xbd\x9c>f<\xef\xbd\x9ctool\xe2\x96\x81sep\xef\xbd\x9c>{"a": [1, 2} done.',
    # A stray `]` after a Mistral v11 call's arguments balances nothing (brackets
    # pair by kind): the rejected attempt never runs over it (T3).
    b'\x00RHi [TOOL_CALLS]f[ARGS]{"a": [1, 2]] done.',
    # The same in a DeepSeek V3.1 call.
    b'\x00RHi <\xef\xbd\x9ctool\xe2\x96\x81calls\xe2\x96\x81begin\xef\xbd\x9c><\xef\xbd\x9ctool\xe2\x96\x81call\xe2\x96\x81begin\xef\xbd\x9c>f<\xef\xbd\x9ctool\xe2\x96\x81sep\xef\xbd\x9c>{"a": [1, 2]] done.',
    # A `{` inside a JSON string of a rejected Mistral v11 call opens nothing:
    # the attempt runs to the `}` that balances its object, never past it.
    b'\x00RHi [TOOL_CALLS]f[ARGS]{"a": "{", 1 2} rest} more.',
    # A rejected Mistral v11 call failing inside a JSON string, then prose:
    # strings open and close from the attempt's start, so the prose is seen.
    b'\x00R[TOOL_CALLS]get_weather\t[CALL_ID]call_74616796\t[ARGS]{"city":"\\n  block\\n src/lib.rs","unit":"fahrenheit"}[TOOL_CALLS]\t\tlist_dir[CALL_ID]call_5c2a449c\t\t[ARGS]{"path":<|tool_call_end|>"line one\\nline two" oops. Sorry, wrong} done.',
    # A rejected Hermes attempt, prose, then a reasoning block: the attempt
    # never runs over the prose to the marker.
    b'\x00R<tool_call>{"name": oops} more prose here <think>t</think> done',
    # A rejected Hermes attempt whose exact closer follows junk: the closer is
    # its markup, never content.
    b'\x00R<tool_call>{"name": "read_file" oops, "a": 1} junk</tool_call> done',
    # Under a length stop, a rejected Hermes attempt that its closer closed
    # never runs on to the end of output (only an open call is truncated).
    b'\x04R<tool_call>{"name": oops}</tool_call> trailing words here',
    # T6: a Qwen value holding its closers then a whole call of a declared
    # tool: one call holding it and two calls apart are both clean.
    b'\x00R<tool_call>\n<function=write_file>\n<parameter=path>\na.md\n</parameter>\n<parameter=content>\nx\n</parameter>\n</function>\n</tool_call>\n<tool_call>\n<function=list_dir>\n<parameter=dir>\nsrc\n</parameter>\n</function>\n</tool_call>\n</parameter>\n</function>\n</tool_call>',
    # T6: a README documenting a declared read-only tool three times inside a
    # write_file value: one call, never three.
    b'\x00R<tool_call>\n<function=write_file>\n<parameter=path>\nREADME.md\n</parameter>\n<parameter=content>\nTo list a directory:\n<function=list_dir>\n<parameter=dir>\nsrc\n</parameter>\n</function>\nor the tests:\n<function=list_dir>\n<parameter=dir>\ntests\n</parameter>\n</function>\nor the docs:\n<function=list_dir>\n<parameter=dir>\ndocs\n</parameter>\n</function>\nThat is all.\n</parameter>\n</function>\n</tool_call>',
    # A rejected Llama 3 attempt followed by `_dir[ARGS]{...}`: a word may
    # begin with `_`, so `_dir` followed by `[` is prose and the attempt never
    # runs over it to the bracket (a campaign failure).
    b'\xe6#[TOOL<|python_tag|>_dir[ARGS]{"path": "."}\nDone.',
    # F16: a cut special token, then a Llama 3 call body left open before
    # prose: the rejection covers the body to its last JSON byte.
    b'\x00RSure. <|python_tag|>t_begin|>"name": "get_weather", "arguments": {"city": "Oslo"<think>wait</think> Done.',
    # F16: the same body cut by the end of output.
    b'\x00RSure. <|python_tag|>t_begin|>"name": "get_weather", "arguments": {"city": "Oslo"',
    # F16: an attempt rejected at a misspelled opener, then a call body cut
    # by a reasoning marker: the body is the attempt's markup, never
    # content (a campaign failure).
    b'\x00R<tool_cal>{"name": "f", "arguments": {"a":{"name": "f", "arguments": {"[THINK]a": "x"}}</tool_call>',
    # F16: an OpenAI call whose arguments string runs into the next call's
    # `"function"` key: the attempt completes that key, and the object after
    # its `:` is the call body the key began, never content (a campaign
    # failure; the golden openai_rejection_covers_call_body_after_completed_key).
    b'\x05\x91[\n  {\n    "id": "call_a1",\n    "type": "function",\n    "function": {\n      "name": "write_file",\n      "arguments": "{\\"path\\": \\"src/main.rs\\", \\"content\\": \\"fn main() {\\\\n    println!(\\\\\\"hi\\\\\\");\\\\n\\\\n\\\\tlet  x = 1;  \\\\n}\\\\n\\", \\"mode\\": \\"create\\", \\"ratio\\": 2.50[TOOL_CALLS], \\"count\\": 1e3, \\"tags\\": [], \\"options\\": {\\"backup\\": false, \\"retries\\": 0, \\"owner\\": {\\"name\\": \\"ops\\"  "function": {\n      "name": "list_dir",\n      "arguments":"write_file",\n      "arguments": "{\\"path\\": \\"src/main.rs\\", \\"content\\": \\"fn main() {\\\\n    println!(\\\\\\"hi\\\\\\");\\\\n\\\\n\\\\tlet  x = 1;  \\\\n}\\\\n\\", \\"mode\\": \\"create\\", \\"ratio\\": 2.50[TOOL_CALLS], \\"count\\": 1e3, \\"tags\\": [], \\"options\\": {\\"backup\\": false, \\"retries\\": 0, \\"owner\\": {\\"name\\": \\"ops\\"  "function": {\n      "name": "list_dir",\n      "arguments": "{\\"path\\": \\"src\\"}"\n    }\n  }\n]',
    # F16: a Hermes attempt failing at a stray `>`, then a call body: the
    # debris is no prose, so the body is the attempt's markup, never content
    # (a campaign failure; the golden
    # hermes_rejection_covers_call_body_after_stray_byte).
    b'[\x05\x91<tool_call>{"name": "launch_rockets", >{"name": "launch_rockets", "<|return|>]',
    # F16: a bracketless Mistral v3 list whose first object fails, then a
    # call body after that object's closing bracket: the body is the
    # attempt's markup, never content (a campaign failure; the golden
    # mistral3_rejection_covers_call_body_after_its_bracket).
    b'\x00R[TOOL_CALLS] {"name": <|end|>, "arguments": {"path": "src"}}, {"name": "read_file", "ar\x00gu',
    # F16: a Llama 3 object failing at a name value that lost its opening
    # quote, then a call member the flipped quotes read as text: a member's
    # spelling begins a call body wherever it is written (a campaign
    # failure; the golden llama3_rejection_covers_call_member_after_broken_string).
    b'[\xe6#Let me check both.\n{"name": "get_weather", "parameters": {"city": "Oslo"}}; {"name": "read_file", "parameters": {"path": "notes.txt", "limit": 10}}\n{"parameters": {"city": "Bergen", "unit": "celsius"}, "name": rgen", "unit": "celsius"}, "name": "get_weather"}ius"}, "name": "get_weather"}]',
]

# Strict forced tool choice: header `\xe6\x23` (forced `read_file`) plus a
# trailer byte in 0xF8..=0xFF, a seed byte after the turn that the probe
# consumes instead of reading it as part of the turn (bit 0 exactly one,
# bit 1 no content, bit 2 no reasoning; see `fuzz_tool_call.rs`). Earlier
# seeds end in other bytes, so they keep their meaning.
_STRICT_CALL = b'<tool_call>\n{"name": "read_file", "arguments": {"path": "a"}}\n</tool_call>'
TOOL_CALL_STRICT = [
    b'\xe6\x23' + _STRICT_CALL + b'\n' + _STRICT_CALL + b'\xf9',  # two calls, exactly one
    b'\xe6\x23Sure.\n' + _STRICT_CALL + b'\xfa',  # prose, no content
    b'\xe6\x23' + _STRICT_CALL + b'\n' + b'\xf8',  # lenient degenerate form
    b'\xe6\x23<think>plan</think>\n' + _STRICT_CALL + b'\xfc',  # reasoning, no reasoning
    b'\xe6\x23\n' + _STRICT_CALL + b'\n\xc2\xa0' + b'\xff',  # U+00A0 is content, all options
]

TOOL_CALL_SEEDS = [
    _tool_call_seed(index, turn)
    for index, turn in enumerate(
        _tool_call_conformance_markup() + TOOL_CALL_WITNESSES + TOOL_CALL_REPAIRS + TOOL_CALL_NEGATIVES)
] + TOOL_CALL_REGRESSIONS + TOOL_CALL_STRICT
TOOL_CALL_TOKENS = [
    b'<tool_call>', b'</tool_call>', b'<function=', b'</function>', b'<parameter=',
    b'</parameter>', b'</parameters>', b'<think>', b'</think>', b'{"name":',
    b'"arguments":', b'"tool_calls"', b'"function"', b'"id"', b'"type"', b'```',
    b'```json\n', b'"parameters":', b'<|python_tag|>', b'<|eom_id|>', b'<|eot_id|>',
    b'; ', b'[TOOL_CALLS]', b'[TOOL_RESULTS]', b'[ARGS]', b'[CALL_ID]', b'[TOOL_CONTENT]',
    b'[THINK]', b'[/THINK]', "<｜tool▁calls▁begin｜>".encode(), "<｜tool▁call▁begin｜>".encode(),
    "<｜tool▁sep｜>".encode(), "<｜tool▁call▁end｜>".encode(), "<｜tool▁calls▁end｜>".encode(),
    b'function', b'```javascript', "<｜tool▁outputs▁begin｜>".encode(),
    b'<|tool_calls_section_begin|>', b'<|tool_call_begin|>', b'<|tool_call_argument_begin|>',
    b'<|tool_call_end|>', b'<|tool_calls_section_end|>', b'functions.', b':0',
    b'{"type":"tool_use",', b'"input":', b'"toolu_01"', b'"type":"thinking"', b'"content":[',
    b'<|start|>', b'<|channel|>', b'<|message|>', b'<|call|>', b'<|end|>', b'<|return|>',
    b'<|constrain|>', b' to=functions.', b'analysis', b'commentary',
    b'<|tool_calls_begin|>', b'"id":', b'</s>', b'read_file', b'write_file', b'current_time', b'path', b'\n', b'\t',
    b'"', b'\\', b'{', b'}', b'[', b']', b':', b',', b'>', b'<', b'null', b'1.50',
    b'\x00', b'\x04', b'\x80', 'é'.encode(), b'\xff', b'\xe2\x82',
]

TARGETS = {
    'json': {'seeds': JSON_SEEDS, 'tokens': JSON_TOKENS, 'differential': None},
    'markdown': {'seeds': MARKDOWN_SEEDS, 'tokens': MARKDOWN_TOKENS, 'differential': 'markdown'},
    'schema': {'seeds': SCHEMA_SEEDS, 'tokens': SCHEMA_TOKENS, 'differential': None},
    'pattern': {'seeds': PATTERN_SEEDS, 'tokens': PATTERN_TOKENS, 'differential': 'pattern'},
    'unified': {'seeds': UNIFIED_SEEDS, 'tokens': UNIFIED_TOKENS, 'differential': None},
    'fuzzy': {'seeds': FUZZY_SEEDS, 'tokens': FUZZY_TOKENS, 'differential': None},
    'matching': {'seeds': MATCHING_SEEDS, 'tokens': MATCHING_TOKENS, 'differential': None},
    'patch': {'seeds': PATCH_SEEDS, 'tokens': PATCH_TOKENS, 'differential': None},
    'tool-call': {'seeds': TOOL_CALL_SEEDS, 'tokens': TOOL_CALL_TOKENS, 'differential': None,
                  'input': 'bytes'},
}


def harness_source(target):
    """The probe source of `target` (`fuzz-<target>` builds `fuzz_<target>.rs`,
    with `-` spelled `_`)."""
    return ROOT / f'crates/core/examples/fuzz_{target.replace("-", "_")}.rs'


def admits_bytes(target):
    """Whether `target`'s probe takes raw bytes (always completing with `ok`)
    instead of rejecting invalid UTF-8 with `invalid-utf8`."""
    return TARGETS[target].get('input', 'utf8') == 'bytes'


class Random:
    """Fixed LCG algorithm; reproducibility does not depend on Python random."""

    def __init__(self, seed):
        self.state = seed

    def index(self, length):
        self.state = (self.state * 6364136223846793005 + 1) & ((1 << 64) - 1)
        return (self.state >> 32) % length


def mutate(data, random, tokens):
    for _ in range(1 + random.index(3)):
        start = random.index(len(data) + 1)
        end = start + random.index(len(data) - start + 1)
        token = tokens[random.index(len(tokens))]
        operation = random.index(5)
        if operation == 0:
            data = data[:start] + token + data[start:]
        elif operation == 1:
            data = data[:start] + data[end:]
        elif operation == 2:
            data = data[:start] + token + data[end:]
        elif operation == 3:
            data = b'[' + data + b']'
        else:
            data = data[:end] + data[start:end] + data[end:]
    return data[:4096]


def covered_regions(document):
    if document.get('type') != 'llvm.coverage.json.export' or document.get('version') != '3.1.0':
        raise ValueError('unsupported LLVM coverage JSON format')
    regions = set()
    core_functions = 0
    core_root = ROOT / 'crates/core/src'
    classified_filenames = {}
    for unit in document['data']:
        for function in unit['functions']:
            filenames = function['filenames']
            core_files = {}
            for file_id, name in enumerate(filenames):
                if name not in classified_filenames:
                    filename = Path(name)
                    classified_filenames[name] = (
                        str(filename.relative_to(ROOT)) if filename.is_relative_to(core_root) else None)
                relative = classified_filenames[name]
                if relative is not None:
                    core_files[file_id] = relative
            core_functions += bool(core_files)
            for region in function['regions']:
                if len(region) != 8 or any(type(value) is not int or value < 0 for value in region):
                    raise ValueError('malformed coverage region')
                line, column, end_line, end_column, count, file_id, _, kind = region
                if file_id >= len(filenames):
                    raise ValueError('coverage file index out of bounds')
                if count and kind == 0 and file_id in core_files:
                    # Coalesce generic instantiations by source coordinates. Count only code regions.
                    regions.add((core_files[file_id], line, column, end_line, end_column))
    if not core_functions:
        raise ValueError('coverage export contains no core functions')
    return regions


# --------------------------------------------------------------------------
# Differential oracles: Node's `RegExp` for `pattern`, `cmark-gfm` for
# `markdown`. Each oracle is invoked out-of-process, never linked.
# --------------------------------------------------------------------------

_NODE_PATTERN_SCRIPT = (
    "const p = process.argv[1], s = process.argv[2];"
    "let re;"
    "try { re = new RegExp(p, 'u'); }"
    "catch (e) { process.stdout.write('unsupported'); process.exit(0); }"
    "process.stdout.write(String(re.test(s)));"
)


def node_pattern_test(pattern, subject, timeout, counts=None):
    """`None` means Node itself refuses the construct under `u` mode (the
    restricted subset both implement) or could not decide within the bounded
    timeout; neither is a mismatch. V8's own backtracking regex engine is
    exactly as vulnerable to catastrophic backtracking as any backtracking
    engine (this project's derivative engine is specifically designed to
    avoid that on the same inputs, e.g. the documented `(a+)+$` seed against
    a long run of `a`s), so a reference-tool timeout on an adversarial
    pattern says nothing about which engine, if either, is wrong."""
    try:
        result = subprocess.run(
            ['node', '-e', _NODE_PATTERN_SCRIPT, '--', pattern, subject],
            capture_output=True, text=True, timeout=timeout,
        )
    except subprocess.TimeoutExpired:
        if counts is not None:
            counts['reference_timeouts'] += 1
        return None
    result.check_returncode()
    if result.stderr:
        raise ValueError(f'node pattern oracle reported a diagnostic: {result.stderr!r}')
    output = result.stdout.strip()
    if output == 'unsupported':
        if counts is not None:
            counts['reference_rejections'] += 1
        return None
    if output not in ('true', 'false'):
        raise ValueError(f'node pattern oracle returned an unexpected value: {output!r}')
    return output == 'true'


class DifferentialMismatch(ValueError):
    """A completed independent oracle disagrees with the implementation."""


def check_pattern_differential(record, timeout, counts=None):
    if record.get('kind') != 'pattern':
        raise ValueError(f'unexpected differential record for target pattern: {record!r}')
    expected = node_pattern_test(record['pattern'], record['subject'], timeout, counts)
    if expected is None:
        return False
    if expected != record['expected']:
        raise DifferentialMismatch(
            'pattern differential mismatch: pattern='
            f'{record["pattern"]!r} subject={record["subject"]!r} '
            f'purrassist_core={record["expected"]} node={expected}'
        )
    return True


CMARK_GFM_VERSION = '0.29.0.gfm.13'
CMARK_GFM_SOURCE_SHA256 = '5abc61798ebd9de5660bc076443c07abad2b8d15dbc11094a3a79644b8ad243a'
CMARK_GFM_URL = f'https://codeload.github.com/github/cmark-gfm/tar.gz/refs/tags/{CMARK_GFM_VERSION}'
CMARK_GFM_EXTENSIONS = ['table', 'strikethrough', 'autolink', 'tasklist', 'tagfilter']
CMARK_BACKTICK_BEFORE = '      subj->backticks[numticks] = subj->pos - numticks;'
CMARK_BACKTICK_AFTER = '''      if (subj->backticks[numticks] < subj->pos - numticks) {
        subj->backticks[numticks] = subj->pos - numticks;
      }'''
CMARK_BACKTICK_PATCH_SHA256 = hashlib.sha256(
    (CMARK_BACKTICK_BEFORE + '\n' + CMARK_BACKTICK_AFTER).encode()).hexdigest()


CMARK_EMPHASIS_PATCHES = [['  bufsize_t openers_bottom[3][128];', '  bufsize_t openers_bottom[2][3][128];'],
 ['  // initialize openers_bottom:\n'
  '  memset(&openers_bottom, 0, sizeof(openers_bottom));\n'
  '  for (i=0; i < 3; i++) {\n'
  "    openers_bottom[i]['*'] = stack_bottom;\n"
  "    openers_bottom[i]['_'] = stack_bottom;\n"
  "    openers_bottom[i]['\\''] = stack_bottom;\n"
  '    openers_bottom[i][\'"\'] = stack_bottom;\n'
  '  }',
  '  // Separate close-only and dual-purpose emphasis runs.\n'
  '  memset(&openers_bottom, 0, sizeof(openers_bottom));\n'
  '  for (i=0; i < 3; i++) {\n'
  "    openers_bottom[0][i]['*'] = stack_bottom;\n"
  "    openers_bottom[1][i]['*'] = stack_bottom;\n"
  "    openers_bottom[0][i]['_'] = stack_bottom;\n"
  "    openers_bottom[1][i]['_'] = stack_bottom;\n"
  "    openers_bottom[0][i]['\\''] = stack_bottom;\n"
  '    openers_bottom[0][i][\'"\'] = stack_bottom;\n'
  '  }'],
 ['    if (closer->can_close) {\n      // Now look backwards for first matching opener:',
  '    if (closer->can_close) {\n'
  "      int can_open_floor = (closer->delim_char == '*' || closer->delim_char == '_') && "
  'closer->can_open;\n'
  '      // Now look backwards for first matching opener:'],
 ['openers_bottom[closer->length % 3][closer->delim_char]',
  'openers_bottom[can_open_floor][closer->length % 3][closer->delim_char]'],
 ['openers_bottom[old_closer->length % 3][old_closer->delim_char]',
  'openers_bottom[can_open_floor][old_closer->length % 3][old_closer->delim_char]']]
CMARK_EMPHASIS_PATCH_SHA256 = hashlib.sha256(
    json.dumps(CMARK_EMPHASIS_PATCHES, separators=(',', ':'), ensure_ascii=True).encode()).hexdigest()


CMARK_DESTINATION_BEFORE = "static bufsize_t manual_scan_link_url_2(cmark_chunk *input, bufsize_t offset,\n                                        cmark_chunk *output) {\n  bufsize_t i = offset;\n  size_t nb_p = 0;\n\n  while (i < input->len) {\n    if (input->data[i] == '\\\\' &&\n        i + 1 < input-> len &&\n        cmark_ispunct(input->data[i+1]))\n      i += 2;\n    else if (input->data[i] == '(') {\n      ++nb_p;\n      ++i;\n      if (nb_p > 32)\n        return -1;\n    } else if (input->data[i] == ')') {\n      if (nb_p == 0)\n        break;\n      --nb_p;\n      ++i;\n    } else if (cmark_isspace(input->data[i])) {\n      if (i == offset) {\n        return -1;\n      }\n      break;\n    } else {\n      ++i;\n    }\n  }\n\n  if (i >= input->len)\n    return -1;\n\n  {\n    cmark_chunk result = {input->data + offset, i - offset, 0};\n    *output = result;\n  }\n  return i - offset;\n}\n"
CMARK_DESTINATION_AFTER = "static bufsize_t manual_scan_link_url_2(cmark_chunk *input, bufsize_t offset,\n                                        cmark_chunk *output) {\n  bufsize_t i = offset;\n  size_t nb_p = 0;\n\n  while (i < input->len) {\n    if (input->data[i] == '\\\\' &&\n        i + 1 < input-> len &&\n        cmark_ispunct(input->data[i+1]))\n      i += 2;\n    else if (input->data[i] == '(') {\n      ++nb_p;\n      ++i;\n      if (nb_p > 32)\n        return -1;\n    } else if (input->data[i] == ')') {\n      if (nb_p == 0)\n        break;\n      --nb_p;\n      ++i;\n    } else if (cmark_isspace(input->data[i])) {\n      if (i == offset) {\n        return -1;\n      }\n      break;\n    } else {\n      ++i;\n    }\n  }\n\n  if (i >= input->len || nb_p != 0)\n    return -1;\n\n  {\n    cmark_chunk result = {input->data + offset, i - offset, 0};\n    *output = result;\n  }\n  return i - offset;\n}\n"
CMARK_DESTINATION_PATCH_SHA256 = hashlib.sha256(
    json.dumps([CMARK_DESTINATION_BEFORE, CMARK_DESTINATION_AFTER], separators=(',', ':'), ensure_ascii=True).encode()).hexdigest()


def patch_reference_destination_balance(source):
    """Reject unmatched bare-destination parentheses at the exact scanner site."""
    if source.count(CMARK_DESTINATION_BEFORE) != 1:
        raise ValueError('cmark-gfm destination patch requires its exact unique function preimage')
    return source.replace(CMARK_DESTINATION_BEFORE, CMARK_DESTINATION_AFTER)


def destination_reference_profile(source):
    """Select potential definition destinations independently of either renderer.

    Existing literal-prefix certificates retain their separate guarded oracle.
    Other raw definition spellings with an opening parenthesis select the
    corrected bare-destination scanner, including container/multiline forms.
    Balanced, escaped, angle and unused spellings are still executed in full.
    """
    return '(' in source and ']:' in source and reference_admission_profile(source) == 'gfm'


CMARK_EMAIL_BEFORE = 'found_at:\n    for (rewind = 0; rewind < max_rewind; ++rewind) {'
CMARK_EMAIL_AFTER = 'found_at:\n    np = 0;\n    for (rewind = 0; rewind < max_rewind; ++rewind) {'
CMARK_EMAIL_PATCH_SHA256 = hashlib.sha256(
    json.dumps([CMARK_EMAIL_BEFORE, CMARK_EMAIL_AFTER], separators=(',', ':'), ensure_ascii=True).encode()).hexdigest()


def patch_reference_email_counter(source):
    """Reset domain periods for each retried candidate at the unique scanner label."""
    if source.count(CMARK_EMAIL_BEFORE) != 1:
        raise ValueError('cmark-gfm email patch requires its exact unique source preimage')
    return source.replace(CMARK_EMAIL_BEFORE, CMARK_EMAIL_AFTER)


def email_reference_profile(source):
    """Select literal or potentially entity-decoded at-signs before execution."""
    return '@' in source or '&' in source


def patch_reference_emphasis_floors(source):
    """Keep close-only and dual-purpose opener floors separate; fail closed."""
    for before, after in CMARK_EMPHASIS_PATCHES:
        if source.count(before) != 1:
            raise ValueError('cmark-gfm emphasis patch requires each exact unique source preimage')
        source = source.replace(before, after)
    return source


def patch_reference_backtick_cache(source):
    """Preserve the furthest known closer; reject an unexpected patch site."""
    lines = source.split('\n')
    if lines.count(CMARK_BACKTICK_BEFORE) != 1:
        raise ValueError('cmark-gfm backtick patch requires its exact unique source preimage')
    return '\n'.join(CMARK_BACKTICK_AFTER if line == CMARK_BACKTICK_BEFORE else line for line in lines)


def ensure_cmark_gfm(*, corrected_backticks=False, corrected_emphasis=False, corrected_destinations=False, corrected_emails=False):
    """Build the pinned `cmark-gfm` CLI from a digest-verified source tarball
    into a persistent `.cache/tools/` location, reused across runs."""
    corrected = corrected_backticks or corrected_emphasis or corrected_destinations or corrected_emails
    if corrected:
        inventory = json.loads((ROOT / 'vendor/tooling.json').read_text())
        entry = next(item for item in inventory if item['name'] == 'cmark-gfm')
        if (entry.get('source_sha256') != CMARK_GFM_SOURCE_SHA256
                or (corrected_backticks and entry.get('backtick_cache_patch_sha256') != CMARK_BACKTICK_PATCH_SHA256)
                or (corrected_emphasis and entry.get('emphasis_floors_patch_sha256') != CMARK_EMPHASIS_PATCH_SHA256)
                or (corrected_destinations and entry.get('destination_balance_patch_sha256') != CMARK_DESTINATION_PATCH_SHA256)
                or (corrected_emails and entry.get('email_counter_patch_sha256') != CMARK_EMAIL_PATCH_SHA256)):
            raise ValueError('cmark-gfm correction differs from its registered digest')
    suffix = ('-backtick-' + CMARK_BACKTICK_PATCH_SHA256[:16] if corrected_backticks else '')
    suffix += '-emphasis-' + CMARK_EMPHASIS_PATCH_SHA256[:16] if corrected_emphasis else ''
    suffix += '-destination-' + CMARK_DESTINATION_PATCH_SHA256[:16] if corrected_destinations else ''
    suffix += '-email-' + CMARK_EMAIL_PATCH_SHA256[:16] if corrected_emails else ''
    tools_root = ROOT / '.cache' / 'tools' / f'cmark-gfm-{CMARK_GFM_VERSION}{suffix}'
    binary = tools_root / 'bin' / 'cmark-gfm'
    identity_path = tools_root / 'backtick-build.json'
    patch_identity = {'patch_sha256': CMARK_BACKTICK_PATCH_SHA256} if corrected_backticks else {}
    if corrected_emphasis:
        patch_identity['emphasis_patch_sha256'] = CMARK_EMPHASIS_PATCH_SHA256
    if corrected_destinations:
        patch_identity['destination_patch_sha256'] = CMARK_DESTINATION_PATCH_SHA256
    if corrected_emails:
        patch_identity['email_patch_sha256'] = CMARK_EMAIL_PATCH_SHA256
    if binary.is_file():
        version = subprocess.run([str(binary), '--version'], capture_output=True, text=True, timeout=10).stdout
        if CMARK_GFM_VERSION in version:
            if not corrected:
                return binary
            inlines = tools_root / 'src' / 'src' / 'inlines.c'
            autolink = tools_root / 'src' / 'extensions' / 'autolink.c'
            autolink_identity = {'corrected_autolink_sha256': hashlib.sha256(autolink.read_bytes()).hexdigest()} if corrected_emails and autolink.is_file() else {}
            if identity_path.is_file() and inlines.is_file() and (not corrected_emails or autolink.is_file()) and json.loads(identity_path.read_text()) == {
                    'source_sha256': CMARK_GFM_SOURCE_SHA256,
                    **patch_identity,
                    **autolink_identity,
                    'corrected_inlines_sha256': hashlib.sha256(inlines.read_bytes()).hexdigest(),
                    'binary_sha256': hashlib.sha256(binary.read_bytes()).hexdigest()}:
                return binary
    tools_root.mkdir(parents=True, exist_ok=True)
    # CMARK_GFM_URL is a fixed, pinned constant; its digest is verified below.
    existing_archive = ROOT / '.cache' / 'tools' / f'cmark-gfm-{CMARK_GFM_VERSION}' / 'source.tar.gz'
    if corrected and existing_archive.is_file():
        data = existing_archive.read_bytes()
    else:
        with urllib.request.urlopen(CMARK_GFM_URL, timeout=60) as response:
            data = response.read()
    digest = hashlib.sha256(data).hexdigest()
    if digest != CMARK_GFM_SOURCE_SHA256:
        raise ValueError(f'cmark-gfm source digest mismatch: got {digest}, expected {CMARK_GFM_SOURCE_SHA256}')
    archive = tools_root / 'source.tar.gz'
    archive.write_bytes(data)
    source = tools_root / 'src'
    if source.exists():
        shutil.rmtree(source)
    source.mkdir()
    with tarfile.open(archive) as tar:
        members = tar.getmembers()
        for member in members:
            parts = member.name.split('/', 1)
            member.name = parts[1] if len(parts) == 2 else ''
        tar.extractall(source, members=[member for member in members if member.name], filter='data')
    if corrected:
        inlines = source / 'src' / 'inlines.c'
        patched = patch_reference_backtick_cache(inlines.read_text()) if corrected_backticks else inlines.read_text()
        if corrected_emphasis:
            patched = patch_reference_emphasis_floors(patched)
        if corrected_destinations:
            patched = patch_reference_destination_balance(patched)
        inlines.write_text(patched)
        if corrected_emails:
            autolink = source / 'extensions' / 'autolink.c'
            autolink.write_text(patch_reference_email_counter(autolink.read_text()))
    build_dir = tools_root / 'build'
    configure = subprocess.run(
        ['cmake', '-S', str(source), '-B', str(build_dir), '-DCMAKE_BUILD_TYPE=Release',
         '-DCMARK_TESTS=OFF', '-DCMARK_SHARED=OFF', '-DCMARK_STATIC=ON',
         '-DCMAKE_POLICY_VERSION_MINIMUM=3.5'],
        capture_output=True, text=True, timeout=120,
    )
    if configure.returncode != 0:
        raise ValueError(f'cmark-gfm configure failed:\n{configure.stdout}\n{configure.stderr}')
    build = subprocess.run(
        ['cmake', '--build', str(build_dir), '--target', 'cmark-gfm', '-j', str(os.cpu_count() or 1)],
        capture_output=True, text=True, timeout=300,
    )
    if build.returncode != 0:
        raise ValueError(f'cmark-gfm build failed:\n{build.stdout}\n{build.stderr}')
    built = build_dir / 'src' / 'cmark-gfm'
    if not built.is_file():
        raise ValueError('cmark-gfm build did not produce the expected executable')
    (tools_root / 'bin').mkdir(exist_ok=True)
    shutil.copy2(built, binary)
    binary.chmod(0o755)
    if corrected:
        identity_path.write_text(json.dumps({'source_sha256': CMARK_GFM_SOURCE_SHA256,
            **patch_identity,
            **({'corrected_autolink_sha256': hashlib.sha256(autolink.read_bytes()).hexdigest()} if corrected_emails else {}),
            'corrected_inlines_sha256': hashlib.sha256(inlines.read_bytes()).hexdigest(),
            'binary_sha256': hashlib.sha256(binary.read_bytes()).hexdigest()}, sort_keys=True) + '\n')
    return binary


# Raw-input profile selection is independent of PurRAssist parsing and never
# runs as a fallback after a mismatch. cmark-gfm cannot recognize task markers
# when an outer container and their item open on the same physical line,
# and incorrectly treats a later literal [x]/[X] as checking an initial [ ].
_LIST_PREFIX = re.compile(r'(?:[-+*]|[0-9]{1,9}[.)])[ \t]+')
_TASK_MARKER = re.compile(r'\[([ \txX\v\f])\](?=[ \t\v\f]|$)')


def tasklist_reference_limit_reasons(source):
    # Scan once through each prefix. A nested quantified regex would admit
    # exponential whitespace partitions on long sequences of quote markers.
    reasons = set()
    reference_prefix = False
    task_prefix = False
    physical_lines = re.split(r'\r\n|[\r\n]', source)
    for line_number, line in enumerate(physical_lines):
        cursor = 0
        containers = 0
        last_list = False
        while cursor < len(line):
            leading = 0
            while cursor < len(line) and line[cursor] == ' ' and leading < 3:
                cursor += 1
                leading += 1
            reference_prefix |= _REFERENCE_LABEL.match(line, cursor) is not None
            task_prefix |= _TASK_MARKER.match(line, cursor) is not None
            marker = _TASK_MARKER.match(line, cursor) if last_list else None
            if marker is not None:
                if containers >= 2:
                    reasons.add("task marker shares physical line with an outer container")
                if marker[1] not in 'xX' and re.search(r"\[[xX]\]", line[marker.end():]):
                    reasons.add("unchecked task contains a later literal checked marker")
                if marker[1] not in ' xX':
                    reasons.add("task marker uses non-space ASCII whitespace inside brackets")
                tail = line[marker.end():]
                if tail and not tail.strip(' \t\v\f'):
                    reasons.add("task marker occupies an otherwise empty first physical line")
                    if not tail.strip(' \t'):
                        reasons.add("empty task separator consists only of spaces or tabs")
                if not tail and line_number + 1 < len(physical_lines):
                    reasons.add("task separator can be the paragraph continuation newline")
            if last_list and line[cursor:] == '[' and line_number + 1 < len(physical_lines):
                if re.match(r'[ \t>]*\](?=[ \t\v\f]|$)', physical_lines[line_number + 1]):
                    reasons.add("task marker's single whitespace character is a logical newline")
            if cursor < len(line) and line[cursor] == '>':
                cursor += 1
                if cursor < len(line) and line[cursor] in ' \t':
                    cursor += 1
                containers += 1
                last_list = False
            else:
                item = _LIST_PREFIX.match(line, cursor)
                if item is None:
                    break
                cursor = item.end()
                containers += 1
                last_list = True
    if task_prefix and any(not line.strip(' \t') for line in physical_lines[:-1]):
        reasons.add('physical blank line can make a task list loose')
    if reference_prefix and task_prefix:
        reasons.add('task marker can follow a reference definition in its first paragraph')
    return sorted(reasons)


def tasklist_reference_limited(source):
    return bool(tasklist_reference_limit_reasons(source))


def generated_hardbreak_reference_profile(source):
    """Certify that BR markup cannot be raw HTML or entity-produced text.

    A possible physical hardbreak selects this profile before execution; only
    actual generated BR elements in the reference output are projected.
    """
    return '<' not in source and '&' not in source and bool(
        re.search(r'(?:\\| {2})(?:\r\n|\r|\n)', source))


def project_reference_hardbreaks(source, html):
    if not generated_hardbreak_reference_profile(source):
        raise ValueError('hardbreak projection requires its raw-input certificate')
    return re.sub(r'(<br>\n)[ \t]+', r'\1', normalize_html(html))


def generated_paragraph_prefix_reference_profile(source):
    """Certify generated paragraph/list markup without semantic entities."""
    return '<' not in source and '&' not in source


def project_reference_paragraph_prefix(source, html):
    if not generated_paragraph_prefix_reference_profile(source):
        raise ValueError('paragraph-prefix projection requires its raw-input certificate')
    return re.sub(r'(<(?:p|li)>)[ \t]+', r'\1', normalize_html(html))


_REFERENCE_LABEL = re.compile(r'\[((?:[^\[\]\\\x00-\x08\x0e-\x1f\x7f]|\\[!"#$%&\'()*+,./:;<=>?@\[\]\\^_`{|}~\-]|\\(?![!"#$%&\'()*+,./:;<=>?@\[\]\\^_`{|}~\-])){1,999})\]:[ \t]*([^ \t\v\f\r\n]+)([^\r\n]*)')
# A definition may place its bare destination after one physical line break.
# This extension is exclusive to invalid unbalanced destinations; title
# admission continues to use the original same-line matcher.
_UNBALANCED_REFERENCE_LABEL = re.compile(_REFERENCE_LABEL.pattern.replace(
    r'\]:[ \t]*', r'\]:[ \t]*(?:\n[ \t]*)?', 1))

_REFERENCE_GUARD = 'PurrassistLiteralReferenceGuard1848099'
_GUARDED_BLOCK = re.compile(r'\A(?:<p>|<h[12]>|<table>\n<thead>\n<tr>\n<th(?: align="(?:left|right|center)")?>)' + _REFERENCE_GUARD)


def unbalanced_reference_paragraph(source):
    """Select an invalid definition-shaped prefix independently.

    Only a nonempty label without unescaped brackets/non-whitespace control bytes and a bare
    destination with an unmatched opening
    parenthesis qualify. One physical newline may separate the colon from
    that destination; blank lines never qualify. It must remain ordinary block content, including any
    continuation lines. Balanced/escaped parentheses and angle destinations
    do not qualify. The primary grammar forbids this reference definition.
    """
    normalized = source.replace('\r\n', '\n').replace('\r', '\n')
    match = _UNBALANCED_REFERENCE_LABEL.match(normalized)
    if match is None or not match[1].strip() or _REFERENCE_GUARD in source:
        return False
    if re.search(r'\n[ \t]*\n', match[1]):
        return False
    destination = match[2]
    if destination.startswith('<') or any(ord(char) < 32 or ord(char) == 127 for char in destination):
        return False
    depth = 0
    cursor = 0
    while cursor < len(destination):
        char = destination[cursor]
        if char == '\\' and cursor + 1 < len(destination) and destination[cursor + 1] in r'''!"#$%&'()*+,-./:;<=>?@[\]^_`{|}~''':
            cursor += 2
            continue
        if char == '(':
            depth += 1
        elif char == ')':
            if depth == 0:
                return False
            depth -= 1
        cursor += 1
    return depth > 0


def escaped_terminal_reference_title(source):
    """Select an isolated invalid title with no unescaped closing delimiter."""
    normalized = source.replace('\r\n', '\n').replace('\r', '\n')
    line, _, rest = normalized.partition('\n')
    if rest.strip(' \t\n') or _REFERENCE_GUARD in source:
        return False
    match = _REFERENCE_LABEL.fullmatch(line)
    if match is None or not match[1].strip():
        return False
    title = match[3].strip(' \t')
    if len(title) < 3 or title[0] not in '"\'(':
        return False
    closer = ')' if title[0] == '(' else title[0]
    if title[-1] != closer:
        return False
    cursor = 1
    escaped_closer = False
    while cursor < len(title):
        char = title[cursor]
        if char == '\\' and cursor + 1 < len(title) and title[cursor + 1] in r"""!"#$%&'()*+,-./:;<=>?@[\]^_`{|}~""":
            escaped_closer = cursor + 2 == len(title) and title[cursor + 1] == closer
            cursor += 2
            continue
        if char == closer or (title[0] == '(' and char == '('):
            return False
        cursor += 1
    return escaped_closer


def reference_admission_profile(source):
    if unbalanced_reference_paragraph(source):
        return 'literal-unbalanced-destination'
    if escaped_terminal_reference_title(source):
        return 'literal-escaped-title-closer'
    return 'gfm'


def cmark_gfm_render(binary, source, timeout, *, tasklists=True):
    args = [str(binary)]
    for extension in CMARK_GFM_EXTENSIONS:
        if extension == 'tasklist' and not tasklists:
            continue
        args += ['-e', extension]
    args.extend(['--unsafe', '--strikethrough-double-tilde'])
    result = subprocess.run(args, input=source.encode('utf-8'), capture_output=True, timeout=timeout)
    result.check_returncode()
    if result.stderr:
        raise ValueError(f'cmark-gfm reported a diagnostic: {result.stderr!r}')
    return result.stdout.decode('utf-8')


def verify_reference_backtick_correction(original, corrected):
    """Execute primary-grammar controls against both independent binaries."""
    for source, expected, raw_expected in [
        ('[`a ``b`` c ``d``', '<p>[`a <code>b</code> c <code>d</code></p>\n',
         '<p>[`a <code>b</code> c ``d``</p>\n'),
        ('[```a ``b`` c ``d``', '<p>[```a <code>b</code> c <code>d</code></p>\n',
         '<p>[```a <code>b</code> c ``d``</p>\n'),
        ('[``a `b` c `d`', '<p>[``a <code>b</code> c <code>d</code></p>\n',
         '<p>[``a <code>b</code> c `d`</p>\n'),
        ('[```a ``b`` c ``d`` ```', '<p>[<code>a ``b`` c ``d`` </code></p>\n',
         '<p>[<code>a ``b`` c ``d`` </code></p>\n'),
        ('[`a\r\n`` b\r\nc `` tail ``d``', '<p>[`a\n<code>b c</code> tail <code>d</code></p>\n',
         '<p>[`a\n<code>b c</code> tail ``d``</p>\n'),
        ('`one` and ``two``', '<p><code>one</code> and <code>two</code></p>\n',
         '<p><code>one</code> and <code>two</code></p>\n'),
    ]:
        for binary, wanted in [(original, raw_expected), (corrected, expected)]:
            actual = cmark_gfm_render(binary, source, 10)
            if actual != wanted:
                raise ValueError(f'cmark-gfm backtick correction control failed: source={source!r} '
                                 f'expected={wanted!r} actual={actual!r}')
    corpus = json.loads((ROOT / 'crates/core/tests/data/markdown/gfm-0.29.0.gfm.13.json').read_text())
    gold_disagreements = []
    for case in corpus:
        raw = cmark_gfm_render(original, case['markdown'], 10)
        fixed = cmark_gfm_render(corrected, case['markdown'], 10)
        if raw != fixed:
            raise ValueError(f'cmark-gfm backtick correction changed official example {case["example"]}')
        if normalize_html(fixed) != normalize_html(case['html']):
            gold_disagreements.append(case['example'])
    return {'authoritative_controls': 6, 'official_examples': len(corpus),
            'original_corrected_exact_agreements': len(corpus),
            'unchanged_reference_gold_disagreements': gold_disagreements}


CMARK_EMPHASIS_CONTROLS = [{'commonmark_0_31_1': '<p>Hot <em>)and<strong>nd</strong> _ict <em>)and<strong>nd</strong> '
                       '_icd</em></em> <em>iced</em>.</p>\n',
  'gfm_0_29': '<p>Hot *)and<strong>nd</strong> _ict *)and<strong>nd</strong> _icd** '
              '<em>iced</em>.</p>\n',
  'source': 'Hot *)and**nd** _ict *)and**nd** _icd** _iced_.'},
 {'commonmark_0_31_1': '<p><em>a<strong>b</strong> c</em>*</p>\n',
  'gfm_0_29': '<p>*a<strong>b</strong> c**</p>\n',
  'source': '*a**b** c**'},
 {'commonmark_0_31_1': '<p><em>)a<strong>b</strong> c</em>*</p>\n',
  'gfm_0_29': '<p>*)a<strong>b</strong> c**</p>\n',
  'source': '*)a**b** c**'},
 {'commonmark_0_31_1': '<p><em>a_b</em> c__</p>\n',
  'gfm_0_29': '<p><em>a_b</em> c__</p>\n',
  'source': '_a_b_ c__'},
 {'commonmark_0_31_1': '<p><em>a<strong>b</strong> c</em></p>\n',
  'gfm_0_29': '<p><em>a<strong>b</strong> c</em></p>\n',
  'source': '*a**b** c*'},
 {'commonmark_0_31_1': '<p><em>)a<strong>b</strong> c</em></p>\n',
  'gfm_0_29': '<p><em>)a<strong>b</strong> c</em></p>\n',
  'source': '*)a**b** c*'},
 {'commonmark_0_31_1': '<p>a<em><strong>b</strong></em>c</p>\n',
  'gfm_0_29': '<p>a<em><strong>b</strong></em>c</p>\n',
  'source': 'a***b***c'},
 {'commonmark_0_31_1': '<p><em><strong>a</strong></em></p>\n',
  'gfm_0_29': '<p><em><strong>a</strong></em></p>\n',
  'source': '***a***'},
 {'commonmark_0_31_1': '<p><strong>a<em>b</em>c</strong></p>\n',
  'gfm_0_29': '<p><strong>a<em>b</em>c</strong></p>\n',
  'source': '**a*b*c**'},
 {'commonmark_0_31_1': '<p><a href="u"><em>a<strong>b</strong> c</em>*</a></p>\n',
  'gfm_0_29': '<p><a href="u">*a<strong>b</strong> c**</a></p>\n',
  'source': '[*a**b** c**](u)'},
 {'commonmark_0_31_1': '<p><em>a<strong>b</strong> c</em>* and <em>d<strong>e</strong> '
                       'f</em>*</p>\n',
  'gfm_0_29': '<p>*a<strong>b</strong> c** and *d<strong>e</strong> f**</p>\n',
  'source': '*a**b** c** and *d**e** f**'}]

def verify_reference_emphasis_correction(backticks, combined):
    """Execute flanking-category controls and preserve the official corpus."""
    disagreements = 0
    for case in CMARK_EMPHASIS_CONTROLS:
        raw = cmark_gfm_render(backticks, case['source'], 10)
        corrected = cmark_gfm_render(combined, case['source'], 10)
        if raw != case['gfm_0_29'] or corrected != case['commonmark_0_31_1']:
            raise ValueError(f'cmark-gfm emphasis correction control failed: {case!r} raw={raw!r} corrected={corrected!r}')
        disagreements += int(raw != corrected)
    corpus = json.loads((ROOT / 'crates/core/tests/data/markdown/gfm-0.29.0.gfm.13.json').read_text())
    gold_disagreements = []
    for case in corpus:
        raw = cmark_gfm_render(backticks, case['markdown'], 10)
        corrected = cmark_gfm_render(combined, case['markdown'], 10)
        if raw != corrected:
            raise ValueError(f'cmark-gfm emphasis correction changed official example {case["example"]}')
        if normalize_html(corrected) != normalize_html(case['html']):
            gold_disagreements.append(case['example'])
    return {'authoritative_controls': len(CMARK_EMPHASIS_CONTROLS),
            'control_original_disagreements': disagreements,
            'official_examples': len(corpus), 'backtick_combined_exact_agreements': len(corpus),
            'unchanged_reference_gold_disagreements': gold_disagreements}


CMARK_DESTINATION_CONTROLS = [{'source': '> [x]: a(b\n', 'raw_html': '<blockquote>\n</blockquote>\n', 'corrected_html': '<blockquote>\n<p>[x]: a(b</p>\n</blockquote>\n'}, {'source': '>[te\na]: /te(a "Tea~"\n\nother\n', 'raw_html': '<blockquote>\n</blockquote>\n<p>other</p>\n', 'corrected_html': '<blockquote>\n<p>[te\na]: /te(a &quot;Tea~&quot;</p>\n</blockquote>\n<p>other</p>\n'}, {'source': '> > [x]: a(b\n', 'raw_html': '<blockquote>\n<blockquote>\n</blockquote>\n</blockquote>\n', 'corrected_html': '<blockquote>\n<blockquote>\n<p>[x]: a(b</p>\n</blockquote>\n</blockquote>\n'}, {'source': '- [x]: a(b\n\nother\n', 'raw_html': '<ul>\n<li></li>\n</ul>\n<p>other</p>\n', 'corrected_html': '<ul>\n<li>[x]: a(b</li>\n</ul>\n<p>other</p>\n'}, {'source': '> [x]: a(b)\n\n[x]\n', 'raw_html': '<blockquote>\n</blockquote>\n<p><a href="a(b)">x</a></p>\n', 'corrected_html': '<blockquote>\n</blockquote>\n<p><a href="a(b)">x</a></p>\n'}, {'source': '> [x]: a\\(b\n\n[x]\n', 'raw_html': '<blockquote>\n</blockquote>\n<p><a href="a(b">x</a></p>\n', 'corrected_html': '<blockquote>\n</blockquote>\n<p><a href="a(b">x</a></p>\n'}, {'source': '> [x]: <a(b>\n\n[x]\n', 'raw_html': '<blockquote>\n</blockquote>\n<p><a href="a(b">x</a></p>\n', 'corrected_html': '<blockquote>\n</blockquote>\n<p><a href="a(b">x</a></p>\n'}, {'source': '> [x]: a(b\n\n[x]: /valid\n\n[x]\n', 'raw_html': '<blockquote>\n</blockquote>\n<p><a href="a(b">x</a></p>\n', 'corrected_html': '<blockquote>\n<p><a href="/valid">x</a>: a(b</p>\n</blockquote>\n<p><a href="/valid">x</a></p>\n'}, {'source': '> [x]: a(b\n\n[x]\n', 'raw_html': '<blockquote>\n</blockquote>\n<p><a href="a(b">x</a></p>\n', 'corrected_html': '<blockquote>\n<p>[x]: a(b</p>\n</blockquote>\n<p>[x]</p>\n'}]


def verify_reference_destination_correction(previous, corrected):
    """Execute literal-definition, container and valid-resolution controls."""
    disagreements = 0
    for case in CMARK_DESTINATION_CONTROLS:
        for ending in ('\n', '\r\n', '\r'):
            source = case['source'].replace('\n', ending)
            raw = cmark_gfm_render(previous, source, 10)
            fixed = cmark_gfm_render(corrected, source, 10)
            if raw != case['raw_html'] or fixed != case['corrected_html']:
                raise ValueError(f'cmark-gfm destination correction control failed: {source!r} raw={raw!r} corrected={fixed!r}')
            disagreements += int(raw != fixed)
    corpus = json.loads((ROOT / 'crates/core/tests/data/markdown/gfm-0.29.0.gfm.13.json').read_text())
    gold_disagreements = []
    for case in corpus:
        raw = cmark_gfm_render(previous, case['markdown'], 10)
        fixed = cmark_gfm_render(corrected, case['markdown'], 10)
        if raw != fixed:
            raise ValueError(f'cmark-gfm destination correction changed official example {case["example"]}')
        if normalize_html(fixed) != normalize_html(case['html']):
            gold_disagreements.append(case['example'])
    return {'authoritative_controls': len(CMARK_DESTINATION_CONTROLS) * 3,
            'control_previous_disagreements': disagreements,
            'official_examples': len(corpus), 'previous_corrected_exact_agreements': len(corpus),
            'unchanged_reference_gold_disagreements': gold_disagreements}


CMARK_EMAIL_CONTROLS = [{'source': '[~~strik:e~~ www.example.org\\ user@exampleww.example.orgser@exam\n]', 'previous_html': '<p>[<del>strik:e</del> www.example.org\\ user@<a href="mailto:exampleww.example.orgser@exam">exampleww.example.orgser@exam</a>\n]</p>\n', 'corrected_html': '<p>[<del>strik:e</del> www.example.org\\ user@exampleww.example.orgser@exam\n]</p>\n'}, {'source': '[~~strike~~ www.ex_er@exampwww.ex_er@exampl~om\n\\]', 'previous_html': '<p>[<del>strike</del> www.ex_er@<a href="mailto:exampwww.ex_er@exampl">exampwww.ex_er@exampl</a>~om\n]</p>\n', 'corrected_html': '<p>[<del>strike</del> www.ex_er@exampwww.ex_er@exampl~om\n]</p>\n'}, {'source': 'a@b.c@d', 'previous_html': '<p>a@<a href="mailto:b.c@d">b.c@d</a></p>\n', 'corrected_html': '<p>a@b.c@d</p>\n'}, {'source': 'a@b.c@d.e', 'previous_html': '<p>a@<a href="mailto:b.c@d.e">b.c@d.e</a></p>\n', 'corrected_html': '<p>a@<a href="mailto:b.c@d.e">b.c@d.e</a></p>\n'}, {'source': 'a@b.c@d.e@f', 'previous_html': '<p>a@b.c@<a href="mailto:d.e@f">d.e@f</a></p>\n', 'corrected_html': '<p>a@b.c@d.e@f</p>\n'}, {'source': 'a@b.c@d.e@f.g', 'previous_html': '<p>a@b.c@<a href="mailto:d.e@f.g">d.e@f.g</a></p>\n', 'corrected_html': '<p>a@b.c@<a href="mailto:d.e@f.g">d.e@f.g</a></p>\n'}, {'source': '[a@b.c@d](mailto:a@b)', 'previous_html': '<p><a href="mailto:a@b">a@b.c@d</a></p>\n', 'corrected_html': '<p><a href="mailto:a@b">a@b.c@d</a></p>\n'}, {'source': '[label]: mailto:a@b\n\n[label] a@b.c@d', 'previous_html': '<p><a href="mailto:a@b">label</a> a@<a href="mailto:b.c@d">b.c@d</a></p>\n', 'corrected_html': '<p><a href="mailto:a@b">label</a> a@b.c@d</p>\n'}, {'source': '`a@b.c@d`', 'previous_html': '<p><code>a@b.c@d</code></p>\n', 'corrected_html': '<p><code>a@b.c@d</code></p>\n'}, {'source': '<a href="mailto:a@b">a@b</a> a@b.c@d', 'previous_html': '<p><a href="mailto:a@b">a@b</a> a@<a href="mailto:b.c@d">b.c@d</a></p>\n', 'corrected_html': '<p><a href="mailto:a@b">a@b</a> a@b.c@d</p>\n'}, {'source': 'a@b.c&#64;d', 'previous_html': '<p>a@<a href="mailto:b.c@d">b.c@d</a></p>\n', 'corrected_html': '<p>a@b.c@d</p>\n'}, {'source': 'a@b.c\\@d', 'previous_html': '<p>a@<a href="mailto:b.c@d">b.c@d</a></p>\n', 'corrected_html': '<p>a@b.c@d</p>\n'}, {'source': 'mailto:a@b.c@d', 'previous_html': '<p>mailto:a@<a href="b.c@d">b.c@d</a></p>\n', 'corrected_html': '<p>mailto:a@b.c@d</p>\n'}, {'source': 'xmpp:a@b.c@d', 'previous_html': '<p>xmpp:a@<a href="b.c@d">b.c@d</a></p>\n', 'corrected_html': '<p>xmpp:a@b.c@d</p>\n'}, {'source': 'a&#64;b.c&#64;d', 'previous_html': '<p>a@<a href="mailto:b.c@d">b.c@d</a></p>\n', 'corrected_html': '<p>a@b.c@d</p>\n'}, {'source': 'a&#x40;b.c&#x40;d', 'previous_html': '<p>a@<a href="mailto:b.c@d">b.c@d</a></p>\n', 'corrected_html': '<p>a@b.c@d</p>\n'}, {'source': 'a&commat;b.c&commat;d', 'previous_html': '<p>a@<a href="mailto:b.c@d">b.c@d</a></p>\n', 'corrected_html': '<p>a@b.c@d</p>\n'}, {'source': 'a&#64;b.c&#64;d&#46;e', 'previous_html': '<p>a@<a href="mailto:b.c@d.e">b.c@d.e</a></p>\n', 'corrected_html': '<p>a@<a href="mailto:b.c@d.e">b.c@d.e</a></p>\n'}, {'source': 'a&commat;b.c&commat;d&period;e', 'previous_html': '<p>a@<a href="mailto:b.c@d.e">b.c@d.e</a></p>\n', 'corrected_html': '<p>a@<a href="mailto:b.c@d.e">b.c@d.e</a></p>\n'}, {'source': '<a@b>', 'previous_html': '<p><a href="mailto:a@b">a@b</a></p>\n', 'corrected_html': '<p><a href="mailto:a@b">a@b</a></p>\n'}, {'source': '<span data="a@b.c@d">x</span>', 'previous_html': '<p><span data="a@b.c@d">x</span></p>\n', 'corrected_html': '<p><span data="a@b.c@d">x</span></p>\n'}, {'source': '<script>let x = "a@b.c@d";</script>', 'previous_html': '&lt;script>let x = "a@b.c@d";&lt;/script>\n', 'corrected_html': '&lt;script>let x = "a@b.c@d";&lt;/script>\n'}, {'source': 'http://example.org/a@b.c@d', 'previous_html': '<p><a href="http://example.org/a@b.c@d">http://example.org/a@b.c@d</a></p>\n', 'corrected_html': '<p><a href="http://example.org/a@b.c@d">http://example.org/a@b.c@d</a></p>\n'}]


def verify_reference_email_correction(previous, corrected):
    """Execute per-candidate period, decoded text and protected-node controls."""
    disagreements = 0
    for case in CMARK_EMAIL_CONTROLS:
        for ending in ('\n', '\r\n', '\r'):
            source = case['source'].replace('\n', ending)
            raw = cmark_gfm_render(previous, source, 10)
            fixed = cmark_gfm_render(corrected, source, 10)
            if raw != case['previous_html'] or fixed != case['corrected_html']:
                raise ValueError(f'cmark-gfm email correction control failed: {source!r} raw={raw!r} corrected={fixed!r}')
            disagreements += int(raw != fixed)
    corpus = json.loads((ROOT / 'crates/core/tests/data/markdown/gfm-0.29.0.gfm.13.json').read_text())
    gold_disagreements = []
    for case in corpus:
        raw = cmark_gfm_render(previous, case['markdown'], 10)
        fixed = cmark_gfm_render(corrected, case['markdown'], 10)
        if raw != fixed:
            raise ValueError(f'cmark-gfm email correction changed official example {case["example"]}')
        if normalize_html(fixed) != normalize_html(case['html']):
            gold_disagreements.append(case['example'])
    return {'authoritative_controls': len(CMARK_EMAIL_CONTROLS) * 3,
            'control_previous_disagreements': disagreements,
            'official_examples': len(corpus), 'previous_corrected_exact_agreements': len(corpus),
            'unchanged_reference_gold_disagreements': gold_disagreements}


_HTML_TAG = re.compile(
    r'<(/?)([a-zA-Z][a-zA-Z0-9:-]*)((?:[ \t\n\r\f]+[a-zA-Z_:][-a-zA-Z0-9_:.]*(?:="[^"]*")?)*)[ \t\n\r\f]*/?>'
)
_HTML_ATTRIBUTE = re.compile(r'[a-zA-Z_:][-a-zA-Z0-9_:.]*(?:="[^"]*")?')
_HTML_TOKEN = re.compile(
    r'<!--.*?(?:-->|--!>|\Z)|<!\[CDATA\[.*?(?:\]\]>|\Z)|'
    r'<\?[^>]*(?:>|\Z)|'
    r'<!(?i:doctype)(?=[ \t\n\r\f>])(?:[^\"\'>]|\"[^\"]*\"|\'[^\']*\')*(?:>|\Z)|'
    r'<!(?!(?i:doctype)(?=[ \t\n\r\f>]))[^>]*(?:>|\Z)|'
    r'<(/?)([a-zA-Z][a-zA-Z0-9:-]*)(?=[ \t\n\r\f/>])(?:[^\"\'>]|\"[^\"]*\"|\'[^\']*\')*>',
    re.DOTALL)
_HTML_RAW_TEXT = frozenset(('script', 'style', 'xmp', 'iframe', 'noembed',
                            'noframes', 'plaintext', 'title', 'textarea'))
_HTML_FOREIGN = frozenset(('svg', 'math'))


def _normalize_tag(match):
    closing, name, attributes = match.group(1), match.group(2), match.group(3)
    parsed_attributes = _HTML_ATTRIBUTE.findall(attributes)
    names = [attribute.split('=', 1)[0].lower() for attribute in parsed_attributes]
    # HTML duplicate attribute names are case-insensitive and the first wins.
    # Sorting duplicates would silently change the element's meaning.
    ordered_attributes = parsed_attributes if len(set(names)) != len(names) else sorted(parsed_attributes)
    rebuilt = ''.join(f' {attribute}' for attribute in ordered_attributes)
    void = name.lower() in {'area', 'base', 'br', 'col', 'embed', 'hr', 'img', 'input',
                            'link', 'meta', 'param', 'source', 'track', 'wbr'}
    slash = '/' if match.group(0).endswith('/>') and not void else ''
    return f'<{closing}{name}{rebuilt}{slash}>'


def normalize_html(text):
    """Normalize only trivial reference-tool formatting, never actual
    content: CRLF line endings, a missing/extra trailing newline, HTML
    attribute order (insignificant per the WHATWG HTML spec), and a void
    element's optional self-closing `/>` (also insignificant; `<input ...>`
    and `<input ... />` are the same element). The last two account for a
    documented spec divergence: the official GFM specification examples
    already vendored at crates/core/tests/data/markdown/gfm-0.29.0.gfm.13.json
    (e.g. example 279, `- [ ] foo\\n- [x] bar\\n`) render task-list
    checkboxes as `<input disabled="" type="checkbox">`, without a
    self-closing slash — exactly what Document::render_html produces, and
    what `crates/core/tests/markdown_render.rs::official_documents` checks
    byte-for-byte against that same corpus — while the `cmark-gfm` CLI's own
    default HTML renderer always self-closes void elements and orders
    attributes differently, diverging from its own project's specification
    fixture in this purely stylistic way.
    """
    text = text.replace('\r\n', '\n')
    pieces = []
    cursor = 0
    opaque_eof = False
    while (opening := text.find('<', cursor)) != -1:
        pieces.append(text[cursor:opening])
        token = _HTML_TOKEN.match(text, opening)
        if token is None:
            # An unrecognized outer token can contain apparent inner tags in
            # an unquoted attribute or declaration. Preserve its whole suffix
            # instead of searching past it and inventing a normalization site.
            pieces.append(text[opening:])
            cursor = len(text)
            opaque_eof = True
            break
        spelling = token[0]
        name = (token[2] or '').lower()
        tag = _HTML_TAG.fullmatch(spelling)
        if not token[1] and name in _HTML_RAW_TEXT | _HTML_FOREIGN:
            # Preserve the whole raw/RCDATA/foreign fragment conservatively.
            # HTML ignores a self-closing flag on non-void raw-text elements;
            # SVG/MathML instead keep that flag and foreign attribute semantics.
            if name in _HTML_FOREIGN and spelling.endswith('/>'):
                end = token.end()
            elif name == 'plaintext':
                end = len(text)
                opaque_eof = True
            else:
                closing = re.compile(r'</' + name + r'(?=[ \t\n\r\f/>])'
                                     r'(?:[^\"\'<>]|\"[^\"]*\"|\'[^\']*\')*>', re.IGNORECASE)
                end = token.end()
                depth = 1
                # Foreign fragments can nest; inner tag normalization is never
                # attempted, including at HTML integration points.
                while depth:
                    close = closing.search(text, end)
                    if close is None:
                        end = len(text)
                        opaque_eof = True
                        break
                    if name in _HTML_FOREIGN:
                        opening = text.find('<', end)
                        nested = _HTML_TOKEN.match(text, opening) if opening != -1 else None
                        if opening != -1 and opening < close.start() and nested is None:
                            end = len(text)
                            opaque_eof = True
                            break
                        if nested is not None and nested.start() < close.start():
                            if not nested[1] and (nested[2] or '').lower() == name and not nested[0].endswith('/>'):
                                depth += 1
                            end = nested.end()
                            continue
                    depth -= 1
                    end = close.end()
            pieces.append(text[token.start():end])
            cursor = end
        else:
            if (spelling.startswith('<!--') and not spelling.endswith(('-->', '--!>'))
                    or spelling.startswith('<![CDATA[') and not spelling.endswith(']]>')
                    or spelling.startswith(('<?', '<!')) and not spelling.endswith('>')):
                opaque_eof = True
            pieces.append(_normalize_tag(tag) if tag else spelling)
            cursor = token.end()
    pieces.append(text[cursor:])
    text = ''.join(pieces)
    # A renderer's final formatting newline is insignificant only outside a
    # content-bearing unterminated raw/RCDATA/foreign fragment.
    return text if opaque_eof else text.rstrip('\n') + '\n'


_TASK_HTML_MARKER = re.compile(r'(<li>(?:\n<p>)?)\[([ \txX\v\f\n])\](?=[ \t\v\f\n])([ \t\n]*)')


def tasklist_projection_limit_reasons(source):
    """Certify inputs whose neutral HTML loses task-marker source information.

    These limits are selected from raw input, never from core checkbox output.
    The neutral full-content comparison remains mandatory for all of them.
    """
    reasons = []
    if '<' in source:
        reasons.append('raw HTML can contain literal marker-looking list markup')
    if '&' in source:
        reasons.append('entity decoding can create marker-looking literal text')
    if '\\' in source:
        reasons.append('backslash escapes can create marker-looking literal text')
    if re.search(r'\[[ \t]*(?:\r\n|\r|\n)', source):
        reasons.append('neutral softbreak HTML loses split-marker indentation and whitespace')
    if re.search(r'\[\s*[xX]\s*\]:', source):
        reasons.append('a reference definition can hide the literal checked marker')
    if 'empty task separator consists only of spaces or tabs' in tasklist_reference_limit_reasons(source):
        reasons.append('neutral empty-task HTML loses the required trailing separator')
    return reasons


def check_tasklist_projection(plain_html, full_html, source=None):
    """Apply the specification's literal first-paragraph task-marker rule.

    Raw-source certificates identify normalization or reference-admission
    ambiguity before execution. Those inputs retain both renderings and
    receive an explicit unavailable count, never a completed full projection.
    """
    if source is not None and tasklist_projection_limit_reasons(source):
        return False
    def replace(match):
        checked = ' checked=""' if match[2].lower() == 'x' else ''
        return match[1] + f'<input{checked} disabled="" type="checkbox"> '
    expected = _TASK_HTML_MARKER.sub(replace, normalize_html(plain_html))
    full = normalize_html(full_html)
    if expected != full:
        raise DifferentialMismatch(f'tasklist specification projection mismatch: expected={expected!r} actual={full!r}')
    return True


def check_markdown_differential(record, binary, timeout, counts=None, *, corrected_backticks_binary=None, corrected_emphasis_binary=None, corrected_destinations_binary=None, corrected_emails_binary=None):
    if record.get('kind') != 'markdown':
        raise ValueError(f'unexpected differential record for target markdown: {record!r}')
    admission_profile = reference_admission_profile(record['source'])
    admission_selected = admission_profile != 'gfm'
    if record.get('reference_admission_profile', admission_profile) != admission_profile:
        raise ValueError('reference admission profile does not match its raw-input certificate')
    email_selected = email_reference_profile(record['source'])
    email_profile = 'reset-email-candidate-periods' if email_selected else 'gfm'
    if record.get('reference_email_profile', email_profile) != email_profile:
        raise ValueError('email profile does not match its raw-input certificate')
    if email_selected and (corrected_emails_binary is None or corrected_destinations_binary is None):
        raise ValueError('email profile requires the corrected executable and its preceding destination baseline')
    paragraph_selected = generated_paragraph_prefix_reference_profile(record['source'])
    paragraph_profile = 'generated-paragraph-prefix' if paragraph_selected else 'gfm'
    if record.get('reference_paragraph_prefix_profile', paragraph_profile) != paragraph_profile:
        raise ValueError('paragraph-prefix profile does not match its raw-input certificate')
    hardbreak_selected = generated_hardbreak_reference_profile(record['source'])
    hardbreak_profile = 'generated-hardbreaks' if hardbreak_selected else 'gfm'
    if record.get('reference_hardbreak_profile', hardbreak_profile) != hardbreak_profile:
        raise ValueError('hardbreak profile does not match its raw-input certificate')
    backticks_selected = '`' in record['source']
    backtick_profile = 'preserve-furthest-backtick-closer' if backticks_selected else 'gfm'
    if record.get('reference_backtick_profile', backtick_profile) != backtick_profile:
        raise ValueError('backtick profile does not match its raw-input certificate')
    if backticks_selected and corrected_backticks_binary is None:
        raise ValueError('backtick profile requires the independently corrected reference executable')
    emphasis_selected = '*' in record['source'] or '_' in record['source']
    emphasis_profile = 'separate-emphasis-opener-floors' if emphasis_selected else 'gfm'
    if record.get('reference_emphasis_profile', emphasis_profile) != emphasis_profile:
        raise ValueError('emphasis profile does not match its raw-input certificate')
    if emphasis_selected and corrected_emphasis_binary is None:
        raise ValueError('emphasis profile requires the independently corrected reference executable')
    destination_selected = destination_reference_profile(record['source'])
    destination_profile = 'balanced-bare-destinations' if destination_selected else 'gfm'
    if record.get('reference_destination_profile', destination_profile) != destination_profile:
        raise ValueError('destination profile does not match its raw-input certificate')
    if destination_selected and corrected_destinations_binary is None:
        raise ValueError('destination profile requires the independently corrected reference executable')
    limited = tasklist_reference_limited(record['source'])
    expected_profile = 'tasklist-reference-limited' if limited else 'gfm'
    if record.get('profile', 'gfm') != expected_profile:
        raise ValueError('Markdown probe profile does not match the independently selected raw-input profile')
    if limited:
        projection_limits = tasklist_projection_limit_reasons(record['source'])
        if record.get('tasklist_projection_limits', projection_limits) != projection_limits:
            raise ValueError('tasklist projection limits do not match their raw-input certificate')
        projection = check_tasklist_projection(record['html'], record['full_html'], record['source'])
        if counts is not None:
            counts['reference_profile_exclusions'] += 1
            key = 'tasklist_projection_checks' if projection else 'tasklist_projection_unavailable'
            counts[key] += 1
            digest = hashlib.sha256(record['source'].encode()).hexdigest()
            if not any(entry['sha256'] == digest for entry in counts['profile_exclusions']):
                counts['profile_exclusions'].append({'sha256': digest, 'source': record['source'],
                    'reasons': tasklist_reference_limit_reasons(record['source']),
                    'projection_unavailable_reasons': projection_limits,
                    'projection_checked': projection})
    ours = normalize_html(record['html'])
    raw_reference = cmark_gfm_render(binary, record['source'], timeout, tasklists=not limited)
    backtick_reference = cmark_gfm_render(corrected_backticks_binary, record['source'], timeout,
                                         tasklists=not limited) if backticks_selected else raw_reference
    selected_binary = corrected_emphasis_binary if emphasis_selected else (corrected_backticks_binary if backticks_selected else binary)
    selected_reference = cmark_gfm_render(selected_binary, record['source'], timeout,
                                         tasklists=not limited) if emphasis_selected else backtick_reference
    destination_reference = cmark_gfm_render(corrected_destinations_binary, record['source'], timeout,
                                            tasklists=not limited) if destination_selected or email_selected else selected_reference
    email_reference = cmark_gfm_render(corrected_emails_binary, record['source'], timeout,
                                      tasklists=not limited) if email_selected else destination_reference
    theirs = normalize_html(email_reference)
    guard_binary = corrected_emails_binary if email_selected else (corrected_destinations_binary if destination_selected else selected_binary)
    if email_selected and counts is not None:
        counts['email_reference_checks'] = counts.get('email_reference_checks', 0) + 1
        disagreed = normalize_html(destination_reference) != normalize_html(email_reference)
        counts['email_reference_disagreements'] = counts.get('email_reference_disagreements', 0) + int(disagreed)
        cases = counts.setdefault('email_reference_cases', [])
        digest = hashlib.sha256(record['source'].encode()).hexdigest()
        if disagreed and not any(case['sha256'] == digest for case in cases):
            cases.append({'sha256': digest, 'source': record['source'],
                          'reason': 'each retried email domain requires its own period after its at-sign',
                          'patch_sha256': CMARK_EMAIL_PATCH_SHA256,
                          'core_html': record['html'], 'full_core_html': record.get('full_html', record['html']),
                          'raw_reference_html': raw_reference, 'reference_html_before_correction': destination_reference,
                          'corrected_reference_html': email_reference})
    if destination_selected and counts is not None:
        counts['destination_reference_checks'] = counts.get('destination_reference_checks', 0) + 1
        disagreed = normalize_html(selected_reference) != normalize_html(destination_reference)
        counts['destination_reference_disagreements'] = counts.get('destination_reference_disagreements', 0) + int(disagreed)
        cases = counts.setdefault('destination_reference_cases', [])
        digest = hashlib.sha256(record['source'].encode()).hexdigest()
        if disagreed and not any(case['sha256'] == digest for case in cases):
            cases.append({'sha256': digest, 'source': record['source'],
                          'reason': 'bare link destinations require balanced unescaped parentheses',
                          'patch_sha256': CMARK_DESTINATION_PATCH_SHA256,
                          'core_html': record['html'], 'full_core_html': record.get('full_html', record['html']),
                          'raw_reference_html': raw_reference, 'reference_html_before_correction': selected_reference,
                          'corrected_reference_html': destination_reference})
    if backticks_selected and counts is not None:
        counts['backtick_reference_checks'] = counts.get('backtick_reference_checks', 0) + 1
        disagreed = normalize_html(raw_reference) != normalize_html(backtick_reference)
        counts['backtick_reference_disagreements'] = counts.get('backtick_reference_disagreements', 0) + int(disagreed)
        cases = counts.setdefault('backtick_reference_cases', [])
        digest = hashlib.sha256(record['source'].encode()).hexdigest()
        if disagreed and not any(case['sha256'] == digest for case in cases):
            cases.append({'sha256': digest, 'source': record['source'],
                          'reason': 'reference backtick cache must preserve its furthest known closer',
                          'patch_sha256': CMARK_BACKTICK_PATCH_SHA256,
                          'profile': record.get('profile', 'gfm'),
                          'core_html': record['html'], 'full_core_html': record.get('full_html', record['html']),
                          'raw_reference_html': raw_reference,
                          'corrected_reference_html': backtick_reference})
    if emphasis_selected and counts is not None:
        counts['emphasis_reference_checks'] = counts.get('emphasis_reference_checks', 0) + 1
        disagreed = normalize_html(backtick_reference) != normalize_html(selected_reference)
        counts['emphasis_reference_disagreements'] = counts.get('emphasis_reference_disagreements', 0) + int(disagreed)
        cases = counts.setdefault('emphasis_reference_cases', [])
        digest = hashlib.sha256(record['source'].encode()).hexdigest()
        if disagreed and not any(case['sha256'] == digest for case in cases):
            cases.append({'sha256': digest, 'source': record['source'],
                          'reason': 'close-only and dual-purpose emphasis runs require distinct opener floors',
                          'patch_sha256': CMARK_EMPHASIS_PATCH_SHA256,
                          'profile': record.get('profile', 'gfm'),
                          'core_html': record['html'], 'full_core_html': record.get('full_html', record['html']),
                          'raw_reference_html': raw_reference,
                          'backtick_reference_html': backtick_reference,
                          'corrected_reference_html': selected_reference})
    if limited and counts is not None:
        digest = hashlib.sha256(record['source'].encode()).hexdigest()
        case = next(entry for entry in counts['profile_exclusions'] if entry['sha256'] == digest)
        if 'raw_reference_html' not in case:
            case.update({'core_neutral_html': record['html'], 'core_full_html': record['full_html'],
                         'neutral_reference_html': raw_reference,
                         'raw_reference_html': cmark_gfm_render(binary, record['source'], timeout)})
    if admission_selected:
        # The pinned reference tracks parenthesis depth but fails to reject a
        # nonzero depth at whitespace. A plain prefix prevents that invalid
        # definition from being consumed. All inline/extension behavior and
        # all subsequent blocks still come from the independent renderer.
        guarded = cmark_gfm_render(guard_binary, _REFERENCE_GUARD + record['source'], timeout,
                                   tasklists=not limited)
        prefix = _GUARDED_BLOCK.match(guarded)
        if prefix is None:
            raise ValueError('literal-reference oracle did not preserve its exact block guard')
        guard_start = prefix.end() - len(_REFERENCE_GUARD)
        theirs = normalize_html(guarded[:guard_start] + guarded[prefix.end():])
        if counts is not None:
            counts['reference_admission_projections'] = counts.get('reference_admission_projections', 0) + 1
            counts['reference_admission_disagreements'] = counts.get('reference_admission_disagreements', 0) + int(normalize_html(raw_reference) != ours)
            cases = counts.setdefault('reference_admission_cases', [])
            digest = hashlib.sha256(record['source'].encode()).hexdigest()
            if not any(entry['sha256'] == digest for entry in cases):
                cases.append({'sha256': digest, 'source': record['source'],
                              'reason': ('unbalanced bare reference destination must remain literal'
                                         if admission_profile == 'literal-unbalanced-destination'
                                         else 'escaped terminal title delimiter cannot close a reference title'),
                              'admission_profile': admission_profile,
                              'core_html': record['html'], 'raw_reference_html': raw_reference,
                              'guarded_reference_html': guarded,
                              'guarded_source_sha256': hashlib.sha256((_REFERENCE_GUARD + record['source']).encode()).hexdigest()})
    if hardbreak_selected:
        before_projection = theirs
        theirs = project_reference_hardbreaks(record['source'], theirs)
        if counts is not None:
            counts['hardbreak_reference_projections'] = counts.get('hardbreak_reference_projections', 0) + 1
            counts['hardbreak_reference_disagreements'] = counts.get('hardbreak_reference_disagreements', 0) + int(before_projection != theirs)
            cases = counts.setdefault('hardbreak_reference_cases', [])
            identity = hashlib.sha256(record['source'].encode()).hexdigest()
            if before_projection != theirs and not any(case['sha256'] == identity for case in cases):
                cases.append({'sha256': identity, 'source': record['source'],
                              'reason': 'generated hardbreaks ignore subsequent physical indentation',
                              'profile': record.get('profile', 'gfm'),
                              'core_html': record['html'], 'full_core_html': record.get('full_html', record['html']),
                              'raw_reference_html': raw_reference,
                              'reference_html_before_projection': before_projection,
                              'projected_reference_html': theirs})
    if paragraph_selected:
        before_projection = theirs
        theirs = project_reference_paragraph_prefix(record['source'], theirs)
        if counts is not None:
            counts['paragraph_prefix_reference_projections'] = counts.get('paragraph_prefix_reference_projections', 0) + 1
            counts['paragraph_prefix_reference_disagreements'] = counts.get('paragraph_prefix_reference_disagreements', 0) + int(before_projection != theirs)
            cases = counts.setdefault('paragraph_prefix_reference_cases', [])
            identity = hashlib.sha256(record['source'].encode()).hexdigest()
            if before_projection != theirs and not any(case['sha256'] == identity for case in cases):
                cases.append({'sha256': identity, 'source': record['source'],
                              'reason': 'generated paragraphs remove initial physical spaces and tabs',
                              'profile': record.get('profile', 'gfm'),
                              'core_html': record['html'], 'full_core_html': record.get('full_html', record['html']),
                              'raw_reference_html': raw_reference,
                              'reference_html_before_projection': before_projection,
                              'projected_reference_html': theirs})
    if ours != theirs:
        raise DifferentialMismatch(
            'markdown differential mismatch: source='
            f'{record["source"]!r}\npurrassist_core={ours!r}\ncmark_gfm={theirs!r}'
        )
    return True


def check_differential(target, record, oracle, timeout, counts=None):
    kind = TARGETS[target]['differential']
    if kind is None:
        raise ValueError(f'target {target!r} emitted an unexpected differential record: {record!r}')
    if kind == 'pattern':
        return check_pattern_differential(record, timeout, counts)
    if kind == 'markdown':
        return check_markdown_differential(record, oracle['cmark_gfm_binary'], timeout, counts,
            corrected_backticks_binary=oracle.get('cmark_gfm_backticks_binary'),
            corrected_emphasis_binary=oracle.get('cmark_gfm_emphasis_binary'),
            corrected_destinations_binary=oracle.get('cmark_gfm_destinations_binary'),
            corrected_emails_binary=oracle.get('cmark_gfm_emails_binary'))
    raise ValueError(f'unknown differential kind: {kind}')


def rustc_sysroot_tool(name):
    """Resolve an `llvm-tools` component binary bundled with the active
    `rustc` (installed via `rustup component add llvm-tools`), rather than a
    separately installed system package: the bundled copy is always built
    from the exact same LLVM revision as `rustc` itself, which a
    coincidentally version-matched system package is not guaranteed to be.
    """
    sysroot = Path(subprocess.check_output(['rustc', '--print', 'sysroot'], text=True).strip())
    info = subprocess.check_output(['rustc', '-vV'], text=True)
    host = re.search(r'^host:\s+(\S+)$', info, re.MULTILINE)
    if not host:
        raise ValueError('could not determine the rustc host triple')
    path = sysroot / 'lib' / 'rustlib' / host[1] / 'bin' / name
    if not path.is_file():
        raise ValueError(f'{name} not found at {path}; run `rustup component add llvm-tools`')
    return path


def tool_identity(target):
    rustc = subprocess.check_output(['rustc', '-vV'], text=True)
    inventory = json.loads((ROOT / 'vendor/tooling.json').read_text())
    expected = next(entry['version'] for entry in inventory if entry['name'] == 'llvm-coverage')
    versions = {'rustc': rustc.strip()}
    tool_paths = {}
    for tool in ('llvm-cov', 'llvm-profdata'):
        tool_paths[tool] = rustc_sysroot_tool(tool)
        versions[tool] = subprocess.check_output([str(tool_paths[tool]), '--version'], text=True).strip()
    for tool, text in versions.items():
        match = re.search(r'LLVM version:?\s+(\S+)', text)
        # The bundled `llvm-tools` component reports "<version>-rust-<rustc
        # version>"; a standalone LLVM install reports a bare version. Only
        # the leading dotted version is a genuine LLVM release identity.
        reported = match[1].split('-rust-')[0] if match else None
        if reported != expected:
            raise ValueError(f'{tool} LLVM version differs from inventory {expected}')
    kind = TARGETS[target]['differential']
    oracle = {}
    if kind == 'pattern':
        node_expected = next(entry['version'] for entry in inventory if entry['name'] == 'node')
        node_version = subprocess.check_output(['node', '--version'], text=True).strip()
        if node_version != f'v{node_expected}':
            raise ValueError(f'node version differs from inventory {node_expected}')
        versions['node'] = node_version
    if kind == 'markdown':
        cmark_expected = next(entry['version'] for entry in inventory if entry['name'] == 'cmark-gfm')
        binary = ensure_cmark_gfm()
        cmark_version_output = subprocess.check_output([str(binary), '--version'], text=True).strip()
        if cmark_expected not in cmark_version_output:
            raise ValueError(f'cmark-gfm version differs from inventory {cmark_expected}')
        versions['cmark-gfm'] = cmark_version_output
        oracle['cmark_gfm_binary'] = binary
        corrected = ensure_cmark_gfm(corrected_backticks=True)
        compatibility = verify_reference_backtick_correction(binary, corrected)
        versions['cmark-gfm-backtick-correction'] = {
            'version': subprocess.check_output([str(corrected), '--version'], text=True).strip(),
            'source_sha256': CMARK_GFM_SOURCE_SHA256,
            'patch_sha256': CMARK_BACKTICK_PATCH_SHA256,
            'patch_preimage': CMARK_BACKTICK_BEFORE,
            'patch_postimage': CMARK_BACKTICK_AFTER,
            'corrected_inlines_sha256': hashlib.sha256(
                (corrected.parent.parent / 'src' / 'src' / 'inlines.c').read_bytes()).hexdigest(),
            'binary_sha256': hashlib.sha256(corrected.read_bytes()).hexdigest(),
            'original_binary_sha256': hashlib.sha256(binary.read_bytes()).hexdigest()}
        versions['cmark-gfm-backtick-correction']['compatibility'] = compatibility
        oracle['cmark_gfm_backticks_binary'] = corrected
        combined = ensure_cmark_gfm(corrected_backticks=True, corrected_emphasis=True)
        emphasis_compatibility = verify_reference_emphasis_correction(corrected, combined)
        versions['cmark-gfm-emphasis-correction'] = {
            'version': subprocess.check_output([str(combined), '--version'], text=True).strip(),
            'source_sha256': CMARK_GFM_SOURCE_SHA256,
            'backtick_patch_sha256': CMARK_BACKTICK_PATCH_SHA256,
            'emphasis_patch_sha256': CMARK_EMPHASIS_PATCH_SHA256,
            'emphasis_patch_preimages_postimages': CMARK_EMPHASIS_PATCHES,
            'corrected_inlines_sha256': hashlib.sha256(
                (combined.parent.parent / 'src' / 'src' / 'inlines.c').read_bytes()).hexdigest(),
            'binary_sha256': hashlib.sha256(combined.read_bytes()).hexdigest(),
            'original_binary_sha256': hashlib.sha256(binary.read_bytes()).hexdigest(),
            'compatibility': emphasis_compatibility}
        oracle['cmark_gfm_emphasis_binary'] = combined
        destinations = ensure_cmark_gfm(corrected_backticks=True, corrected_emphasis=True, corrected_destinations=True)
        destination_compatibility = verify_reference_destination_correction(combined, destinations)
        versions['cmark-gfm-destination-correction'] = {
            'version': subprocess.check_output([str(destinations), '--version'], text=True).strip(),
            'source_sha256': CMARK_GFM_SOURCE_SHA256,
            'backtick_patch_sha256': CMARK_BACKTICK_PATCH_SHA256,
            'emphasis_patch_sha256': CMARK_EMPHASIS_PATCH_SHA256,
            'destination_patch_sha256': CMARK_DESTINATION_PATCH_SHA256,
            'destination_preimage': CMARK_DESTINATION_BEFORE,
            'destination_postimage': CMARK_DESTINATION_AFTER,
            'corrected_inlines_sha256': hashlib.sha256(
                (destinations.parent.parent / 'src' / 'src' / 'inlines.c').read_bytes()).hexdigest(),
            'binary_sha256': hashlib.sha256(destinations.read_bytes()).hexdigest(),
            'previous_binary_sha256': hashlib.sha256(combined.read_bytes()).hexdigest(),
            'original_binary_sha256': hashlib.sha256(binary.read_bytes()).hexdigest(),
            'compatibility': destination_compatibility}
        oracle['cmark_gfm_destinations_binary'] = destinations
        emails = ensure_cmark_gfm(corrected_backticks=True, corrected_emphasis=True, corrected_destinations=True, corrected_emails=True)
        email_compatibility = verify_reference_email_correction(destinations, emails)
        versions['cmark-gfm-email-correction'] = {
            'version': subprocess.check_output([str(emails), '--version'], text=True).strip(),
            'source_sha256': CMARK_GFM_SOURCE_SHA256,
            'backtick_patch_sha256': CMARK_BACKTICK_PATCH_SHA256,
            'emphasis_patch_sha256': CMARK_EMPHASIS_PATCH_SHA256,
            'destination_patch_sha256': CMARK_DESTINATION_PATCH_SHA256,
            'email_patch_sha256': CMARK_EMAIL_PATCH_SHA256,
            'email_preimage': CMARK_EMAIL_BEFORE, 'email_postimage': CMARK_EMAIL_AFTER,
            'corrected_inlines_sha256': hashlib.sha256(
                (emails.parent.parent / 'src' / 'src' / 'inlines.c').read_bytes()).hexdigest(),
            'corrected_autolink_sha256': hashlib.sha256(
                (emails.parent.parent / 'src' / 'extensions' / 'autolink.c').read_bytes()).hexdigest(),
            'binary_sha256': hashlib.sha256(emails.read_bytes()).hexdigest(),
            'previous_binary_sha256': hashlib.sha256(destinations.read_bytes()).hexdigest(),
            'original_binary_sha256': hashlib.sha256(binary.read_bytes()).hexdigest(),
            'compatibility': email_compatibility}
        oracle['cmark_gfm_emails_binary'] = emails
    return versions, oracle, tool_paths


def input_identity(target):
    """Identify source inputs independently of mutable documentation/reports."""
    sources = sorted(path for path in (ROOT / 'crates/core/src').rglob('*') if path.is_file())
    sources += [ROOT / name for name in ('Cargo.toml', 'Cargo.lock', 'rust-toolchain.toml', 'crates/core/Cargo.toml', 'crates/core/README.md')]
    digest = hashlib.sha256()
    for path in sorted(sources):
        digest.update(str(path.relative_to(ROOT)).encode() + b'\0' + hashlib.sha256(path.read_bytes()).digest())
    return {
        'engine_source_sha256': digest.hexdigest(),
        'harness_source_sha256': hashlib.sha256(harness_source(target).read_bytes()).hexdigest(),
        'runner_source_sha256': hashlib.sha256(Path(__file__).read_bytes()).hexdigest(),
    }


def build(directory, target):
    sources_before = input_identity(target)
    environment = dict(os.environ)
    flags = environment.get('CARGO_ENCODED_RUSTFLAGS')
    flags = flags.split('\x1f') if flags is not None else environment.get('RUSTFLAGS', '').split()
    environment['CARGO_ENCODED_RUSTFLAGS'] = '\x1f'.join(flags + ['-C', 'instrument-coverage'])
    example = f'fuzz-{target}'
    command = ['cargo', 'build', '-p', 'purrassist-core', '--example', example, '--locked',
               '--config', 'profile.dev.opt-level=1', '--config', 'profile.dev.debug=1',
               '--config', 'profile.dev.strip=false', '--config', 'profile.dev.debug-assertions=true',
               '--config', 'profile.dev.overflow-checks=true', '--message-format=json']
    result = subprocess.run(command, cwd=ROOT, env=environment, capture_output=True, text=True)
    (directory / 'build.log').write_text(result.stderr)
    result.check_returncode()
    if input_identity(target) != sources_before:
        raise ValueError('fuzz build source changed during compilation; rerun on stable inputs')
    artifacts = [json.loads(line) for line in result.stdout.splitlines()]
    binaries = [row['executable'] for row in artifacts if row.get('target', {}).get('name') == example and row.get('executable')]
    if len(binaries) != 1:
        raise ValueError(f'expected exactly one probe executable for {example}')
    built_binary = Path(binaries[0])
    # Cargo reuses its output path. Snapshot the executable so a concurrent
    # rebuild cannot change the implementation between campaign executions.
    binary = directory / 'probe'
    shutil.copy2(built_binary, binary)
    if input_identity(target) != sources_before:
        raise ValueError('fuzz build source changed before executable capture; rerun')
    return binary, {'command': command, 'sha256': hashlib.sha256(binary.read_bytes()).hexdigest(),
                    'inherited_rustflags_sha256': hashlib.sha256('\x1f'.join(flags).encode()).hexdigest(),
                    'appended_rustflags': ['-C', 'instrument-coverage'], **sources_before}


def check_markdown_stop(record, source, counts=None):
    """Validate a closed declared non-result; never count it as comparison."""
    if not isinstance(record, dict) or set(record) != {'kind', 'source', 'stage', 'reason', 'resource'} or record['kind'] != 'resource-stop':
        raise ValueError('Markdown stop record must contain only its closed protocol fields')
    if not isinstance(record['stage'], str) or not isinstance(record['reason'], str):
        raise ValueError('Markdown stop stage and reason must be strings')
    if record['resource'] is not None and not isinstance(record['resource'], str):
        raise ValueError('Markdown stop resource must be a string or null')
    if record['source'] != source or record['stage'] not in {'parse', 'render', 'neutral-render'}:
        raise ValueError('Markdown stop record source or stage is invalid')
    if record['reason'] == 'BudgetExceeded':
        if record['resource'] not in {'Work', 'Candidates', 'InputBytes', 'ScratchBytes', 'Depth', 'EvidenceEntries', 'HelperCalls'}:
            raise ValueError('Markdown stop resource is invalid')
    elif record['reason'] == 'Cancelled':
        if record['resource'] is not None:
            raise ValueError('Markdown cancellation cannot name an exhausted resource')
    else:
        raise ValueError('unexpected Markdown stop reason')
    if counts is not None:
        counts['resource_non_results'] = counts.get('resource_non_results', 0) + 1
        key = ':'.join((record['stage'], record['reason'], record['resource'] or 'none'))
        reasons = counts.setdefault('resource_non_result_reasons', {})
        reasons[key] = reasons.get(key, 0) + 1
        cases = counts.setdefault('resource_non_result_cases', [])
        identity = hashlib.sha256(source.encode()).hexdigest()
        if not any(entry['sha256'] == identity and entry['stage'] == record['stage'] and entry['reason'] == record['reason'] and entry['resource'] == record['resource'] for entry in cases):
            cases.append({'sha256': identity, **record})


def probe(binary, data, directory, timeout, target, oracle, tool_paths, counts=None):
    # Remove prior successful profiles so stale counters can never supply feedback.
    raw, indexed = directory / 'current.profraw', directory / 'current.profdata'
    raw.unlink(missing_ok=True)
    indexed.unlink(missing_ok=True)
    (directory / 'current.bin').write_bytes(data)
    (directory / 'probe.stderr').write_bytes(b'')
    environment = dict(os.environ, LLVM_PROFILE_FILE=str(raw), LC_ALL='C')
    environment.pop('PURRASSIST_FUZZ_TASKLIST_REFERENCE_PROFILE', None)
    original_markdown = None
    admission_selected = False
    admission_profile = 'gfm'
    hardbreak_selected = False
    paragraph_selected = False
    email_selected = False
    backticks_selected = False
    emphasis_selected = False
    destination_selected = False
    task_projection_limits = []
    if target == 'markdown':
        try:
            original_markdown = data.decode('utf-8')
            limited = tasklist_reference_limited(original_markdown)
            admission_profile = reference_admission_profile(original_markdown)
            admission_selected = admission_profile != 'gfm'
            paragraph_selected = generated_paragraph_prefix_reference_profile(original_markdown)
            email_selected = email_reference_profile(original_markdown)
            hardbreak_selected = generated_hardbreak_reference_profile(original_markdown)
            backticks_selected = '`' in original_markdown
            emphasis_selected = '*' in original_markdown or '_' in original_markdown
            destination_selected = destination_reference_profile(original_markdown)
            task_projection_limits = tasklist_projection_limit_reasons(original_markdown)
        except UnicodeDecodeError:
            limited = False
        if limited:
            environment['PURRASSIST_FUZZ_TASKLIST_REFERENCE_PROFILE'] = 'tasklist-neutral'
    result = subprocess.run([str(binary)], input=data, capture_output=True, env=environment, timeout=timeout)
    (directory / 'probe.stderr').write_bytes(result.stderr)
    if result.returncode:
        if counts is not None:
            counts['crashes'] += 1
        result.check_returncode()
    try:
        data.decode('utf-8')
        invalid = False
    except UnicodeDecodeError:
        invalid = True
    if invalid and admits_bytes(target):
        # A byte-input probe recovers the bytes; nothing was rejected.
        invalid = False
        if counts is not None:
            counts['non_utf8_admitted'] = counts.get('non_utf8_admitted', 0) + 1
    if result.stderr:
        raise ValueError('probe reported a diagnostic on stderr')
    lines = result.stdout.split(b'\n')
    if lines and lines[-1] == b'':
        lines.pop()
    expected_first = b'invalid-utf8' if invalid else b'ok'
    if not lines or lines[0] != expected_first:
        raise ValueError('probe output differs from its completion protocol')
    if invalid and len(lines) != 1:
        raise ValueError('an invalid-utf8 rejection must not emit differential records')
    if target == 'markdown' and not invalid and len(lines) != 2:
        raise ValueError('Markdown completion requires exactly one HTML or declared-stop record')
    checked = 0
    for line in lines[1:]:
        record = json.loads(line)
        if target == 'markdown':
            if record.get('source') != original_markdown:
                raise ValueError('Markdown differential record must retain the exact admitted input')
            if record.get('kind') == 'resource-stop':
                check_markdown_stop(record, original_markdown, counts)
                continue
            record['reference_admission_profile'] = admission_profile
            record['reference_email_profile'] = 'reset-email-candidate-periods' if email_selected else 'gfm'
            record['reference_paragraph_prefix_profile'] = 'generated-paragraph-prefix' if paragraph_selected else 'gfm'
            record['reference_hardbreak_profile'] = 'generated-hardbreaks' if hardbreak_selected else 'gfm'
            record['reference_emphasis_profile'] = 'separate-emphasis-opener-floors' if emphasis_selected else 'gfm'
            record['reference_backtick_profile'] = 'preserve-furthest-backtick-closer' if backticks_selected else 'gfm'
            record['reference_destination_profile'] = 'balanced-bare-destinations' if destination_selected else 'gfm'
            record['tasklist_projection_limits'] = task_projection_limits
        if check_differential(target, record, oracle, timeout, counts):
            checked += 1
    merged = subprocess.run(
        [str(tool_paths['llvm-profdata']), 'merge', '-sparse', str(raw), '-o', str(indexed)],
        check=True, capture_output=True, timeout=timeout,
    )
    exported = subprocess.run(
        [str(tool_paths['llvm-cov']), 'export', str(binary), f'-instr-profile={indexed}'],
        check=True, capture_output=True, timeout=timeout,
    )
    if merged.stderr or exported.stderr:
        raise ValueError('LLVM reported a coverage diagnostic')
    return covered_regions(json.loads(exported.stdout)), invalid, checked


def source_digest():
    names = subprocess.check_output(['git', 'ls-files', '-co', '--exclude-standard', '-z'], cwd=ROOT).split(b'\0')
    digest = hashlib.sha256()
    for name in sorted(set(names) - {b''}):
        path = ROOT / os.fsdecode(name)
        if path.is_file():
            digest.update(name + b'\0' + hashlib.sha256(path.read_bytes()).digest())
    return digest.hexdigest()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--target', required=True, choices=sorted(TARGETS), help='fuzz target')
    parser.add_argument('--iterations', type=int, default=1000, help='mutations after all fixed seeds')
    parser.add_argument('--seed', type=int, default=0x5A1727EE614D934B)
    parser.add_argument('--timeout', type=float, default=10, help='seconds per probe, LLVM, or oracle command')
    parser.add_argument('--replay', type=Path, help='execute one saved input instead of a campaign')
    args = parser.parse_args()
    if args.iterations < 0 or not 0 <= args.seed < 1 << 64 or not 0 < args.timeout <= 3600:
        parser.error('invalid iteration, seed, or timeout bound')
    target = args.target
    tokens = TARGETS[target]['tokens']
    cache = ROOT / '.cache'
    cache.mkdir(exist_ok=True)
    directory = Path(tempfile.mkdtemp(prefix=f'fuzz-{target}-', dir=cache))
    (directory / 'corpus').mkdir()
    print(f'Campaign artifacts: {directory}', flush=True)
    tools, oracle, tool_paths = tool_identity(target)
    binary, build_identity = build(directory, target)
    report = {'format': 'purrassist.fuzz.v1', 'target': target, 'status': 'running', 'tools': tools,
              'source_commit': subprocess.check_output(['git', 'rev-parse', 'HEAD'], cwd=ROOT, text=True).strip(),
              'source_dirty': bool(subprocess.check_output(['git', 'status', '--porcelain'], cwd=ROOT)),
              'nonignored_file_tree_sha256': source_digest(),
              'build': build_identity, 'seed': args.seed, 'requested_mutations': args.iterations,
              'timeout_seconds': args.timeout, 'maximum_input_bytes': 4096,
              'executions': 0, 'invalid_utf8': 0, 'non_utf8_admitted': 0, 'differential_checks': 0, 'differential_mismatches': 0,
              'crashes': 0, 'reference_rejections': 0, 'reference_timeouts': 0,
              'resource_non_results': 0, 'resource_non_result_reasons': {}, 'resource_non_result_cases': [],
              'reference_profile_exclusions': 0, 'tasklist_projection_checks': 0,
              'tasklist_projection_unavailable': 0, 'profile_exclusions': [],
              'reference_admission_projections': 0, 'reference_admission_disagreements': 0,
              'reference_admission_cases': [],
              'hardbreak_reference_projections': 0, 'hardbreak_reference_disagreements': 0,
              'hardbreak_reference_cases': [],
              'paragraph_prefix_reference_projections': 0, 'paragraph_prefix_reference_disagreements': 0,
              'paragraph_prefix_reference_cases': [],
              'email_reference_checks': 0, 'email_reference_disagreements': 0,
              'email_reference_cases': [],
              'emphasis_reference_checks': 0, 'emphasis_reference_disagreements': 0,
              'emphasis_reference_cases': [],
              'backtick_reference_checks': 0, 'backtick_reference_disagreements': 0,
              'backtick_reference_cases': [],
              'destination_reference_checks': 0, 'destination_reference_disagreements': 0,
              'destination_reference_cases': [],
              'retained': [], 'regions': 0}
    corpus, seen = [], set()
    random = Random(args.seed)
    seeds = [args.replay.read_bytes()] if args.replay else TARGETS[target]['seeds']
    if any(len(data) > 4096 for data in seeds):
        raise ValueError('replay exceeds probe input limit')
    mutations = 0 if args.replay else args.iterations
    try:
        for index in range(len(seeds) + mutations):
            parent = None
            if index < len(seeds):
                data = seeds[index]
            else:
                parent = random.index(len(corpus))
                data = mutate(corpus[parent], random, tokens)
            report['current_execution'] = index
            regions, invalid, checked = probe(
                binary, data, directory, args.timeout, target, oracle, tool_paths, report
            )
            report['differential_checks'] += checked
            novel = regions - seen
            seen.update(regions)
            if novel or index < len(seeds):
                digest = hashlib.sha256(data).hexdigest()
                (directory / 'corpus' / f'{digest}.bin').write_bytes(data)
                corpus.append(data)
                report['retained'].append({'execution': index, 'sha256': digest, 'new_regions': len(novel),
                                           'parent_corpus_index': parent, 'bytes': len(data)})
            report['executions'] += 1
            report['invalid_utf8'] += invalid
            report['regions'] = len(seen)
            if index % 100 == 0:
                print(f'{index + 1} executions; {len(corpus)} corpus entries; {len(seen)} core regions; '
                      f'{report["differential_checks"]} differential checks', flush=True)
        if not seen:
            if not args.replay:
                raise ValueError('campaign produced no covered core regions')
        report['status'] = 'passed'
    except (Exception, KeyboardInterrupt) as error:
        report['status'] = 'failed'
        report['failure_type'] = type(error).__name__
        if isinstance(error, DifferentialMismatch):
            report['differential_mismatches'] += 1
        (directory / 'failure.txt').write_text(repr(error) + '\n')
        print(f'Failure retained in {directory / "current.bin"}', flush=True)
        raise
    finally:
        (directory / 'report.json').write_text(json.dumps(report, indent=2) + '\n')
        (directory / 'regions.json').write_text(json.dumps(sorted(seen), indent=2) + '\n')
    print(f'Passed {report["executions"]} inputs; report: {directory / "report.json"}', flush=True)


if __name__ == '__main__':
    main()
