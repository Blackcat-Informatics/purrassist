// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0
//! Push-based JSON recovery is chunk-invariant and linear, and `json.extract`
//! candidate discovery stays linear in the number of brackets.
//!
//! Chunk invariance is checked over every JSON input in the recovery goldens
//! and the vendored JSON Schema suite's instance data, plus tricky inputs
//! that suspend inside escapes, surrogate pairs, comments and multi-byte
//! characters: the recovered node (with spans), consumed length, edits and
//! the governor's complete usage must equal a single-chunk run for every
//! split point and for seeded multi-split chunkings.
use purrassist_core::contract::Usage;
use purrassist_core::json::incremental::IncrementalRecovery;
use purrassist_core::json::{Node, SyntaxEdit, Value, parse};
use purrassist_core::schema::{
    Schema,
    recovery::{EXTRACT_WORK_PER_BYTE, RecoveryRequest},
};
use purrassist_core::{Budget, Effort, Governor, Outcome, StopReason};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

type Recovered = Result<(Node, usize, Vec<SyntaxEdit>), StopReason>;

fn data_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data")
}

fn json_files(directory: &Path) -> Vec<PathBuf> {
    let mut found = Vec::new();
    let mut stack = vec![directory.to_path_buf()];
    while let Some(directory) = stack.pop() {
        for entry in std::fs::read_dir(&directory).expect("directory") {
            let path = entry.expect("entry").path();
            if path.is_dir() {
                stack.push(path);
            } else if path
                .extension()
                .is_some_and(|extension| extension == "json")
            {
                found.push(path);
            }
        }
    }
    found.sort();
    found
}

/// Suite instance data longer than this is sampled out; every split point of
/// every remaining input is checked, so this bounds the quadratic test cost.
const MAX_SPLIT_INPUT: usize = 2048;

/// Every golden input, every golden fixture document, every suite instance
/// (compact) and suspension-heavy malformed inputs, deduplicated and sorted.
fn corpus() -> Vec<String> {
    let mut inputs = BTreeSet::new();
    for path in json_files(&data_dir().join("recovery-goldens")) {
        let text = std::fs::read_to_string(&path).expect("golden");
        let golden = parse(&text).expect("golden fixture is JSON");
        let input = golden.get("input").and_then(Node::as_str).expect("input");
        inputs.insert(input.to_string());
        if text.len() <= MAX_SPLIT_INPUT {
            inputs.insert(text);
        }
    }
    for path in json_files(&data_dir().join("json-schema/tests")) {
        let document = parse(&std::fs::read_to_string(&path).expect("suite")).expect("suite JSON");
        let Value::Array(groups) = &document.value else {
            continue;
        };
        for group in groups {
            if let Some(Node {
                value: Value::Array(tests),
                ..
            }) = group.get("tests")
            {
                for test in tests {
                    if let Some(data) = test.get("data") {
                        let text = data.to_json();
                        if text.len() <= MAX_SPLIT_INPUT {
                            inputs.insert(text);
                        }
                    }
                }
            }
        }
    }
    for tricky in [
        "",
        " \t\n",
        "// only a comment",
        "/* unterminated",
        "/**/ 1 /***/",
        "/*/ 1 */ 2",
        "1//c\n",
        "[1/2, /x]",
        "{/a: 1}",
        "{\"a\" /*c*/ : /*d*/ 1 // e\n}",
        "\"\\uD83D\\uDE00\"",
        "\"\\uD83D\\u0041\"",
        "\"\\uD83D\\",
        "\"\\uD83D\\x\"",
        "\"\\uD83D",
        "\"\\uD8",
        "\"\\u12\" x",
        "'\\''",
        "'it''s'",
        "“curly \\” quote” tail",
        "“unterminated",
        "\"a\u{1}b\\u001F\\u001f\\/\\n\"",
        "\"é😀猫\" {}",
        "{clé: 'valeur', \"emoji\": 😀, ключ: значение}",
        "[1,,2,]",
        "[,]",
        "{a,}",
        "{\"a\":1,",
        "[1 2 3",
        "{a b c}",
        "[:]",
        "{:1, :2}",
        "[1}",
        "{]",
        "{'a' 'b'}",
        "None",
        "TRUE",
        "[01, -0, 1e5, 1E+2, 1., -]",
        "{\"k\": [1, {\"j\": }]}",
        "\u{a0}\u{2028}[\u{3000}1\u{feff}]",
        "{\"a\":1} trailing [1]",
    ] {
        inputs.insert(tricky.to_string());
    }
    inputs.into_iter().collect()
}

