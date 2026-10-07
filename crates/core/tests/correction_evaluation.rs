// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0
//! Incorrect-automatic-correction evaluation over five first-party corpora
//! (`crates/core/tests/data/evaluation/`, provenance in that directory's
//! `README.md`): string selection, path matching, English phonetic
//! ambiguity, JSON Schema recovery, and tool-call recovery (a mutation
//! corpus generated from grammar witnesses by the `tool-call-corpus`
//! example, whose generator this file shares and reruns to prove the
//! committed corpus current). For each corpus this asserts **zero**
//! incorrect automatic `Resolved`/`BestGuess` corrections, computes
//! risk-coverage (precision vs. abstention), and computes the exact
//! Clopper-Pearson 95% upper confidence bound on the incorrect-correction
//! rate (via the regularized incomplete beta function, implemented below
//! with no dependencies) — then asserts that bound at or under the stated
//! limit in `docs/MATCHING-QUALITY.md`, and asserts the exact tallies that
//! document quotes. A mismatch between this file and that document is a
//! build failure by design: see the acceptance note in each test.
#[path = "tool_call_corpus/mod.rs"]
mod tool_call_corpus;

use purrassist_core::{
    Budget, Effort, Governor, Outcome,
    digest::Digest,
    json::{self, Node, Value},
    matching::{self, Candidate, MatchRequest, SelectionPolicy},
    paths::PathIndex,
    schema::{Schema, recovery::RecoveryRequest},
};
use std::collections::BTreeMap;
use std::path::Path;

// ---------------------------------------------------------------------
// Exact binomial inversion: the Clopper-Pearson upper confidence bound,
// via the regularized incomplete beta function's quantile (no dependency
// on any statistics crate; a from-scratch, independently written
// transcription of the standard continued-fraction algorithm for the
// incomplete beta function and the Lanczos approximation for the log-gamma
// function it needs).
//
// `a`/`b` are the two shape parameters of a Beta distribution and `x` is
// the point evaluated, standard notation for this textbook algorithm;
// clippy's readability/micro-optimization suggestions for this kind of
// numerical-methods code (`mul_add` rewrites, single-character shape-
// parameter names) would obscure the correspondence with the reference
// algorithm without measurably changing its result, so this module scopes
// them off rather than mechanically applying them. `cast_precision_loss`
// is genuinely safe here: every cast is a loop index bounded by
// `MAX_ITERATIONS` or a case-tally bounded by the corpus sizes above, both
// far below `f64`'s 52-bit exact-integer range.
// ---------------------------------------------------------------------
/// Lanczos approximation to `ln(Gamma(x))` for `x > 0`, accurate to
/// better than 1e-10 relative error across the small integer/half-
/// integer arguments this module calls it with.
#[allow(clippy::many_single_char_names, clippy::suboptimal_flops)]
fn ln_gamma(x: f64) -> f64 {
    const COEFFICIENTS: [f64; 6] = [
        76.180_091_729_471_46,
        -86.505_320_329_416_77,
        24.014_098_240_830_91,
        -1.231_739_572_450_155,
        0.120_865_097_386_617_9e-2,
        -0.539_523_938_495_3e-5,
    ];
    let mut y = x;
    let base = x + 5.5;
    let leading = base - (x + 0.5) * base.ln();
    let mut series = 1.000_000_000_190_015;
    for coefficient in COEFFICIENTS {
        y += 1.0;
        series += coefficient / y;
    }
    -leading + (2.506_628_274_631_000_5 * series / x).ln()
}

