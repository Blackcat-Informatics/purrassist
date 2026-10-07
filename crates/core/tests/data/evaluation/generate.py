#!/usr/bin/env python3
# SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
# SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0
"""Deterministic, first-party evaluation-corpus generator for Task 13.

Produces four JSON corpora consumed by `crates/core/tests/correction_evaluation.rs`:

- `strings.json` -- string-selection cases (`purrassist_core::matching`).
- `paths.json` -- path-matching cases (`purrassist_core::paths`).
- `phonetic.json` -- English near-homophone cases (`purrassist_core::matching`).
- `json_recovery.json` -- JSON Schema recovery cases (`purrassist_core::schema`).

Every case records its query/input, its candidates/schema, and either the
single correct answer (`truth`) or an explicit `"should abstain"` ground
truth (`truth: null`). No third-party corpus is used: word lists, phrase
templates, and homophone pairs below are curated by hand for this generator;
the only external input is the project's own pinned UTS #39 confusables
fixture (`crates/core/tests/data/unicode/confusables.txt`), read to source
*authentic* Unicode confusable characters (never hand-invented lookalikes).

Deterministic: no random-number generation. Re-running this script on an
unchanged confusables fixture reproduces byte-identical output. Run with
`python3 crates/core/tests/data/evaluation/generate.py` from anywhere; it
locates the repository root from its own path.
"""
import copy
import hashlib
import json
from pathlib import Path

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[4]
CONFUSABLES = ROOT / "crates/core/tests/data/unicode/confusables.txt"

WORDS = [
    "reader", "writer", "customer", "service", "project", "market", "invoice",
    "document", "picture", "feature", "capital", "calendar", "hospital",
    "exercise", "journey", "teacher", "student", "kitchen", "garden", "office",
    "morning", "evening", "holiday", "weather", "airport", "stadium",
    "library", "factory", "village", "bridge", "camera", "rocket", "planet",
    "desert", "forest", "valley", "harbor", "castle", "engine", "signal",
]

DIACRITIC_WORDS = [
    ("cafe", "café"), ("naive", "naïve"), ("resume", "résumé"),
    ("jalapeno", "jalapeño"), ("facade", "façade"), ("pinata", "piñata"),
    ("fiancee", "fiancée"), ("cliche", "cliché"), ("entree", "entrée"),
    ("nee", "née"), ("expose", "exposé"), ("soiree", "soirée"),
    ("creme", "crème"), ("puree", "purée"), ("saute", "sauté"),
    ("attache", "attaché"), ("blase", "blasé"), ("canape", "canapé"),
    ("consomme", "consommé"), ("decor", "décor"),
]

STRASSE_WORDS = [
    ("strasse", "Straße"), ("grossen", "Großen"), ("massgabe", "Maßgabe"),
    ("weissbier", "Weißbier"), ("fussball", "Fußball"), ("spass", "Spaß"),
    ("strassenbahn", "Straßenbahn"), ("nussbaum", "Nußbaum"),
    ("schluessel", "Schlüssel"), ("gruessen", "grüßen"),
]

NEGATION_PHRASES = [
    "ready device", "available slot", "stable connection", "active session",
    "valid token", "online server", "complete backup", "working sync",
    "connected client", "healthy cluster", "verified account",
    "confirmed order", "trusted source", "safe mode", "known issue",
    "successful build", "supported platform", "authorized user",
    "initialized state", "equal shares", "clear signal", "happy customer",
]
NEGATION_MARKERS = ["not", "no", "non-hyphen", "un-hyphen", "bang"]


def negate(phrase, marker):
    first, rest = phrase.split(" ", 1)
    if marker == "not":
        return f"not {phrase}"
    if marker == "no":
        return f"no {phrase}"
    if marker == "non-hyphen":
        return f"non-{first} {rest}"
    if marker == "un-hyphen":
        return f"un-{first} {rest}"
    if marker == "bang":
        return f"{phrase}!"
    raise ValueError(marker)