/// Recover `chunks` in order with a fresh default governor.
fn recover_chunks(chunks: &[&[u8]]) -> (Recovered, Usage) {
    let mut governor = Governor::new(Budget::default());
    let mut parser = IncrementalRecovery::new(&governor);
    let mut fed = Ok(());
    for chunk in chunks {
        fed = parser.feed(chunk, &mut governor);
        if fed.is_err() {
            break;
        }
    }
    let result = match fed {
        Ok(()) => parser.finish(&mut governor),
        Err(reason) => Err(reason),
    };
    (result, governor.usage())
}

#[test]
fn every_split_equals_batch() {
    let corpus = corpus();
    assert!(corpus.len() > 1_000, "corpus has {} inputs", corpus.len());
    let mut splits = 0_usize;
    for input in &corpus {
        let bytes = input.as_bytes();
        let batch = recover_chunks(&[bytes]);
        for split in 0..=bytes.len() {
            assert_eq!(
                recover_chunks(&[&bytes[..split], &bytes[split..]]),
                batch,
                "split at byte {split} of {input:?}"
            );
            splits += 1;
        }
    }
    println!("{} inputs, {splits} split points", corpus.len());
    assert!(splits > 50_000, "only {splits} split points checked");
}

/// First-party xorshift64: deterministic, dependency-free chunk boundaries.
struct XorShift(u64);
impl XorShift {
    const fn next(&mut self) -> u64 {
        let mut state = self.0;
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        self.0 = state;
        state
    }
    fn below(&mut self, bound: usize) -> usize {
        usize::try_from(self.next() % u64::try_from(bound).expect("bound fits")).expect("fits")
    }
}

#[test]
fn seeded_multisplit_equals_batch() {
    for seed in [0x9e37_79b9_7f4a_7c15_u64, 0xd1b5_4a32_d192_ed03] {
        let mut random = XorShift(seed);
        for input in corpus() {
            let bytes = input.as_bytes();
            let batch = recover_chunks(&[bytes]);
            for _ in 0..64 {
                let cuts = 1 + random.below(8);
                let mut points: Vec<usize> =
                    (0..cuts).map(|_| random.below(bytes.len() + 1)).collect();
                points.sort_unstable();
                let mut chunks = Vec::new();
                let mut start = 0;
                for point in points {
                    chunks.push(&bytes[start..point]);
                    start = point;
                }
                chunks.push(&bytes[start..]);
                assert_eq!(
                    recover_chunks(&chunks),
                    batch,
                    "seed {seed:#x}, chunk lengths {:?} of {input:?}",
                    chunks.iter().map(|chunk| chunk.len()).collect::<Vec<_>>()
                );
            }
        }
    }
}

/// Repeat `unit` (joined by `separator`, wrapped in `open`/`close`) until the
/// document reaches `target` bytes.
fn repeated(open: &str, unit: &str, separator: &str, close: &str, target: usize) -> String {
    let mut text = String::from(open);
    while text.len() + unit.len() + separator.len() + close.len() <= target {
        if text.len() > open.len() {
            text.push_str(separator);
        }
        text.push_str(unit);
    }
    text.push_str(close);
    text
}