/// Continued-fraction evaluation used by [`incomplete_beta`] (Lentz's
/// method applied to the standard incomplete-beta continued fraction).
#[allow(
    clippy::many_single_char_names,
    clippy::cast_precision_loss,
    clippy::suboptimal_flops
)] // loop index bounded by MAX_ITERATIONS
fn continued_fraction(a: f64, b: f64, x: f64) -> f64 {
    const MAX_ITERATIONS: usize = 200;
    const EPSILON: f64 = 1e-14;
    const TINY: f64 = 1e-300;
    let sum = a + b;
    let a_plus_one = a + 1.0;
    let a_minus_one = a - 1.0;
    let mut c = 1.0_f64;
    let mut d = 1.0 - sum * x / a_plus_one;
    if d.abs() < TINY {
        d = TINY;
    }
    d = 1.0 / d;
    let mut h = d;
    for step in 1..=MAX_ITERATIONS {
        let step = step as f64;
        let step2 = 2.0 * step;
        let even = step * (b - step) * x / ((a_minus_one + step2) * (a + step2));
        d = 1.0 + even * d;
        if d.abs() < TINY {
            d = TINY;
        }
        c = 1.0 + even / c;
        if c.abs() < TINY {
            c = TINY;
        }
        d = 1.0 / d;
        h *= d * c;
        let odd = -(a + step) * (sum + step) * x / ((a + step2) * (a_plus_one + step2));
        d = 1.0 + odd * d;
        if d.abs() < TINY {
            d = TINY;
        }
        c = 1.0 + odd / c;
        if c.abs() < TINY {
            c = TINY;
        }
        d = 1.0 / d;
        let delta = d * c;
        h *= delta;
        if (delta - 1.0).abs() < EPSILON {
            break;
        }
    }
    h
}

/// The regularized incomplete beta function `I_x(a, b)`, for `a, b > 0`
/// and `x` in `[0, 1]`.
#[allow(clippy::many_single_char_names, clippy::suboptimal_flops)]
fn incomplete_beta(x: f64, a: f64, b: f64) -> f64 {
    assert!((0.0..=1.0).contains(&x), "incomplete_beta domain");
    if x <= 0.0 || x >= 1.0 {
        return x.clamp(0.0, 1.0);
    }
    let log_front = ln_gamma(a + b) - ln_gamma(a) - ln_gamma(b) + a * x.ln() + b * (1.0 - x).ln();
    let front = log_front.exp();
    if x < (a + 1.0) / (a + b + 2.0) {
        front * continued_fraction(a, b, x) / a
    } else {
        1.0 - front * continued_fraction(b, a, 1.0 - x) / b
    }
}

/// The quantile (inverse CDF) of a `Beta(a, b)` distribution at `p`,
/// found by bisection over [`incomplete_beta`] (monotone in `x`), to
/// double precision.
#[allow(clippy::many_single_char_names)]
fn quantile(p: f64, a: f64, b: f64) -> f64 {
    assert!((0.0..=1.0).contains(&p), "beta_quantile probability");
    let mut low = 0.0_f64;
    let mut high = 1.0_f64;
    for _ in 0..200 {
        let mid = low.midpoint(high);
        if incomplete_beta(mid, a, b) < p {
            low = mid;
        } else {
            high = mid;
        }
        if high - low < 1e-15 {
            break;
        }
    }
    low.midpoint(high)
}

/// The exact Clopper-Pearson one-sided upper confidence bound on a
/// binomial proportion: `incorrect` events observed out of `attempted`
/// independent trials, at confidence `1 - alpha`.
///
/// This is the textbook beta-quantile form of the Clopper-Pearson
/// interval (e.g. Clopper & Pearson, 1934): the upper bound is the
/// `1 - alpha` quantile of a `Beta(incorrect + 1, attempted -
/// incorrect)` distribution, or `1.0` when every trial was incorrect.
/// When `incorrect == 0` this is algebraically `1 - alpha^(1/attempted)`,
/// which the general beta-quantile path below reproduces (checked by
/// `clopper_pearson_upper_matches_the_closed_form_for_zero_incorrect`).
#[allow(clippy::many_single_char_names, clippy::cast_precision_loss)] // corpus-sized tallies
fn clopper_pearson_upper(incorrect: u64, attempted: u64, alpha: f64) -> f64 {
    assert!(
        attempted > 0,
        "Clopper-Pearson bound requires at least one trial"
    );
    assert!(incorrect <= attempted, "incorrect decisions exceed trials");
    assert!(
        (0.0..1.0).contains(&alpha),
        "alpha must be strictly between zero and one"
    );
    if incorrect == attempted {
        return 1.0;
    }
    let a = (incorrect + 1) as f64;
    let b = (attempted - incorrect) as f64;
    quantile(1.0 - alpha, a, b)
}