def load_confusables():
    """Cyrillic-to-Latin single-codepoint confusable prototypes actually
    present in the pinned UTS #39 fixture; never a hand-authored table."""
    table = {}
    for line in CONFUSABLES.read_text(encoding="utf-8").splitlines():
        if not line or line.startswith("#"):
            continue
        fields = line.split(";")
        if len(fields) < 2:
            continue
        source = fields[0].split()
        target = fields[1].split()
        if len(source) != 1 or len(target) != 1:
            continue
        source_char = chr(int(source[0], 16))
        target_char = chr(int(target[0], 16))
        if target_char in "abcdefghijklmnopqrstuvwxyz" and 0x0400 <= ord(source_char) <= 0x04FF:
            table.setdefault(target_char, source_char)
    return table


def fullwidth(text):
    return "".join(chr(ord(c) + 0xFEE0) if "a" <= c <= "z" or "A" <= c <= "Z" else c for c in text)


def string_cases():
    cases = []
    confusables = load_confusables()

    def add(category, query, choices, truth, minimum_score=7500):
        cases.append({
            "id": f"{category}-{len(cases)}",
            "category": category,
            "query": query,
            "candidates": [
                {"id": cid, "text": text, "aliases": aliases}
                for (cid, text, aliases) in choices
            ],
            "truth": truth,
            "minimum_score": minimum_score,
        })

    def distractors(index, count=2):
        return [WORDS[(index + 1 + offset) % len(WORDS)] for offset in range(count)]

    # Confusable skeleton: one authentic Cyrillic lookalike substituted for
    # every position of every word that contains a confusable-mapped letter.
    for index, word in enumerate(WORDS):
        for position, letter in enumerate(word):
            if letter in confusables:
                mutated = word[:position] + confusables[letter] + word[position + 1:]
                dist = distractors(index)
                add("confusable", mutated,
                    [("right", word, []), ("wrong1", dist[0], []), ("wrong2", dist[1], [])],
                    "right")

    # Width/compatibility fold: full word and half-word fullwidth mutation.
    for index, word in enumerate(WORDS):
        dist = distractors(index)
        add("width", fullwidth(word),
            [("right", word, []), ("wrong1", dist[0], []), ("wrong2", dist[1], [])], "right")
        half = len(word) // 2
        add("width", fullwidth(word[:half]) + word[half:],
            [("right", word, []), ("wrong1", dist[0], []), ("wrong2", dist[1], [])], "right")

    # Case fold: all-uppercase and Title-case mutation.
    for index, word in enumerate(WORDS):
        dist = distractors(index)
        for mutated in (word.upper(), word.capitalize()):
            add("case_fold", mutated,
                [("right", word, []), ("wrong1", dist[0], []), ("wrong2", dist[1], [])], "right")

    # Diacritic-strip (missing accent) and NFC canonical composition
    # (decomposed base letter + combining acute accent vs. the precomposed
    # candidate spelling, before any case/width folding).
    for index, (plain, accented) in enumerate(DIACRITIC_WORDS):
        dist = [WORDS[index % len(WORDS)], WORDS[(index + 5) % len(WORDS)]]
        add("diacritic", plain,
            [("right", accented, []), ("wrong1", dist[0], []), ("wrong2", dist[1], [])], "right")
        decomposed = accented.replace("é", "e\u0301").replace("è", "e\u0300").replace(
            "ñ", "n\u0303").replace("ç", "c\u0327")
        if decomposed != accented:
            add("diacritic", decomposed,
                [("right", accented, []), ("wrong1", dist[0], []), ("wrong2", dist[1], [])], "right")

    # Full Unicode case folding beyond simple case (ß -> ss).
    for index, (plain, special) in enumerate(STRASSE_WORDS):
        dist = [WORDS[index % len(WORDS)], WORDS[(index + 7) % len(WORDS)]]
        add("case_fold", plain,
            [("right", special, []), ("wrong1", dist[0], []), ("wrong2", dist[1], [])], "right")

    # Depunctuation: compound identifiers written with different separators.
    for index in range(len(WORDS) - 1):
        left, right = WORDS[index], WORDS[index + 1]
        query = f"{left}_{right}"
        for separator, category_suffix in ((" ", "space"), ("", "joined")):
            candidate_text = f"{left}{separator}{right}" if separator else f"{left}{right}"
            dist = distractors(index)
            add(f"depunctuation_{category_suffix}", query,
                [("right", candidate_text, []), ("wrong1", dist[0], []), ("wrong2", dist[1], [])],
                "right")

    # Protected class: negation markers exclude an otherwise strong match.
    for phrase in NEGATION_PHRASES:
        for marker in NEGATION_MARKERS:
            add("negation", phrase, [("wrong", negate(phrase, marker), [])], None)

    # Protected class: semantic-version components.
    for major in range(1, 4):
        for minor in range(0, 4):
            for patch in (0, 1, 5, 9):
                base = f"release-v{major}.{minor}.{patch}"
                other = f"release-v{major}.{minor}.{patch + 1}"
                add("semver", base, [("wrong", other, [])], None)

    # Protected class: ISO calendar dates.
    for year in (2023, 2024, 2025):
        for month in (1, 6, 12):
            for day in (1, 15, 28):
                base = f"log-{year:04d}-{month:02d}-{day:02d}-error"
                other_day = day + 1 if day < 28 else day - 1
                other = f"log-{year:04d}-{month:02d}-{other_day:02d}-error"
                add("iso_date", base, [("wrong", other, [])], None)

    # Protected class: hexadecimal digests (>= 7 hex characters, one letter).
    for seed in range(60):
        digest = format(0xDEAD_BEEF_0000 + seed, "x").rjust(8, "0")
        other = digest[:-1] + ("f" if digest[-1] != "f" else "e")
        add("hex_digest", f"build-{digest}", [("wrong", f"build-{other}", [])], None)

    # Protected class: quantities paired with a unit abbreviation.
    for amount in (1, 5, 10, 30, 60, 100, 250, 500):
        for unit_a, unit_b in (("ms", "s"), ("kb", "mb"), ("mb", "gb"), ("s", "min")):
            add("unit", f"timeout {amount}{unit_a}", [("wrong", f"timeout {amount}{unit_b}", [])], None)

    # Digit runs beyond "invoice": abstain on a genuine collision, resolve
    # on a same-digits spelling typo, resolve via an exact caller alias.
    nouns = ["invoice", "ticket", "order", "case", "receipt", "shipment", "claim", "batch"]
    for index, noun in enumerate(nouns):
        for number in (1001, 2050, 3399, 4210, 5678):
            base = f"{noun}-{number}"
            wrong_number = f"{noun}-{number + 1}"
            add("digit_run", base, [("wrong", wrong_number, [])], None)
            chars = list(noun)
            if len(chars) >= 2:
                chars[0], chars[1] = chars[1], chars[0]
            typo = "".join(chars) + f"-{number}"
            add("digit_run", typo,
                [("right", base, []), ("wrong", wrong_number, [])], "right")
        base = f"{noun}-9001"
        add("digit_run", base,
            [("right", f"{noun}-9002", [base]), ("wrong", f"{noun}-9003", [])], "right")

    # Caller-declared aliases: a domain alias with no textual signal,
    # competing aliases producing ambiguity, and Effort::Exact ignoring aliases.
    domain_pairs = [
        ("iced tea", "cold tea"), ("soda", "fizzy drink"), ("cellphone", "mobile"),
        ("trunk", "boot"), ("apartment", "flat"), ("elevator", "lift"),
        ("sneakers", "trainers"), ("cookie", "biscuit"), ("candy", "sweets"),
        ("truck", "lorry"),
    ]
    for query, canonical_text in domain_pairs:
        cases.append({
            "id": f"alias-{len(cases)}", "category": "alias", "query": query,
            "candidates": [
                {"id": "right", "text": canonical_text, "aliases": [query]},
                {"id": "wrong", "text": "unrelated thing", "aliases": []},
            ],
            "truth": "right", "minimum_score": 7500,
        })
        cases.append({
            "id": f"alias-{len(cases)}", "category": "alias_ambiguous", "query": query,
            "candidates": [
                {"id": "one", "text": canonical_text, "aliases": [query]},
                {"id": "two", "text": canonical_text + " deluxe", "aliases": [query]},
            ],
            "truth": None, "minimum_score": 7500,
        })

    # Unrelated strings: a true-negative baseline that must stay abstained.
    for index in range(len(WORDS)):
        left = WORDS[index]
        right = WORDS[(index + len(WORDS) // 2) % len(WORDS)]
        add("unrelated", left, [("wrong", right, [])], None)

    return cases


CONTEXT_WORDS = [
    "reader", "writer", "parser", "loader", "helper", "runner", "worker",
    "server", "client", "engine", "router", "sensor", "widget", "gadget",
    "sample", "record", "ledger", "cursor", "buffer", "socket",
] + WORDS
DIRECTORIES = [
    "src", "lib", "tests", "docs", "examples", "tools", "scripts",
    "config", "assets", "vendor", "build", "dist", "internal", "public",
    "shared", "modules",
]


def path_cases():
    cases = []
    confusables = load_confusables()

    def add(category, paths, query, context, truth):
        cases.append({
            "id": f"{category}-{len(cases)}", "category": category,
            "paths": paths, "query": query, "context": context, "truth": truth,
        })

    # Basename case-fold: unique candidate under one directory.
    for index, word in enumerate(CONTEXT_WORDS):
        directory = DIRECTORIES[index % len(DIRECTORIES)]
        for extension in ("rs", "py", "md"):
            target = f"{directory}/{word}.{extension}"
            for mutated in (f"{word}.{extension}".upper(), f"{word.capitalize()}.{extension}"):
                add("case_fold", [target], mutated, directory, target)

    # Width basename mutation (full and half-word fullwidth).
    for index, word in enumerate(CONTEXT_WORDS):
        directory = DIRECTORIES[(index + 1) % len(DIRECTORIES)]
        target = f"{directory}/{word}.md"
        add("width", [target], fullwidth(word) + ".md", directory, target)
        half = len(word) // 2
        add("width", [target], fullwidth(word[:half]) + word[half:] + ".md", directory, target)

    # Diacritic basename mutation: missing accent and NFC decomposed form.
    for index, (plain, accented) in enumerate(DIACRITIC_WORDS):
        directory = DIRECTORIES[index % len(DIRECTORIES)]
        target = f"{directory}/{accented}.txt"
        add("diacritic", [target], f"{plain}.txt", directory, target)
        decomposed = (accented
            .replace("\u00e9", "e\u0301")
            .replace("\u00e8", "e\u0300")
            .replace("\u00f1", "n\u0303")
            .replace("\u00e7", "c\u0327"))
        if decomposed != accented:
            add("diacritic", [target], f"{decomposed}.txt", directory, target)

    # Confusable-skeleton basename mutation: every confusable-mapped letter
    # position in every context word.
    for index, word in enumerate(CONTEXT_WORDS):
        for position, letter in enumerate(word):
            if letter in confusables:
                directory = DIRECTORIES[index % len(DIRECTORIES)]
                target = f"{directory}/{word}.py"
                mutated = word[:position] + confusables[letter] + word[position + 1:] + ".py"
                add("confusable", [target], mutated, directory, target)

    # Depunctuation in filenames, both directions and both separators.
    for index in range(len(CONTEXT_WORDS) - 1):
        left, right = CONTEXT_WORDS[index], CONTEXT_WORDS[index + 1]
        directory = DIRECTORIES[index % len(DIRECTORIES)]
        target = f"{directory}/{left}_{right}.rs"
        add("depunctuation", [target], f"{left}-{right}.rs", directory, target)
        add("depunctuation", [target], f"{left} {right}.rs", directory, target)
        reverse_target = f"{directory}/{left}-{right}.py"
        add("depunctuation", [reverse_target], f"{left}_{right}.py", directory, reverse_target)

    # Near-duplicate basenames disambiguated (or not) by parent context,
    # across several directory-pair offsets.
    for offset in (1, 2, 3, 5, 7):
        for index, word in enumerate(CONTEXT_WORDS):
            first = DIRECTORIES[index % len(DIRECTORIES)]
            second = DIRECTORIES[(index + offset) % len(DIRECTORIES)]
            if first == second:
                continue
            a, b = f"{first}/{word}.rs", f"{second}/{word}.rs"
            add("context_disambiguation", [a, b], f"{word}.rs", first, a)
            add("context_ambiguous", [a, b], f"{word}.rs", "", None)

    # Protected classes carried through to path basenames.
    for index, noun in enumerate(["invoice", "ticket", "order", "receipt"]):
        directory = DIRECTORIES[index % len(DIRECTORIES)]
        other = f"{directory}/{noun}-1002.pdf"
        add("digit_run", [other], f"{noun}-1001.pdf", directory, None)
        add("iso_date", [f"{directory}/log-2024-01-16.txt"], "log-2024-01-15.txt", directory, None)

    # Traversal and portability rejection: the query itself is not a
    # candidate the index could ever contain, so it must never resolve.
    for invalid in ["../secrets", "/absolute", "a/../b", "a//b", "C:/file", "a\\b", ""]:
        add("invalid", [f"{DIRECTORIES[0]}/reader.rs"], invalid, "", None)

    return cases


HOMOPHONE_PAIRS = [
    ("their", "there"), ("to", "too"), ("whether", "weather"), ("where", "wear"),
    ("bare", "bear"), ("break", "brake"), ("cite", "sight"), ("flour", "flower"),
    ("hair", "hare"), ("hear", "here"), ("knight", "night"), ("made", "maid"),
    ("mail", "male"), ("meat", "meet"), ("pair", "pear"), ("peace", "piece"),
    ("plain", "plane"), ("right", "write"), ("sale", "sail"), ("sea", "see"),
    ("sole", "soul"), ("some", "sum"), ("son", "sun"), ("steak", "stake"),
    ("suite", "sweet"), ("tale", "tail"), ("threw", "through"), ("waist", "waste"),
    ("wait", "weight"), ("way", "weigh"), ("weak", "week"), ("wood", "would"),
    ("aisle", "isle"), ("ate", "eight"), ("buy", "by"), ("cell", "sell"),
    ("coarse", "course"), ("dear", "deer"), ("die", "dye"), ("fair", "fare"),
    ("fir", "fur"), ("flea", "flee"), ("grate", "great"), ("groan", "grown"),
    ("heal", "heel"), ("heard", "herd"), ("hoarse", "horse"), ("hole", "whole"),
    ("hour", "our"), ("idle", "idol"), ("knot", "not"), ("loan", "lone"),
    ("mane", "main"), ("missed", "mist"), ("muscle", "mussel"), ("naval", "navel"),
    ("pain", "pane"), ("passed", "past"), ("peal", "peel"), ("peer", "pier"),
    ("pleas", "please"), ("pole", "poll"), ("pray", "prey"), ("rain", "reign"),
    ("read", "reed"), ("real", "reel"), ("road", "rode"), ("root", "route"),
    ("scene", "seen"), ("seam", "seem"), ("shear", "sheer"), ("soar", "sore"),
    ("stair", "stare"), ("steal", "steel"), ("straight", "strait"), ("tacks", "tax"),
    ("team", "teem"), ("tide", "tied"), ("toad", "towed"), ("vain", "vein"),
    ("wade", "weighed"), ("waive", "wave"), ("whine", "wine"), ("board", "bored"),
    ("bald", "bawled"), ("bass", "base"), ("berry", "bury"), ("bread", "bred"),
    ("chord", "cord"), ("days", "daze"), ("doe", "dough"), ("duel", "dual"),
    ("earn", "urn"), ("faze", "phase"), ("feat", "feet"), ("gait", "gate"),
    ("hall", "haul"), ("him", "hymn"), ("knead", "need"), ("knew", "new"),
    ("know", "no"), ("lessen", "lesson"), ("liar", "lyre"), ("locks", "lox"),
    ("meddle", "medal"), ("morning", "mourning"), ("none", "nun"), ("oar", "or"),
    ("pause", "paws"), ("praise", "prays"), ("profit", "prophet"), ("quarts", "quartz"),
    ("raise", "rays"), ("rap", "wrap"), ("ring", "wring"), ("rote", "wrote"),
    ("rye", "wry"), ("sear", "seer"), ("shone", "shown"), ("slay", "sleigh"),
    ("summary", "summery"), ("told", "tolled"), ("ton", "tun"), ("vale", "veil"),
    ("wail", "whale"), ("which", "witch"), ("your", "you're"), ("yoke", "yolk"),
]
DISTRACTOR_WORDS = [
    "office", "market", "castle", "signal", "planet", "forest", "bridge",
    "camera", "garden", "harbor",
]


def phonetic_cases():
    cases = []
    for index, (left, right) in enumerate(HOMOPHONE_PAIRS):
        for offset in range(5):
            wrong = [DISTRACTOR_WORDS[(index + offset) % len(DISTRACTOR_WORDS)],
                     DISTRACTOR_WORDS[(index + offset + 1) % len(DISTRACTOR_WORDS)]]
            cases.append({
                "id": f"phonetic-{len(cases)}", "query": left,
                "candidates": [
                    {"id": "right", "text": right},
                    {"id": "wrong1", "text": wrong[0]},
                    {"id": "wrong2", "text": wrong[1]},
                ],
                "truth": "right", "minimum_score": 6000,
            })
            cases.append({
                "id": f"phonetic-{len(cases)}", "query": right,
                "candidates": [
                    {"id": "right", "text": left},
                    {"id": "wrong1", "text": wrong[0]},
                    {"id": "wrong2", "text": wrong[1]},
                ],
                "truth": "right", "minimum_score": 6000,
            })
    # Two equally supported candidate identities must preserve ambiguity.
    # These are deliberate negative examples, independent of whether the
    # homophone is recognized by Soundex or by an edit-distance signal.
    for left, right in HOMOPHONE_PAIRS:
        cases.append({
            "id": f"phonetic-ambiguous-{len(cases)}", "query": left,
            "candidates": [{"id": "one", "text": right},
                           {"id": "two", "text": right}],
            "truth": None, "minimum_score": 6000,
        })
    return cases


SCHEMAS = [
    {
        "schema": {"type": "object", "required": ["temperature", "unit"],
                    "properties": {"temperature": {"type": "number"}, "unit": {"enum": ["c", "f"]}},
                    "additionalProperties": False},
        "value": {"temperature": 21.5, "unit": "c"},
    },
    {
        "schema": {"type": "object", "required": ["name", "count"],
                    "properties": {"name": {"type": "string"}, "count": {"type": "integer"}},
                    "additionalProperties": False},
        "value": {"name": "widgets", "count": 12},
    },
    {
        "schema": {"type": "array", "items": {"type": "integer"}, "minItems": 1},
        "value": [1, 2, 3],
    },
    {
        "schema": {"type": "object", "required": ["drink"],
                    "properties": {"drink": {"enum": ["tea", "coffee", "water"]}},
                    "additionalProperties": False},
        "value": {"drink": "tea"},
    },
    {
        "schema": {"type": "object", "required": ["active"],
                    "properties": {"active": {"type": "boolean"}}, "additionalProperties": False},
        "value": {"active": True},
    },
    {
        "schema": {"type": "object", "required": ["items"],
                    "properties": {"items": {"type": "array", "items": {"type": "string"}}},
                    "additionalProperties": False},
        "value": {"items": ["a", "b", "c"]},
    },
    {
        "schema": {"type": "object", "required": ["id", "label"],
                    "properties": {"id": {"type": "integer"}, "label": {"type": "string"}},
                    "additionalProperties": False},
        "value": {"id": 7, "label": "widget"},
    },
    {
        "schema": {"type": "object", "required": ["score"],
                    "properties": {"score": {"type": "number"}}, "additionalProperties": False},
        "value": {"score": 42},
    },
]


def canonical(value):
    if isinstance(value, bool):
        return "true" if value else "false"
    if isinstance(value, int):
        return str(value)
    if isinstance(value, float):
        text = repr(value)
        return text
    if isinstance(value, str):
        return json.dumps(value)
    if isinstance(value, list):
        return "[" + ",".join(canonical(item) for item in value) + "]"
    if isinstance(value, dict):
        return "{" + ",".join(f"{json.dumps(k)}:{canonical(v)}" for k, v in value.items()) + "}"
    raise TypeError(value)


def truncate(text):
    trimmed = text.rstrip("}]")
    return trimmed if len(trimmed) < len(text) else None


def prose_wrap(text):
    return f"Sure, here is the value you asked for:\n```json\n{text}\n```\nLet me know if you need anything else!"


def key_typo(value):
    if not isinstance(value, dict) or not value:
        return None
    key = next(iter(value))
    if len(key) < 2:
        return None
    typo = key[1] + key[0] + key[2:]
    return canonical({(typo if k == key else k): v for k, v in value.items()})


def stringify_scalar(value):
    if not isinstance(value, dict) or not value:
        return None
    key = next(iter(value))
    inner = value[key]
    if not isinstance(inner, (int, float, bool)):
        return None
    mutated = dict(value)
    mutated[key] = canonical(inner)  # now a Python str; canonical() will quote it
    return canonical(mutated)


def type_swap(value):
    if not isinstance(value, dict) or not value:
        return None
    key = next(iter(value))
    inner = value[key]
    if isinstance(inner, bool):
        return None
    if isinstance(inner, int):
        mutated = dict(value)
        mutated[key] = str(inner)
        return canonical(mutated)
    return None


def duplicate_key(value, schema=None):
    """Duplicate the first key with a genuinely different, still
    schema-valid value, so the corruption is unambiguously ambiguous
    (matching `{"x":1,"x":2}` in `crates/core/benches/recovery.rs`) rather
    than two agreeing occurrences, or an enum violation, a recoverer could
    reasonably resolve to one deterministic answer."""
    if not isinstance(value, dict) or not value:
        return None
    key = next(iter(value))
    inner = value[key]
    enum = None
    if isinstance(schema, dict):
        enum = schema.get("properties", {}).get(key, {}).get("enum")
    if enum:
        other = next((member for member in enum if member != inner), None)
        if other is None:
            return None
    elif isinstance(inner, bool):
        other = not inner
    elif isinstance(inner, int):
        other = inner + 1
    elif isinstance(inner, float):
        other = inner + 1.0
    elif isinstance(inner, str):
        other = inner + "x"
    else:
        return None
    text = canonical(value)
    return text[:-1] + f',{json.dumps(key)}:{canonical(other)}' + text[-1]


def schema_variants():
    """Vary schema identities and original values, never just whitespace.

    Fourteen variants of each first-party template produce 112 distinct
    schema/value pairs. Named properties receive suffixes, with required
    names changed consistently; strings, numeric values and arrays vary.
    """
    def vary(value, variant):
        if isinstance(value, bool):
            return bool(variant % 2)
        if isinstance(value, (int, float)):
            return value + variant * 17
        if isinstance(value, str):
            return value + WORDS[variant]
        if isinstance(value, list):
            return [vary(item, variant) for item in value] + [vary(value[0], variant + 1)]
        raise TypeError(value)

    for variant in range(14):
        for entry in SCHEMAS:
            schema = copy.deepcopy(entry["schema"])
            original = entry["value"]
            if isinstance(original, dict):
                names = {key: f"{key}_{WORDS[variant]}" for key in original}
                schema["required"] = [names[key] for key in schema["required"]]
                schema["properties"] = {names[key]: child for key, child in schema["properties"].items()}
                value = {}
                for key, child in original.items():
                    constraints = schema["properties"][names[key]]
                    enum = constraints.get("enum")
                    value[names[key]] = enum[variant % len(enum)] if enum else vary(child, variant)
            else:
                value = vary(original, variant)
                schema["minItems"] = 1 + variant % 3
            yield {"schema": schema, "value": value}


def json_recovery_cases():
    cases = []
    for template_index, entry in enumerate(schema_variants()):
        schema, value = entry["schema"], entry["value"]
        original = canonical(value)
        transforms = [
            ("truncation", truncate, True),
            ("prose_wrapping", prose_wrap, True),
            ("key_typo", key_typo, True),
            ("stringification", stringify_scalar, True),
            ("type_swap", type_swap, True),
            ("duplicate_key", lambda v: duplicate_key(v, schema), False),
        ]
        for name, transform, recoverable in transforms:
            corrupted = transform(value if name in ("key_typo", "stringification", "type_swap", "duplicate_key")
                                   else original)
            if corrupted is None:
                continue
            cases.append({
                "id": f"{name}-{template_index}",
                "schema": schema,
                "input": corrupted,
                "truth": None if not recoverable else value,
            })
        # A second, independent truncation depth and a nested nesting/prose
        # combination, multiplying coverage per schema without inventing new
        # corruption categories.
        doubly_truncated = truncate(original)
        if doubly_truncated is not None:
            deeper = truncate(doubly_truncated)
            if deeper is not None:
                cases.append({
                    "id": f"truncation-deep-{template_index}", "schema": schema,
                    "input": deeper, "truth": value,
                })
        nested_prose = prose_wrap("  \n" + original + "\n  ")
        cases.append({
            "id": f"prose-padded-{template_index}", "schema": schema,
            "input": nested_prose, "truth": value,
        })
        wrapped_array = f"[[{original}]]"
        cases.append({
            "id": f"array-wrap-{template_index}", "schema": schema,
            "input": wrapped_array, "truth": value,
        })
    return cases



def write(name, cases):
    path = HERE / name
    path.write_text(json.dumps(cases, indent=2, sort_keys=False) + "\n")
    return path


def main():
    outputs = {}
    for name, builder in (
        ("strings.json", string_cases),
        ("paths.json", path_cases),
        ("phonetic.json", phonetic_cases),
        ("json_recovery.json", json_recovery_cases),
    ):
        cases = builder()
        path = write(name, cases)
        outputs[name] = {"cases": len(cases), "distinct_queries": len({json.dumps({key: value for key, value in case.items() if key not in ("id", "category", "truth")}, sort_keys=True) for case in cases}), "sha256": hashlib.sha256(path.read_bytes()).hexdigest()}
        print(f"{name}: {len(cases)} cases")
    manifest = {
        "generator": "crates/core/tests/data/evaluation/generate.py",
        "generator_sha256": hashlib.sha256(Path(__file__).read_bytes()).hexdigest(),
        "inputs": {
            "confusables": str(CONFUSABLES.relative_to(ROOT)),
            "confusables_sha256": hashlib.sha256(CONFUSABLES.read_bytes()).hexdigest(),
        },
        "outputs": outputs,
    }
    # Corpora generated outside this script (the Rust tool-call corpus
    # generator) keep their registered entries.
    manifest_path = HERE / "manifest.json"
    if manifest_path.exists():
        previous = json.loads(manifest_path.read_text())
        if "generators" in previous:
            manifest["generators"] = previous["generators"]
    manifest_path.write_text(json.dumps(manifest, indent=2) + "\n")


if __name__ == "__main__":
    main()