/// A named generator of an input of (about) the requested byte length.
type Shape = (&'static str, fn(usize) -> String);

fn work_for(input: &str) -> u64 {
    let mut governor = Governor::new(Budget {
        work: u64::MAX / 2,
        scratch_bytes: 1 << 34,
        evidence_entries: u64::MAX / 2,
        ..Budget::default()
    });
    let mut parser = IncrementalRecovery::new(&governor);
    for chunk in input.as_bytes().chunks(4096) {
        parser.feed(chunk, &mut governor).expect("feed");
    }
    let (_, consumed, _) = parser.finish(&mut governor).expect("recovered");
    assert_eq!(consumed, input.len());
    governor.usage().work
}

#[test]
fn work_is_linear() {
    let sizes = [64 * 1024, 256 * 1024, 1024 * 1024];
    let shapes: [Shape; 3] = [
        ("one large string value", |target| {
            repeated(
                "{\"text\": \"",
                "lorem ipsum \\\"quoted\\\" é 😀 \\u00e9 \\uD83D\\uDE00 ",
                "",
                "\"}",
                target,
            )
        }),
        ("wide and deep nesting", |target| {
            repeated(
                "[",
                r#"{"k":[1,{"a":"b","c":[true,null,{"d":{"e":-1.5e3}}]}],"n":{"m":{"o":{"p":[[],{}]}}}}"#,
                ",",
                "]",
                target,
            )
        }),
        ("repaired relaxed syntax", |target| {
            repeated(
                "[",
                "{key: 'value', list: [1 2,], flag: True, /* note */ none: None,}",
                ",",
                "]",
                target,
            )
        }),
    ];
    for (shape, generate) in shapes {
        let inputs: Vec<String> = sizes.iter().map(|&size| generate(size)).collect();
        let work: Vec<u64> = inputs.iter().map(|input| work_for(input)).collect();
        for index in 1..inputs.len() {
            #[allow(clippy::cast_precision_loss)]
            let byte_ratio = inputs[index].len() as f64 / inputs[0].len() as f64;
            #[allow(clippy::cast_precision_loss)]
            let work_ratio = work[index] as f64 / work[0] as f64;
            let relative = work_ratio / byte_ratio;
            println!(
                "{shape}: {} bytes -> {} work; ratio {work_ratio:.4} vs bytes {byte_ratio:.4} ({relative:.4})",
                inputs[index].len(),
                work[index]
            );
            assert!(
                (1.0 / 1.05..=1.05).contains(&relative),
                "{shape}: work ratio {work_ratio} vs byte ratio {byte_ratio}"
            );
        }
    }
}

fn assert_linear_extraction(schema: &Schema, input: &str, budget: Option<Budget>) -> Outcome<Node> {
    let result = schema.recover(
        input,
        RecoveryRequest {
            effort: Effort::Standard,
            budget,
            ..RecoveryRequest::default()
        },
    );
    assert!(
        !matches!(result.receipt.stopped, StopReason::BudgetExceeded(_)),
        "stopped: {:?}",
        result.receipt.stopped
    );
    let bound = EXTRACT_WORK_PER_BYTE * u64::try_from(input.len()).expect("length fits");
    println!(
        "{} bytes: work {} (bound {bound})",
        input.len(),
        result.receipt.usage.work
    );
    assert!(
        result.receipt.usage.work <= bound,
        "work {} exceeds {bound}",
        result.receipt.usage.work
    );
    result.outcome
}

#[test]
fn extract_many_brackets_is_linear() {
    let schema = Schema::compile(r#"{"type":"object","required":["id"]}"#).expect("schema");
    // Prose with 100 000 `{`: a run of empty placeholders before the answer
    // (dense, because the Standard preset admits 1 000 000 Work units and
    // retaining the source alone charges one per byte).
    let placeholders = format!(
        "Template fields: {} then the answer is {{\"id\": 7}}.",
        "{}".repeat(100_000)
    );
    assert_eq!(placeholders.matches('{').count(), 100_001);
    assert!(matches!(
        assert_linear_extraction(&schema, &placeholders, None),
        Outcome::Resolved(_)
    ));
    // 100 000 `{` inside a JSON string: every one used to start an attempt.
    let quoted = format!(
        "The model wrote {{\"id\": 3, \"code\": \"{}\"}} and stopped.",
        "{".repeat(100_000)
    );
    assert!(matches!(
        assert_linear_extraction(&schema, &quoted, None),
        Outcome::Resolved(_)
    ));
    // A 1 MiB tool-call argument wrapped in prose. Its recovered tree, the
    // candidate copy and the search's key storage need more than the
    // presets' 64 MiB of scratch, so this case raises only that limit; Work
    // is the Deep preset's, and still bounded per byte.
    let argument = repeated(
        "{\"id\": 1, \"rows\": [",
        r#"{"k": "v", "n": [1, 2, 3]}"#,
        ",",
        "]}",
        1 << 20,
    );
    let wrapped = format!("Calling the tool with {argument} now.");
    let budget = Budget {
        scratch_bytes: 1 << 30,
        ..Effort::Deep.budget()
    };
    assert!(matches!(
        assert_linear_extraction(&schema, &wrapped, Some(budget)),
        Outcome::Resolved(_)
    ));
}