#[test]
#[allow(clippy::cast_precision_loss)] // loop bound, far below f64's exact-integer range
fn clopper_pearson_upper_matches_the_closed_form_for_zero_incorrect() {
    for attempted in [1_u64, 5, 50, 500, 5_000] {
        let alpha = 0.05_f64;
        let closed_form = 1.0 - alpha.powf(1.0 / attempted as f64);
        let computed = clopper_pearson_upper(0, attempted, alpha);
        assert!(
            (computed - closed_form).abs() < 1e-9,
            "attempted={attempted}: computed={computed}, closed_form={closed_form}"
        );
    }
    // Every trial incorrect: the bound is exactly 1.0 (an early return, not
    // an approximation), but `assert_eq!` on floats is flagged regardless.
    assert!(clopper_pearson_upper(3, 3, 0.05) >= 1.0);
}

// ---------------------------------------------------------------------
// Corpus loading (via the crate's own dependency-free JSON parser; no
// serde, no third-party crate reads its own evaluation fixtures).
// ---------------------------------------------------------------------

const DOCUMENTATION: &str = include_str!("../../../docs/MATCHING-QUALITY.md");

fn corpus_text(name: &str) -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/data/evaluation")
        .join(name);
    std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("reading {}: {error}", path.display()))
}

/// The cases of a corpus: a JSON array, or one JSON record per line for a
/// `.jsonl` corpus.
fn read_corpus(name: &str) -> Vec<Node> {
    let text = corpus_text(name);
    if Path::new(name)
        .extension()
        .is_some_and(|extension| extension == "jsonl")
    {
        return text
            .lines()
            .enumerate()
            .map(|(line, record)| {
                json::parse(record)
                    .unwrap_or_else(|error| panic!("parsing {name}:{}: {error}", line + 1))
            })
            .collect();
    }
    let root = json::parse(&text).unwrap_or_else(|error| panic!("parsing {name}: {error}"));
    match root.value {
        Value::Array(items) => items,
        other => panic!("{name}: expected a top-level array, found {other:?}"),
    }
}

fn field<'a>(node: &'a Node, key: &str) -> &'a Node {
    node.get(key)
        .unwrap_or_else(|| panic!("missing field {key:?} in {node:?}"))
}

fn text_field<'a>(node: &'a Node, key: &str) -> &'a str {
    field(node, key)
        .as_str()
        .unwrap_or_else(|| panic!("field {key:?} is not a string in {node:?}"))
}

fn array_field<'a>(node: &'a Node, key: &str) -> &'a [Node] {
    match &field(node, key).value {
        Value::Array(items) => items,
        other => panic!("field {key:?} is not an array, found {other:?}"),
    }
}

fn number_field(node: &Node, key: &str) -> u32 {
    match &field(node, key).value {
        Value::Number(number) => number
            .as_str()
            .parse()
            .unwrap_or_else(|error| panic!("field {key:?} is not a u32: {error}")),
        other => panic!("field {key:?} is not a number, found {other:?}"),
    }
}

fn optional_text<'a>(node: &'a Node, key: &str) -> Option<&'a str> {
    node.get(key).and_then(Node::as_str)
}

fn build_candidates(node: &Node) -> Vec<Candidate> {
    array_field(node, "candidates")
        .iter()
        .map(|entry| {
            let id = text_field(entry, "id");
            let text = text_field(entry, "text");
            let mut candidate = Candidate::new(id, text);
            if let Some(aliases) = entry.get("aliases")
                && let Value::Array(items) = &aliases.value
            {
                candidate.aliases = items
                    .iter()
                    .map(|alias| {
                        alias
                            .as_str()
                            .unwrap_or_else(|| panic!("alias is not a string"))
                            .to_string()
                    })
                    .collect();
            }
            candidate
        })
        .collect()
}

// ---------------------------------------------------------------------
// Outcome tallying: correct automatic decisions, incorrect automatic
// decisions (the invariant this whole file exists to keep at zero), and
// abstentions (`Ambiguous`/`Unresolved`, i.e. coverage this corpus does not
// claim). `should abstain` ground truth (`truth: null`) makes *any*
// automatic decision incorrect, regardless of which value it names.
// ---------------------------------------------------------------------

#[derive(Default, Clone, Copy, Debug)]
struct Tally {
    correct: u64,
    incorrect: u64,
    abstained: u64,
}
impl Tally {
    const fn attempted(self) -> u64 {
        self.correct + self.incorrect
    }
    const fn total(self) -> u64 {
        self.attempted() + self.abstained
    }
    const fn record(&mut self, matched: bool) {
        if matched {
            self.correct += 1;
        } else {
            self.incorrect += 1;
        }
    }
    const fn add(&mut self, other: Self) {
        self.correct += other.correct;
        self.incorrect += other.incorrect;
        self.abstained += other.abstained;
    }
}

fn string_corpus_tally(threshold: Option<u32>) -> (Tally, usize) {
    let cases = read_corpus("strings.json");
    let mut tally = Tally::default();
    for case in &cases {
        let query = text_field(case, "query");
        let truth = optional_text(case, "truth");
        let minimum_score = threshold.unwrap_or_else(|| number_field(case, "minimum_score"));
        let candidates = build_candidates(case);
        let effort = if text_field(case, "category") == "exact_effort" {
            Effort::Exact
        } else {
            Effort::Standard
        };
        let request = MatchRequest {
            effort,
            ..MatchRequest::default()
        };
        let policy = SelectionPolicy {
            minimum_score,
            ..SelectionPolicy::default()
        };
        let answer = matching::resolve(query, &candidates, &request, policy);
        match &answer.outcome {
            Outcome::Resolved(value) | Outcome::BestGuess(value) => {
                tally.record(truth == Some(value.candidate.id.as_str()));
            }
            Outcome::Ambiguous(_) | Outcome::Unresolved => tally.abstained += 1,
        }
    }
    (tally, cases.len())
}

fn phonetic_corpus_tally(threshold: Option<u32>) -> (Tally, usize) {
    let cases = read_corpus("phonetic.json");
    let mut tally = Tally::default();
    for case in &cases {
        let query = text_field(case, "query");
        let truth = optional_text(case, "truth");
        let minimum_score = threshold.unwrap_or_else(|| number_field(case, "minimum_score"));
        let candidates = build_candidates(case);
        let request = MatchRequest::default();
        let policy = SelectionPolicy {
            minimum_score,
            ..SelectionPolicy::default()
        };
        let answer = matching::resolve(query, &candidates, &request, policy);
        match &answer.outcome {
            Outcome::Resolved(value) | Outcome::BestGuess(value) => {
                tally.record(truth == Some(value.candidate.id.as_str()));
            }
            Outcome::Ambiguous(_) | Outcome::Unresolved => tally.abstained += 1,
        }
    }
    (tally, cases.len())
}

fn path_corpus_tally(threshold: Option<u32>) -> (Tally, usize) {
    let cases = read_corpus("paths.json");
    let mut tally = Tally::default();
    for case in &cases {
        let paths: Vec<String> = array_field(case, "paths")
            .iter()
            .map(|entry| {
                entry
                    .as_str()
                    .unwrap_or_else(|| panic!("path is not a string"))
                    .to_string()
            })
            .collect();
        let query = text_field(case, "query");
        let context = text_field(case, "context");
        let truth = optional_text(case, "truth");
        let mut governor = Governor::new(Budget::default());
        let index = PathIndex::new(paths, &mut governor)
            .unwrap_or_else(|error| panic!("indexing case {case:?}: {error:?}"));
        let request = MatchRequest::default();
        let result = index.resolve_with_policy(
            query,
            context,
            &request,
            purrassist_core::paths::SelectionPolicy {
                minimum_score: threshold.unwrap_or(7500),
                ..purrassist_core::paths::SelectionPolicy::default()
            },
        );
        match &result.outcome {
            Outcome::Resolved(value) | Outcome::BestGuess(value) => {
                tally.record(truth == Some(value.path.as_str()));
            }
            Outcome::Ambiguous(_) | Outcome::Unresolved => tally.abstained += 1,
        }
    }
    (tally, cases.len())
}

fn json_recovery_corpus_tally() -> (Tally, usize) {
    let cases = read_corpus("json_recovery.json");
    let mut tally = Tally::default();
    for case in &cases {
        let schema_text = field(case, "schema").to_json();
        let schema = Schema::compile(&schema_text)
            .unwrap_or_else(|error| panic!("compiling schema for {case:?}: {error:?}"));
        let input = text_field(case, "input");
        let truth = field(case, "truth");
        let result = schema.recover(input, RecoveryRequest::default());
        match &result.outcome {
            Outcome::Resolved(value) | Outcome::BestGuess(value) => {
                let matched = !matches!(truth.value, Value::Null) && value.equivalent(truth);
                tally.record(matched);
            }
            Outcome::Ambiguous(_) | Outcome::Unresolved => tally.abstained += 1,
        }
    }
    (tally, cases.len())
}

/// The tool-call mutation corpus (JSON Lines; see `tool_call_corpus`).
const TOOL_CALLS: &str = "tool-calls.jsonl";

/// The tool-call corpus evaluated: the overall tally, per category, per
/// printed dialect and per dialect allow-list, and the abstention recall
/// over truth-`Unresolved` cases.
struct ToolCallEvaluation {
    tally: Tally,
    total: usize,
    categories: BTreeMap<String, (Tally, u64)>,
    dialects: BTreeMap<String, Tally>,
    printed: BTreeMap<String, Tally>,
    unresolved_truth: u64,
    unresolved_abstained: u64,
}

/// The calls a correct answer recovers (canonical name and arguments, in
/// order), or `None` when the case must abstain.
fn tool_call_truth(case: &Node) -> Option<Vec<(String, Node)>> {
    let truth = field(case, "truth");
    if truth.as_str() == Some(tool_call_corpus::UNRESOLVED) {
        return None;
    }
    let Value::Array(calls) = &truth.value else {
        panic!("truth is `Unresolved` or an array of calls: {truth:?}")
    };
    Some(
        calls
            .iter()
            .map(|call| {
                (
                    text_field(call, "name").to_owned(),
                    field(call, "arguments").clone(),
                )
            })
            .collect(),
    )
}

/// Every case recovered under its request and scored. The answer is the
/// calls the caller may run when it accepts guesses
/// ([`ToolCalls::usable_accepting_guesses`]): `Resolved` calls and `BestGuess`
/// calls on read-only tools. A `BestGuess` on a mutating tool, an
/// `Ambiguous` or `Unresolved` call or turn, and a turn with no usable call
/// are abstentions. An answer is correct when its calls equal the truth
/// (tool, [`Node::equivalent`] arguments, order); any other answer,
/// including any answer to a truth-`Unresolved` case, is incorrect.
///
/// [`ToolCalls::usable_accepting_guesses`]: purrassist_core::tool_call::ToolCalls::usable_accepting_guesses
fn tool_call_corpus_tally() -> ToolCallEvaluation {
    let cases = read_corpus(TOOL_CALLS);
    let tools = tool_call_corpus::tools();
    let adapter = tool_call_corpus::adapter_tools();
    let mut evaluation = ToolCallEvaluation {
        tally: Tally::default(),
        total: cases.len(),
        categories: BTreeMap::new(),
        dialects: BTreeMap::new(),
        printed: BTreeMap::new(),
        unresolved_truth: 0,
        unresolved_abstained: 0,
    };
    for case in &cases {
        let request = tool_call_corpus::request(case);
        let set = tool_call_corpus::tools_for(case, &tools, &adapter);
        // A case whose output is not well-formed UTF-8 carries its exact
        // bytes as `input_hex`.
        let bytes: Vec<u8> = optional_text(case, "input_hex").map_or_else(
            || text_field(case, "input").as_bytes().to_vec(),
            |hex| {
                (0..hex.len())
                    .step_by(2)
                    .map(|at| u8::from_str_radix(&hex[at..at + 2], 16).expect("hex input"))
                    .collect()
            },
        );
        let result = set.recover(&bytes, &request);
        let answer: Option<Vec<(String, Node)>> = match &result.outcome {
            Outcome::Resolved(calls) | Outcome::BestGuess(calls) => {
                let usable = calls.usable_accepting_guesses();
                (!usable.is_empty()).then(|| {
                    usable
                        .iter()
                        .map(|call| {
                            (
                                call.tool.clone().expect("a usable call names its tool"),
                                call.arguments.clone(),
                            )
                        })
                        .collect()
                })
            }
            Outcome::Ambiguous(_) | Outcome::Unresolved => None,
        };
        let truth = tool_call_truth(case);
        let mut tally = Tally::default();
        match (&answer, &truth) {
            (None, _) => tally.abstained += 1,
            (Some(answer), Some(truth)) => tally.record(
                answer.len() == truth.len()
                    && answer
                        .iter()
                        .zip(truth)
                        .all(|((tool, arguments), (name, expected))| {
                            tool == name && arguments.equivalent(expected)
                        }),
            ),
            (Some(_), None) => tally.record(false),
        }
        if tally.incorrect > 0 {
            eprintln!(
                "incorrect: {} answered {:?}",
                text_field(case, "id"),
                answer.as_ref().map(|calls| calls
                    .iter()
                    .map(|(tool, arguments)| format!("{tool} {}", arguments.to_json()))
                    .collect::<Vec<_>>())
            );
        }
        if truth.is_none() {
            evaluation.unresolved_truth += 1;
            evaluation.unresolved_abstained += tally.abstained;
        }
        evaluation.tally.add(tally);
        let category = evaluation
            .categories
            .entry(text_field(case, "category").to_owned())
            .or_default();
        category.0.add(tally);
        category.1 += u64::from(truth.is_none());
        evaluation
            .dialects
            .entry(text_field(case, "dialect").to_owned())
            .or_default()
            .add(tally);
        // The dialect the case's calls are printed in: its identity's
        // first segment (`qwen`, `hermes`, `openai`, or `mixed`).
        let id = text_field(case, "id");
        evaluation
            .printed
            .entry(id[..id.find('/').expect("a dialect segment")].to_owned())
            .or_default()
            .add(tally);
    }
    evaluation
}

/// One corpus's full evaluation: zero incorrect corrections, the risk
/// (`1 - precision` among automatic decisions), the coverage (the fraction
/// of cases an automatic decision was reached for at all), and the exact
/// Clopper-Pearson 95% upper bound on the incorrect-correction rate.
#[allow(clippy::cast_precision_loss)] // corpus-sized counts, far below f64's exact-integer range
fn evaluate(name: &str, tally: Tally, total_cases: usize, limit: f64) {
    assert_eq!(
        tally.total(),
        total_cases as u64,
        "{name}: tally does not cover every case"
    );
    assert_eq!(
        tally.incorrect, 0,
        "{name}: incorrect automatic correction(s) observed"
    );
    let attempted = tally.attempted();
    let coverage = attempted as f64 / total_cases as f64;
    let risk = if attempted == 0 {
        0.0
    } else {
        tally.incorrect as f64 / attempted as f64
    };
    assert!(
        attempted > 0,
        "{name}: no decisions cannot establish a risk bound"
    );
    let bound = clopper_pearson_upper(tally.incorrect, attempted, 0.05);
    eprintln!(
        "{name}: {total_cases} cases; {} correct, {} incorrect, {} abstained; coverage={coverage:.4}; risk={risk:.6}; cp95_upper={bound:.6}; limit={limit:.6}",
        tally.correct, tally.incorrect, tally.abstained
    );
    let row = format!(
        "| {name} | {total_cases} | {} | {} | {} | {:.1}% | {:.1}% | {:.3}% | {:.0}% |",
        tally.correct,
        tally.incorrect,
        tally.abstained,
        coverage * 100.0,
        risk * 100.0,
        bound * 100.0,
        limit * 100.0
    );
    assert!(
        DOCUMENTATION.lines().any(|line| line == row),
        "documented evaluation differs from measured row: {row}"
    );
    assert!(
        bound <= limit,
        "{name}: Clopper-Pearson 95% upper bound {bound:.6} exceeds the documented limit {limit:.6}"
    );
}

/// The tool-call corpus tallies `docs/MATCHING-QUALITY.md` quotes.
const TOOL_CALL_CASES: usize = 16336;
const TOOL_CALL_CORRECT: u64 = 11236;
const TOOL_CALL_ABSTAINED: u64 = 5100;

// ---------------------------------------------------------------------
// Documented numbers: `docs/MATCHING-QUALITY.md`'s evaluation table quotes
// exactly these tallies and limits. A mismatch is a build failure by
// design, keeping the document truthful.
// ---------------------------------------------------------------------

#[test]
fn string_selection_evaluation_has_zero_incorrect_corrections() {
    let (tally, total) = string_corpus_tally(None);
    assert_eq!(
        total, 907,
        "docs/MATCHING-QUALITY.md quotes this case count"
    );
    assert_eq!(
        tally.correct, 539,
        "docs/MATCHING-QUALITY.md quotes this correct count"
    );
    assert_eq!(
        tally.abstained, 368,
        "docs/MATCHING-QUALITY.md quotes this abstained count"
    );
    evaluate("String selection", tally, total, 0.02);
}

#[test]
fn path_matching_evaluation_has_zero_incorrect_corrections() {
    let (tally, total) = path_corpus_tally(None);
    assert_eq!(
        total, 1594,
        "docs/MATCHING-QUALITY.md quotes this case count"
    );
    assert_eq!(
        tally.correct, 1279,
        "docs/MATCHING-QUALITY.md quotes this correct count"
    );
    assert_eq!(
        tally.abstained, 315,
        "docs/MATCHING-QUALITY.md quotes this abstained count"
    );
    evaluate("Path matching", tally, total, 0.02);
}

#[test]
fn phonetic_ambiguity_evaluation_has_zero_incorrect_corrections() {
    let (tally, total) = phonetic_corpus_tally(None);
    assert_eq!(
        total, 1408,
        "docs/MATCHING-QUALITY.md quotes this case count"
    );
    assert_eq!(
        tally.correct, 1089,
        "docs/MATCHING-QUALITY.md quotes this correct count"
    );
    assert_eq!(
        tally.abstained, 319,
        "docs/MATCHING-QUALITY.md quotes this abstained count"
    );
    evaluate("Phonetic ambiguity", tally, total, 0.05);
}

#[test]
fn json_recovery_evaluation_has_zero_incorrect_corrections() {
    let (tally, total) = json_recovery_corpus_tally();
    assert_eq!(
        total, 714,
        "docs/MATCHING-QUALITY.md quotes this case count"
    );
    assert_eq!(
        tally.correct, 630,
        "docs/MATCHING-QUALITY.md quotes this correct count"
    );
    assert_eq!(
        tally.abstained, 84,
        "docs/MATCHING-QUALITY.md quotes this abstained count"
    );
    evaluate("JSON recovery", tally, total, 0.02);
}

/// Every category of the tool-call corpus has at least this many cases.
const MINIMUM_CATEGORY_CASES: u64 = 20;

#[test]
#[allow(clippy::cast_precision_loss)] // corpus-sized counts, far below f64's exact-integer range
fn tool_call_recovery_evaluation_has_zero_incorrect_corrections() {
    let evaluation = tool_call_corpus_tally();
    let (tally, total) = (evaluation.tally, evaluation.total);
    // Abstention recall: the fraction of cases whose only correct answer is
    // abstention that abstained.
    let recall = format!(
        "Abstention recall: {} of {} truth-`Unresolved` cases abstained ({:.1}%).",
        evaluation.unresolved_abstained,
        evaluation.unresolved_truth,
        evaluation.unresolved_abstained as f64 * 100.0 / evaluation.unresolved_truth as f64
    );
    let categories: Vec<(String, Tally, String)> = evaluation
        .categories
        .iter()
        .map(|(category, (tally, unresolved))| {
            let row = format!(
                "| `{category}` | {} | {} | {} | {} | {unresolved} |",
                tally.total(),
                tally.correct,
                tally.incorrect,
                tally.abstained
            );
            (category.clone(), *tally, row)
        })
        .collect();
    let dialects: Vec<(String, Tally, String)> = evaluation
        .printed
        .iter()
        .chain(&evaluation.dialects)
        .map(|(dialect, tally)| {
            let row = format!(
                "| `{dialect}` | {} | {} | {} | {} |",
                tally.total(),
                tally.correct,
                tally.incorrect,
                tally.abstained
            );
            (dialect.clone(), *tally, row)
        })
        .collect();
    // Print everything before asserting, so one run shows every number.
    eprintln!("{recall}");
    for (_, _, row) in categories.iter().chain(&dialects) {
        eprintln!("{row}");
    }
    for (name, tally, _) in categories.iter().chain(&dialects) {
        assert_eq!(tally.incorrect, 0, "{name}: incorrect correction(s)");
    }
    assert_eq!(
        total, TOOL_CALL_CASES,
        "docs/MATCHING-QUALITY.md quotes this case count"
    );
    assert_eq!(
        tally.correct, TOOL_CALL_CORRECT,
        "docs/MATCHING-QUALITY.md quotes this correct count"
    );
    assert_eq!(
        tally.abstained, TOOL_CALL_ABSTAINED,
        "docs/MATCHING-QUALITY.md quotes this abstained count"
    );
    evaluate("Tool-call recovery", tally, total, 0.01);
    assert!(evaluation.unresolved_truth > 0);
    assert_eq!(
        evaluation.unresolved_abstained, evaluation.unresolved_truth,
        "abstention recall must be 100%"
    );
    assert!(
        DOCUMENTATION.lines().any(|line| line == recall),
        "documented abstention recall differs: {recall}"
    );
    for (category, tally, row) in &categories {
        assert!(
            tally.total() >= MINIMUM_CATEGORY_CASES,
            "{category}: only {} cases",
            tally.total()
        );
        assert!(
            DOCUMENTATION.lines().any(|line| line == row),
            "documented per-rule evaluation differs: {row}"
        );
    }
    for (_, _, row) in &dialects {
        assert!(
            DOCUMENTATION.lines().any(|line| line == row),
            "documented per-dialect evaluation differs: {row}"
        );
    }
}

/// The committed tool-call corpus is exactly what its generator produces
/// now, and `manifest.json` registers its digest and census.
#[test]
fn corpus_regenerates_identically() {
    let cases = tool_call_corpus::generate();
    let generated = tool_call_corpus::render(&cases);
    let committed = corpus_text(TOOL_CALLS);
    assert!(
        generated == committed,
        "tests/data/evaluation/{TOOL_CALLS} is stale: run `cargo run -p purrassist-core --example tool-call-corpus`"
    );
    let manifest = json::parse(&corpus_text("manifest.json")).expect("manifest");
    let entry = manifest
        .get("generators")
        .and_then(|generators| generators.get(TOOL_CALLS))
        .expect("the manifest registers the tool-call corpus");
    let digest = Digest::of(committed.as_bytes()).to_string();
    assert_eq!(
        Some(digest.trim_start_matches("sha256:")),
        entry.get("sha256").and_then(Node::as_str)
    );
    assert_eq!(number_field(entry, "cases") as usize, cases.len());
    assert_eq!(
        number_field(entry, "distinct_requests") as usize,
        tool_call_corpus::distinct_requests(&cases)
    );
}

#[test]
fn corpora_have_unique_identities_and_at_least_500_distinct_requests() {
    for corpus in [
        "strings.json",
        "paths.json",
        "phonetic.json",
        "json_recovery.json",
        TOOL_CALLS,
    ] {
        let cases = read_corpus(corpus);
        let identities: std::collections::BTreeSet<_> =
            cases.iter().map(|case| text_field(case, "id")).collect();
        assert_eq!(
            identities.len(),
            cases.len(),
            "duplicate case identity in {corpus}"
        );
        let requests: std::collections::BTreeSet<_> = cases
            .iter()
            .map(|case| {
                let mut case = case.clone();
                let Value::Object(fields) = &mut case.value else {
                    panic!("case must be an object")
                };
                fields.retain(|member| {
                    member.key != "id" && member.key != "category" && member.key != "truth"
                });
                case.to_json()
            })
            .collect();
        assert!(
            requests.len() >= 500,
            "{corpus}: only {} distinct requests",
            requests.len()
        );
    }
}

#[test]
fn risk_coverage_sweeps_have_zero_incorrect_automatic_decisions() {
    for (name, evaluate_corpus, thresholds) in [
        (
            "String selection",
            string_corpus_tally as fn(Option<u32>) -> (Tally, usize),
            [7500, 9000, 9650, 10000],
        ),
        (
            "Path matching",
            path_corpus_tally,
            [7500, 9000, 9650, 10000],
        ),
        (
            "Phonetic ambiguity",
            phonetic_corpus_tally,
            [6000, 7500, 9000, 10000],
        ),
    ] {
        let mut previous_attempted = u64::MAX;
        for threshold in thresholds {
            let (tally, total) = evaluate_corpus(Some(threshold));
            assert_eq!(
                tally.incorrect, 0,
                "{name} at threshold {threshold}: {tally:?}"
            );
            assert!(
                tally.attempted() <= previous_attempted,
                "raising score threshold increased coverage"
            );
            previous_attempted = tally.attempted();
            let row = format!(
                "| {name} | {threshold} | {} | {} | {} |",
                tally.correct, tally.incorrect, tally.abstained
            );
            eprintln!("{row} total={total}");
            assert!(
                DOCUMENTATION.lines().any(|line| line == row),
                "documented risk–coverage differs: {row}"
            );
        }
    }
}

#[test]
fn nonzero_error_bound_inverts_independent_binomial_probability() {
    let bound = clopper_pearson_upper(1, 10, 0.05);
    let below_two_errors = (10.0 * bound).mul_add((1.0 - bound).powi(9), (1.0 - bound).powi(10));
    assert!((below_two_errors - 0.05).abs() < 1e-9);
    assert!((clopper_pearson_upper(9, 10, 0.05) - 0.95_f64.powf(0.1)).abs() < 1e-9);
}
