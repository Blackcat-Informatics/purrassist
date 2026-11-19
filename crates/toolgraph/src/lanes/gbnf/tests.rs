// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0

//! Behaviour of the GBNF lane: accepted strings validate, losses are ledgered,
//! unsupported constructs are refused and output is deterministic.
//!
//! The sampler below reads the emitted grammar text itself, so a test exercises
//! the language the grammar defines and not the compiler's internal state.

use std::{
    collections::{BTreeMap, BTreeSet},
    fmt::Write as _,
};

use purrassist_core::{
    Budget, Effort, Governor, StopReason, contract::Resource, json, schema::Schema,
};
use purrdf_core::loss::LossProfile;

use super::{Builder, GbnfError};
use crate::{
    PrefixTable, TERM_NAMES, TermBindings, ToolGraph, Vocabulary, codec::mcp, lanes,
    register_loss_profile,
};

const NAMESPACE: &str = "https://example.org/toolgraph#";
const TOOLS: &str = include_str!("../../../../../docs/tool-graph/TOOLS.md");

enum Expr {
    Literal(String),
    Class(bool, Vec<(u32, u32)>),
    Rule(String),
    Sequence(Vec<Self>),
    Choice(Vec<Self>),
    Repeat(Box<Self>, usize, Option<usize>),
}

/// A recursive-descent reader for the subset of GBNF the compiler emits.
struct Reader<'a> {
    chars: std::iter::Peekable<std::str::Chars<'a>>,
}

impl Reader<'_> {
    fn hex(&mut self, digits: usize) -> char {
        let hex: String = (0..digits)
            .map(|_| self.chars.next().expect("hex"))
            .collect();
        char::from_u32(u32::from_str_radix(&hex, 16).expect("hex")).expect("char")
    }

    fn escaped(&mut self) -> char {
        match self.chars.next().expect("escape") {
            'n' => '\n',
            'r' => '\r',
            't' => '\t',
            'x' => self.hex(2),
            'u' => self.hex(4),
            'U' => self.hex(8),
            other => other,
        }
    }

    fn choice(&mut self) -> Expr {
        let mut branches = vec![self.sequence()];
        while self.chars.peek() == Some(&'|') {
            self.chars.next();
            branches.push(self.sequence());
        }
        Expr::Choice(branches)
    }

    fn sequence(&mut self) -> Expr {
        let mut items = Vec::new();
        loop {
            while self.chars.peek() == Some(&' ') {
                self.chars.next();
            }
            if matches!(self.chars.peek(), None | Some('|' | ')')) {
                return Expr::Sequence(items);
            }
            let mut atom = self.atom();
            while let Some(&mark @ ('*' | '+' | '?')) = self.chars.peek() {
                self.chars.next();
                let (low, high) = match mark {
                    '*' => (0, None),
                    '+' => (1, None),
                    _ => (0, Some(1)),
                };
                atom = Expr::Repeat(Box::new(atom), low, high);
            }
            items.push(atom);
        }
    }

    fn atom(&mut self) -> Expr {
        match self.chars.next().expect("atom") {
            '"' => {
                let mut text = String::new();
                loop {
                    match self.chars.next().expect("closed literal") {
                        '"' => return Expr::Literal(text),
                        '\\' => text.push(self.escaped()),
                        other => text.push(other),
                    }
                }
            }
            '[' => {
                let negated = self.chars.peek() == Some(&'^');
                if negated {
                    self.chars.next();
                }
                let mut ranges = Vec::new();
                loop {
                    let low = match self.chars.next().expect("closed class") {
                        ']' => return Expr::Class(negated, ranges),
                        '\\' => self.escaped(),
                        other => other,
                    };
                    let mut ahead = self.chars.clone();
                    let high = if ahead.next() == Some('-') && ahead.peek() != Some(&']') {
                        self.chars.next();
                        match self.chars.next().expect("range end") {
                            '\\' => self.escaped(),
                            other => other,
                        }
                    } else {
                        low
                    };
                    ranges.push((u32::from(low), u32::from(high)));
                }
            }
            '(' => {
                let inner = self.choice();
                assert_eq!(self.chars.next(), Some(')'));
                inner
            }
            first => {
                let mut name = first.to_string();
                while let Some(&next) = self.chars.peek() {
                    if next.is_ascii_alphanumeric() || next == '-' {
                        name.push(next);
                        self.chars.next();
                    } else {
                        break;
                    }
                }
                Expr::Rule(name)
            }
        }
    }
}

fn read(grammar: &str) -> BTreeMap<String, Expr> {
    grammar
        .lines()
        .map(|line| {
            let (name, body) = line.split_once(" ::= ").expect("rule line");
            (
                name.to_owned(),
                Reader {
                    chars: body.chars().peekable(),
                }
                .choice(),
            )
        })
        .collect()
}

struct Sampler<'a> {
    rules: &'a BTreeMap<String, Expr>,
    state: u64,
    /// The fewest characters each rule can still generate, so a long walk can finish.
    costs: BTreeMap<String, usize>,
}

fn cost(expr: &Expr, costs: &BTreeMap<String, usize>) -> usize {
    match expr {
        Expr::Literal(text) => text.chars().count(),
        Expr::Class(..) => 1,
        Expr::Rule(name) => costs.get(name).copied().unwrap_or(usize::MAX / 4),
        Expr::Sequence(items) => items.iter().fold(0, |total, item| {
            usize::saturating_add(total, cost(item, costs))
        }),
        Expr::Choice(branches) => branches
            .iter()
            .map(|branch| cost(branch, costs))
            .min()
            .unwrap_or(0),
        Expr::Repeat(inner, low, _) => low.saturating_mul(cost(inner, costs)),
    }
}

impl<'a> Sampler<'a> {
    fn new(rules: &'a BTreeMap<String, Expr>) -> Self {
        let mut costs: BTreeMap<String, usize> = BTreeMap::new();
        loop {
            let next: BTreeMap<String, usize> = rules
                .iter()
                .map(|(name, expr)| (name.clone(), cost(expr, &costs)))
                .collect();
            if next == costs {
                break;
            }
            costs = next;
        }
        Self {
            rules,
            state: 0x9E37_79B9_7F4A_7C15,
            costs,
        }
    }

    fn next(&mut self, bound: usize) -> usize {
        self.state ^= self.state << 13;
        self.state ^= self.state >> 7;
        self.state ^= self.state << 17;
        usize::try_from(self.state >> 11).expect("fits") % bound
    }

    fn emit(&mut self, expr: &Expr, output: &mut String) {
        match expr {
            Expr::Literal(text) => output.push_str(text),
            Expr::Rule(name) => self.emit(self.rules.get(name).expect("defined rule"), output),
            Expr::Sequence(items) => items.iter().for_each(|item| self.emit(item, output)),
            Expr::Choice(branches) => {
                // Past a long output, take the branch that ends soonest.
                let pick = if output.len() > 64 {
                    (0..branches.len())
                        .min_by_key(|&index| cost(&branches[index], &self.costs))
                        .expect("a branch")
                } else {
                    self.next(branches.len())
                };
                self.emit(&branches[pick], output);
            }
            Expr::Repeat(inner, low, high) => {
                let span = high.map_or(3, |high| high - low);
                for _ in 0..low + self.next(span + 1) {
                    self.emit(inner, output);
                }
            }
            Expr::Class(false, ranges) => {
                let (low, high) = ranges[self.next(ranges.len())];
                let span = usize::try_from(high - low).expect("small") + 1;
                let code = low + u32::try_from(self.next(span)).expect("small");
                output.push(char::from_u32(code).expect("scalar"));
            }
            Expr::Class(true, ranges) => {
                let pool = ['a', 'Z', ' ', '/', 'é', '日', '😀', '{'];
                let allowed: Vec<char> = pool
                    .into_iter()
                    .filter(|c| {
                        !ranges
                            .iter()
                            .any(|(l, h)| (*l..=*h).contains(&u32::from(*c)))
                    })
                    .collect();
                output.push(allowed[self.next(allowed.len())]);
            }
        }
    }

    fn sample(&mut self) -> String {
        let mut output = String::new();
        self.emit(self.rules.get("root").expect("root rule"), &mut output);
        output
    }
}

/// The positions at which `expr` can stop after starting at `from` in `text`.
fn ends(
    rules: &BTreeMap<String, Expr>,
    expr: &Expr,
    text: &[char],
    from: usize,
) -> BTreeSet<usize> {
    match expr {
        Expr::Literal(literal) => {
            let length = literal.chars().count();
            if text
                .get(from..from + length)
                .is_some_and(|slice| slice.iter().copied().eq(literal.chars()))
            {
                BTreeSet::from([from + length])
            } else {
                BTreeSet::new()
            }
        }
        Expr::Class(negated, ranges) => text
            .get(from)
            .filter(|c| {
                ranges
                    .iter()
                    .any(|(low, high)| (*low..=*high).contains(&u32::from(**c)))
                    != *negated
            })
            .map_or_else(BTreeSet::new, |_| BTreeSet::from([from + 1])),
        Expr::Rule(name) => ends(rules, rules.get(name).expect("defined rule"), text, from),
        Expr::Sequence(items) => items.iter().fold(BTreeSet::from([from]), |at, item| {
            at.iter()
                .flat_map(|&position| ends(rules, item, text, position))
                .collect()
        }),
        Expr::Choice(branches) => branches
            .iter()
            .flat_map(|branch| ends(rules, branch, text, from))
            .collect(),
        Expr::Repeat(inner, low, high) => {
            let (mut results, mut seen) = (BTreeSet::<usize>::new(), BTreeSet::<usize>::new());
            let mut frontier = BTreeSet::from([from]);
            for count in 0.. {
                if count >= *low {
                    results.extend(&frontier);
                }
                if *high == Some(count) || frontier.is_empty() {
                    break;
                }
                let mut next: BTreeSet<usize> = frontier
                    .iter()
                    .flat_map(|&position| ends(rules, inner, text, position))
                    .collect();
                if count >= *low {
                    seen.extend(&frontier);
                    next.retain(|position| !seen.contains(position));
                }
                frontier = next;
            }
            results
        }
    }
}

/// Whether the language of `grammar` contains all of `text`.
fn accepts(grammar: &str, text: &str) -> bool {
    let rules = read(grammar);
    let characters: Vec<char> = text.chars().collect();
    ends(
        &rules,
        rules.get("root").expect("root rule"),
        &characters,
        0,
    )
    .contains(&characters.len())
}

fn compile(schema: &str) -> (String, Vec<String>) {
    try_compile(schema).expect("schema lowers")
}

fn try_compile(schema: &str) -> Result<(String, Vec<String>), GbnfError> {
    let mut governor = Governor::new(Effort::Deep.budget());
    let node = json::parse_with(schema, &mut governor).expect("schema JSON");
    let mut builder = Builder::new("t");
    let text = builder.grammar(&node, &mut governor)?;
    Ok((text, builder.omitted))
}

/// Sample `count` strings of `grammar` and return those that fail `schema`,
/// each with the keywords that failed. A string the JSON reader rejects is a
/// lone surrogate escape, the documented `json-string-validity` loss.
fn rejected(schema: &str, grammar: &str, count: usize) -> Vec<(String, Vec<String>)> {
    let schema = Schema::compile(schema).expect("compiled schema");
    let rules = read(grammar);
    let mut sampler = Sampler::new(&rules);
    let mut rejected = Vec::new();
    for _ in 0..count {
        let text = sampler.sample();
        match json::parse(&text) {
            Ok(instance) => {
                let report = schema.validate(&instance).expect("validation");
                if !report.valid {
                    let keywords = report.violations.into_iter().map(|v| v.keyword).collect();
                    rejected.push((text, keywords));
                }
            }
            Err(_) => assert!(
                text.to_ascii_lowercase().contains("\\ud"),
                "unparseable and not a surrogate escape: {text}"
            ),
        }
    }
    rejected
}

// Invariant: every string the grammar accepts validates against the schema it was
// compiled from, across scalars, enums, arrays, nested objects with optional
// members, type arrays, anyOf and disjoint oneOf.
#[test]
fn every_accepted_string_validates_against_its_schema() {
    let schemas = [
        r#"{"type":"object","properties":{"s":{"type":"string"},"i":{"type":"integer"},"n":{"type":"number"},"b":{"type":"boolean"},"z":{"type":"null"}},"required":["s","i"],"additionalProperties":false}"#,
        r#"{"type":"object","properties":{"a":{"type":"string"},"b":{"type":"integer"},"c":{"type":"boolean"}},"additionalProperties":false}"#,
        r#"{"type":"object","properties":{"mode":{"enum":["r","w\"x"]},"fixed":{"const":7},"flag":{"type":"string","enum":["a"]}},"required":["mode","fixed"],"additionalProperties":false}"#,
        r#"{"type":"object","properties":{"rows":{"type":"array","items":{"type":"object","properties":{"k":{"type":["string","null"]},"v":{"anyOf":[{"type":"integer"},{"type":"string"}]}},"required":["k"],"additionalProperties":false}},"pick":{"oneOf":[{"type":"string"},{"type":"integer"},{"type":"null"}]}},"required":["rows"],"additionalProperties":false}"#,
        r#"{"type":"object","additionalProperties":false}"#,
    ];
    for schema in schemas {
        let (grammar, omitted) = compile(schema);
        assert_eq!(omitted, Vec::<String>::new(), "{schema}");
        assert_eq!(rejected(schema, &grammar, 300), [], "{schema}\n{grammar}");
    }
}

/// The worked example's Turtle before any test variation.
fn worked_turtle() -> &'static str {
    TOOLS
        .split("## End-to-end example")
        .nth(1)
        .and_then(|section| section.split("```turtle\n").nth(1))
        .and_then(|fence| fence.split("\n```").next())
        .expect("worked Turtle fence")
}

/// The worked example's vocabulary and graph from the supplied Turtle.
fn load_example(turtle: &str) -> (Vocabulary, ToolGraph) {
    let vocabulary = Vocabulary::new(
        "pa",
        PrefixTable(vec![("pa".into(), NAMESPACE.into())]),
        TermBindings {
            namespace_iri: NAMESPACE.into(),
            local_names: TERM_NAMES
                .iter()
                .map(|name| ((*name).into(), format!("{NAMESPACE}{name}")))
                .collect(),
        },
        register_loss_profile(LossProfile::builtin()).expect("loss profile"),
    )
    .expect("vocabulary");
    let mut governor = Governor::new(Budget::default());
    let graph = ToolGraph::load(
        turtle.as_bytes(),
        "text/turtle",
        None,
        &vocabulary,
        &mut governor,
    )
    .expect("graph loads");
    (vocabulary, graph)
}

// Invariant: every string a worked-example grammar accepts validates against the
// schema it came from, including the customer pattern and the limit bounds, the
// output is byte-identical across runs, and the only ledgered losses are the
// surrogate escapes and the output shape.
#[test]
fn the_worked_example_grammars_accept_only_schema_valid_strings() {
    let (vocabulary, graph) = load_example(worked_turtle());
    let mut governor = Governor::new(Budget::default());
    let (grammars, ledger) = lanes::gbnf(&graph, &vocabulary, &mut governor).expect("gbnf lane");
    let (again, _) = lanes::gbnf(&graph, &vocabulary, &mut governor).expect("gbnf lane");
    assert_eq!(grammars, again);
    assert_eq!(
        grammars
            .iter()
            .map(lanes::ToolGrammar::tool)
            .collect::<Vec<_>>(),
        ["edit_file", "query_customer_orders"]
    );

    let (manifest, _) = mcp::encode(&graph, &vocabulary, &mut governor).expect("manifest");
    let root = json::parse(manifest.as_str()).expect("manifest JSON");
    let json::Value::Array(declarations) = &root.get("tools").expect("tools").value else {
        panic!("tools array");
    };
    for grammar in &grammars {
        let declaration = declarations
            .iter()
            .find(|d| d.get("name").and_then(json::Node::as_str) == Some(grammar.tool()))
            .expect("declaration");
        let schema = declaration
            .get("inputSchema")
            .expect("input schema")
            .to_json();
        assert_eq!(
            rejected(&schema, grammar.as_str(), 200),
            [],
            "{}",
            grammar.tool()
        );
    }
    let codes = |code: &str| ledger.entries().iter().filter(|e| e.code == code).count();
    assert_eq!(codes("json-string-validity-not-expressible"), 1);
    assert_eq!(codes("output-shape-not-expressible"), 2);
    assert_eq!(codes("input-shape-not-expressible"), 0);
}

/// The public lane enforces SHACL property counts and each item's pattern
/// against the same schema exposed through the MCP Extended manifest.
#[test]
fn shacl_array_counts_and_item_patterns_reach_the_gbnf_grammar() {
    let turtle = worked_turtle().replacen(
        r#"sh:pattern "^customer-[0-9]+$" ; sh:minCount 1 ; sh:maxCount 1"#,
        r#"sh:pattern "^customer-[0-9]+$" ; sh:minCount 2 ; sh:maxCount 3"#,
        1,
    );
    let (vocabulary, graph) = load_example(&turtle);
    let mut governor = Governor::new(Budget::default());
    let (grammars, ledger) = lanes::gbnf(&graph, &vocabulary, &mut governor).expect("gbnf lane");
    let grammar = grammars
        .iter()
        .find(|grammar| grammar.tool() == "query_customer_orders")
        .expect("query grammar");
    let (manifest, _) = mcp::encode(&graph, &vocabulary, &mut governor).expect("manifest");
    let root = json::parse(manifest.as_str()).expect("manifest JSON");
    let json::Value::Array(declarations) = &root.get("tools").expect("tools").value else {
        panic!("tools array");
    };
    let declaration = declarations
        .iter()
        .find(|declaration| {
            declaration.get("name").and_then(json::Node::as_str) == Some(grammar.tool())
        })
        .expect("query declaration");
    let schema = Schema::compile(
        &declaration
            .get("inputSchema")
            .expect("input schema")
            .to_json(),
    )
    .expect("compiled input schema");
    for (items, expected) in [
        (r#"["customer-1"]"#, false),
        (r#"["customer-1","customer-2"]"#, true),
        (r#"["customer-1","customer-2","customer-3"]"#, true),
        (
            r#"["customer-1","customer-2","customer-3","customer-4"]"#,
            false,
        ),
        (r#"["customer-1","wrong"]"#, false),
    ] {
        let text = format!(r#"{{"customer_id":{items},"limit":1}}"#);
        assert_eq!(accepts(grammar.as_str(), &text), expected, "{text}");
        let instance = json::parse(&text).expect("instance JSON");
        assert_eq!(
            schema.validate(&instance).expect("validation").valid,
            expected
        );
    }
    assert!(
        !ledger
            .entries()
            .iter()
            .any(|entry| entry.code == "input-shape-not-expressible")
    );
}

// Invariant: required members come first in property order, optional members
// follow with their own separators, and the text is the stable canonical form.
#[test]
fn object_members_use_the_canonical_order_and_text() {
    let (grammar, omitted) = compile(
        r#"{"type":"object","properties":{"b":{"type":"integer"},"a":{"type":"boolean"},"c":{"type":"null"}},"required":["a","b"],"additionalProperties":false}"#,
    );
    assert_eq!(omitted, Vec::<String>::new());
    assert_eq!(
        grammar,
        concat!(
            "root ::= object-1\n",
            "object-1 ::= \"{\" ws member-2 ws \",\" ws member-3 (ws \",\" ws member-4)? ws \"}\"\n",
            "member-2 ::= \"\\\"b\\\"\" ws \":\" ws integer\n",
            "member-3 ::= \"\\\"a\\\"\" ws \":\" ws (\"true\" | \"false\")\n",
            "member-4 ::= \"\\\"c\\\"\" ws \":\" ws \"null\"\n",
            "ws ::= [ \\t\\n\\r]*\n",
            "integer ::= \"-\"? (\"0\" | [1-9] [0-9]*)\n",
        )
    );
}

// Invariant: a keyword the grammar does not carry and a member outside
// `properties` are recorded at their pointers, not silently dropped.
#[test]
fn uncarried_keywords_and_open_members_are_ledgered_at_their_pointers() {
    let (_, omitted) = compile(
        r#"{"type":"object","properties":{"s":{"type":"string","format":"email"},"i":{"type":"integer","default":1}},"required":["s"]}"#,
    );
    assert_eq!(
        omitted,
        [
            "/additionalProperties (members outside properties are not generated)",
            "/properties/s/format",
            "/properties/i/default",
        ]
    );
}

// Invariant: a construct that cannot be lowered soundly is a typed refusal and
// yields no grammar.
#[test]
fn inexpressible_constructs_are_refused_with_a_typed_error() {
    let refused = [
        r##"{"type":"object","properties":{"r":{"$ref":"#/$defs/r"}}}"##,
        r#"{"type":"object","properties":{"a":{"type":"array"}}}"#,
        r#"{"type":"object","properties":{"o":{"oneOf":[{"type":"number"},{"type":"integer"}]}}}"#,
        r#"{"type":"object","properties":{"e":{"type":"string","enum":[1]}}}"#,
        r#"{"type":"object","properties":{"e":{"enum":[{"x":1}]}}}"#,
        r#"{"type":"object","required":["ghost"]}"#,
        r#"{"type":"object","properties":{"t":true}}"#,
        r#"{"type":"array","items":{"type":"string"}}"#,
    ];
    for schema in refused {
        assert!(
            matches!(try_compile(schema), Err(GbnfError::Unsupported { .. })),
            "{schema}"
        );
    }
}

/// `text` as a JSON string literal, as the schema or an instance spells it.
fn quoted(text: &str) -> String {
    let mut output = String::from('"');
    for character in text.chars() {
        match character {
            '"' => output.push_str("\\\""),
            '\\' => output.push_str("\\\\"),
            '\n' => output.push_str("\\n"),
            '\t' => output.push_str("\\t"),
            '\r' => output.push_str("\\r"),
            '\u{0C}' => output.push_str("\\f"),
            control if control < ' ' => {
                let _ = write!(output, "\\u{:04x}", u32::from(control));
            }
            other => output.push(other),
        }
    }
    output.push('"');
    output
}

/// The input schema with one member `v` constrained by `keywords`.
fn member_schema(keywords: &str) -> String {
    format!(
        r#"{{"type":"object","properties":{{"v":{{{keywords}}}}},"required":["v"],"additionalProperties":false}}"#
    )
}

/// Each `(value, expected)` pair: the grammar accepts the serialization of the
/// object holding `value` exactly when `expected`, and the schema agrees.
fn assert_member_cases(keywords: &str, cases: &[(String, bool)]) {
    let schema = member_schema(keywords);
    let (grammar, omitted) = compile(&schema);
    assert_eq!(omitted, Vec::<String>::new(), "{keywords}");
    let compiled = Schema::compile(&schema).expect("compiled schema");
    for (value, expected) in cases {
        let text = format!("{{\"v\":{value}}}");
        assert_eq!(
            accepts(&grammar, &text),
            *expected,
            "grammar: {keywords}: {text}"
        );
        let instance = json::parse(&text).expect("instance JSON");
        assert_eq!(
            compiled.validate(&instance).expect("validation").valid,
            *expected,
            "schema: {keywords}: {text}"
        );
    }
    assert_eq!(
        rejected(&schema, &grammar, 300),
        [],
        "{keywords}\n{grammar}"
    );
}

/// Arrays accept all and only their allowed lengths, including both sides of
/// a mixed-bit interval, and branch-local counts survive a union.
#[test]
fn array_count_bounds_match_schema_validation() {
    let item = r#""type":"array","items":{"type":"integer"}"#;
    for (bounds, minimum, maximum) in [
        (r#""minItems":3e0"#, 3, 8),
        (r#""maxItems":0"#, 0, 0),
        (r#""maxItems":4.0"#, 0, 4),
        (r#""minItems":2,"maxItems":7"#, 2, 7),
        (r#""minItems":5,"maxItems":5"#, 5, 5),
    ] {
        let cases: Vec<_> = (0..=8)
            .map(|length| {
                let items = vec!["1"; length].join(",");
                (format!("[{items}]"), (minimum..=maximum).contains(&length))
            })
            .collect();
        assert_member_cases(&format!("{item},{bounds}"), &cases);
    }
    assert_member_cases(
        &format!(r#""anyOf":[{{{item},"minItems":2,"maxItems":2}},{{"type":"null"}}]"#),
        &[
            ("[1]".to_owned(), false),
            ("[1,2]".to_owned(), true),
            ("[1,2,3]".to_owned(), false),
            ("null".to_owned(), true),
        ],
    );
    assert_member_cases(
        r#""type":"array","maxItems":0"#,
        &[("[]".to_owned(), true), ("[1]".to_owned(), false)],
    );
}

/// Huge admitted bounds stay compact and out-of-domain or invalid bounds
/// give a typed refusal at the count keyword.
#[test]
fn array_count_expansion_is_bounded_and_names_the_refused_keyword() {
    let item = r#""type":"array","items":{"type":"integer"}"#;
    let schema = member_schema(&format!(r#"{item},"maxItems":18446744073709551615"#));
    let (grammar, omitted) = compile(&schema);
    assert_eq!(omitted.len(), 0);
    assert!(grammar.len() < 24_000);
    assert!(accepts(&grammar, r#"{"v":[1]}"#));
    let schema = json::parse(&schema).expect("array schema");
    let mut governor = Governor::new(Budget {
        scratch_bytes: 4_096,
        ..Budget::default()
    });
    assert!(matches!(
        Builder::new("test").grammar(&schema, &mut governor),
        Err(GbnfError::Resource(StopReason::BudgetExceeded(
            Resource::ScratchBytes
        )))
    ));

    for (bounds, keyword, expected) in [
        (
            r#""minItems":2,"maxItems":1"#,
            "maxItems",
            "maxItems is less than minItems",
        ),
        (
            r#""minItems":false"#,
            "minItems",
            "an item count must be a nonnegative integer",
        ),
        (
            r#""minItems":-1"#,
            "minItems",
            "an item count must be a nonnegative integer",
        ),
        (
            r#""maxItems":1.5"#,
            "maxItems",
            "an item count must be a nonnegative integer",
        ),
        (
            r#""maxItems":18446744073709551616"#,
            "maxItems",
            "an item count exceeds the 64-bit grammar bound",
        ),
    ] {
        let result = try_compile(&member_schema(&format!("{item},{bounds}")));
        let Err(GbnfError::Unsupported {
            pointer, reason, ..
        }) = result
        else {
            panic!("{bounds}: {result:?}");
        };
        assert_eq!(pointer, format!("/properties/v/{keyword}"));
        assert_eq!(reason, expected);
    }
}

/// Cases of a string-valued pattern: each decoded string and whether it matches.
fn assert_pattern(pattern: &str, cases: &[(&str, bool)]) {
    let keywords = format!(r#""type":"string","pattern":{}"#, quoted(pattern));
    let cases: Vec<(String, bool)> = cases
        .iter()
        .map(|(value, expected)| (quoted(value), *expected))
        .collect();
    assert_member_cases(&keywords, &cases);
}

// Invariant: literal characters and every escape that denotes one character
// (`\.`, `\/`, `\xHH`, `\uHHHH`, `\u{...}` over an astral scalar, `\n`, `\t`, `\v`,
// `\f`, `\r`, `\0`, `\cX`) are translated exactly, and JSON-escaped controls round-trip.
#[test]
fn pattern_literals_and_character_escapes_are_translated_exactly() {
    assert_pattern(
        r"^a\.\x41B\u{1F600}\n\t\/$",
        &[
            ("a.AB\u{1F600}\n\t/", true),
            ("axAB\u{1F600}\n\t/", false),
            ("a.AB\u{1F600}\n\t", false),
        ],
    );
    assert_pattern(
        r"^\0\v\f\r\cA$",
        &[
            ("\0\u{0B}\u{0C}\r\u{01}", true),
            ("\0\u{0B}\u{0C}\r\u{02}", false),
        ],
    );
}

// Invariant: classes, negated classes, ranges and the class escapes denote
// exactly their code points.
#[test]
fn pattern_classes_and_class_escapes_are_translated_exactly() {
    assert_pattern(
        r"^[a-c\d_]+[^x-z]$",
        &[
            ("ab1_q", true),
            ("ab1x", false),
            ("dq", false),
            ("a\"", true),
        ],
    );
    assert_pattern(
        r"^\d\D\w\W\s\S$",
        &[("1a_ \tx", true), ("a1_ \tx", false), ("1a_ \t ", false)],
    );
}

// Invariant: `\p{...}` and `\P{...}` denote their Unicode table sets, over
// scalars outside the Basic Multilingual Plane as well.
#[test]
fn pattern_unicode_properties_are_translated_exactly() {
    assert_pattern(
        r"^\p{Lu}\P{L}\p{Script=Greek}\p{Emoji_Presentation}$",
        &[
            ("A1\u{3B1}\u{1F600}", true),
            ("a1\u{3B1}\u{1F600}", false),
            ("AA\u{3B1}\u{1F600}", false),
        ],
    );
}

// Invariant: the dot is every scalar except the line terminators, so an
// astral scalar is one character under the Unicode reading.
#[test]
fn pattern_dot_excludes_only_line_terminators() {
    assert_pattern(
        r"^.$",
        &[
            ("x", true),
            ("\u{1F600}", true),
            ("\"", true),
            ("\n", false),
            ("\r", false),
            ("\u{2028}", false),
            ("xy", false),
        ],
    );
}

// Invariant: alternation, groups, non-capturing groups and named groups accept
// the union of their branches.
#[test]
fn pattern_alternation_and_groups_are_translated_exactly() {
    assert_pattern(
        r"^(?:ab|cd)(?<n>e|f)(g)$",
        &[
            ("abeg", true),
            ("cdfg", true),
            ("abgg", false),
            ("abe", false),
        ],
    );
}

// Invariant: every quantifier, greedy or lazy, accepts the same whole strings.
#[test]
fn pattern_quantifiers_are_translated_exactly() {
    assert_pattern(
        r"^a*b+c?d{2}e{2,}f{1,3}?$",
        &[
            ("bddeef", true),
            ("aabbcddeeeff", true),
            ("bdeef", false),
            ("bddef", false),
            ("bddeeffff", false),
            ("ddeef", false),
        ],
    );
    assert_pattern(r"^a+?b*?$", &[("aab", true), ("b", false), ("a", true)]);
}

// Invariant: a pattern is not anchored, so its grammar is any prefix, the
// pattern and any suffix, and the string need only contain a match.
#[test]
fn an_unanchored_pattern_accepts_a_longer_string() {
    assert_pattern(
        r"ab+c",
        &[
            ("xxabbcyy", true),
            ("abc", true),
            ("acb", false),
            ("", false),
        ],
    );
}

// Invariant: `^` or `$` alone pins only the end where it is written.
#[test]
fn a_pattern_anchored_at_one_end_pins_only_that_end() {
    assert_pattern(r"^ab", &[("abzzz", true), ("zab", false)]);
    assert_pattern(r"ab$", &[("zzab", true), ("abz", false)]);
}

// Invariant: an anchor inside one alternative pins only that alternative.
#[test]
fn an_anchor_inside_an_alternative_pins_only_that_alternative() {
    assert_pattern(
        r"^a|b$",
        &[("azz", true), ("zzb", true), ("zaz", false), ("bz", false)],
    );
}

// Invariant: `\b` and `\B` test the word-ness of the adjacent characters, so a
// whole-string grammar splits each rule by its neighbour's class.
#[test]
fn pattern_word_boundaries_are_translated_exactly() {
    assert_pattern(
        r"\bcat\b",
        &[
            ("a cat!", true),
            ("cat", true),
            ("concatenate", false),
            ("cats", false),
        ],
    );
    assert_pattern(r"\Bat", &[("cat", true), ("at", false), (" at", false)]);
}

// Invariant: an identity escape of a character that is not a syntax character
// is invalid under the Unicode reading and is taken as that literal.
#[test]
fn an_escaped_underscore_is_read_as_the_literal_character() {
    let (grammar, omitted) = compile(&member_schema(r#""type":"string","pattern":"^a\\_b\\-$""#));
    assert_eq!(omitted, Vec::<String>::new());
    assert!(accepts(&grammar, r#"{"v":"a_b-"}"#));
    assert!(!accepts(&grammar, r#"{"v":"axb-"}"#));
}

// Invariant: an invalid pattern and one no string matches are typed refusals.
#[test]
fn an_invalid_or_unsatisfiable_pattern_is_a_typed_refusal() {
    let invalid = try_compile(&member_schema(r#""type":"string","pattern":"(""#));
    assert!(
        matches!(invalid, Err(GbnfError::Pattern { .. })),
        "{invalid:?}"
    );
    let empty = try_compile(&member_schema(r#""type":"string","pattern":"[^\\s\\S]""#));
    assert!(
        matches!(empty, Err(GbnfError::Unsupported { .. })),
        "{empty:?}"
    );
}

/// The decimal integers as the instance text and the integer value.
fn integer_cases(range: std::ops::RangeInclusive<i64>, keywords: &str) -> Vec<(String, bool)> {
    let schema = Schema::compile(&member_schema(keywords)).expect("schema");
    range
        .map(|value| {
            let text = value.to_string();
            let instance = json::parse(&format!("{{\"v\":{text}}}")).expect("instance");
            (text, schema.validate(&instance).expect("validation").valid)
        })
        .collect()
}

// Invariant: integer bounds, inclusive, exclusive, fractional and open at either
// end, admit exactly the integers the schema admits, across digit-length changes.
#[test]
fn integer_bounds_are_represented_exactly() {
    for bounds in [
        r#""minimum":-3,"maximum":12"#,
        r#""exclusiveMinimum":0,"exclusiveMaximum":100"#,
        r#""minimum":5"#,
        r#""maximum":-5"#,
        r#""minimum":1.5,"maximum":9.5"#,
        r#""exclusiveMinimum":-2.5,"exclusiveMaximum":2.5"#,
        r#""minimum":7,"maximum":7"#,
        r#""minimum":99,"maximum":1000"#,
        r#""minimum":-1000,"maximum":-99"#,
        r#""minimum":1e2,"maximum":1.5e2"#,
        r#""minimum":0,"exclusiveMinimum":3"#,
    ] {
        let keywords = format!(r#""type":"integer",{bounds}"#);
        let cases = integer_cases(-1200..=1200, &keywords);
        assert!(cases.iter().any(|(_, valid)| *valid), "{bounds}");
        assert_member_cases(&keywords, &cases);
    }
}

// Invariant: decimal bounds admit exactly the finite decimals the schema admits,
// compared digit by digit with no rounding, and a decimal at the edge's own
// value or one digit beyond it falls on the schema's side of the bound.
#[test]
fn decimal_bounds_are_represented_exactly() {
    for bounds in [
        r#""minimum":0.5,"maximum":2.25"#,
        r#""exclusiveMinimum":-1.5,"exclusiveMaximum":0"#,
        r#""exclusiveMinimum":0.1"#,
        r#""maximum":0.30"#,
        r#""minimum":-0.05,"maximum":0.05"#,
        r#""minimum":1e1,"exclusiveMaximum":1.2e1"#,
    ] {
        let keywords = format!(r#""type":"number",{bounds}"#);
        let schema = Schema::compile(&member_schema(&keywords)).expect("schema");
        let mut texts: Vec<String> = Vec::new();
        for hundredths in 1..=1500_i64 {
            let (whole, fraction) = (hundredths / 100, hundredths % 100);
            for sign in ["", "-"] {
                texts.push(format!("{sign}{whole}.{fraction:02}"));
                texts.push(format!("{sign}{whole}.{fraction:02}0"));
                texts.push(format!("{sign}{whole}.{fraction:02}1"));
                if fraction % 10 == 0 {
                    texts.push(format!("{sign}{whole}.{}", fraction / 10));
                }
                if fraction == 0 {
                    texts.push(format!("{sign}{whole}"));
                }
            }
        }
        texts.extend([
            "0".to_owned(),
            "0.0".to_owned(),
            "0.0000001".to_owned(),
            "-0.0000001".to_owned(),
        ]);
        let cases: Vec<(String, bool)> = texts
            .into_iter()
            .map(|text| {
                let instance = json::parse(&format!("{{\"v\":{text}}}")).expect("instance");
                let valid = schema.validate(&instance).expect("validation").valid;
                (text, valid)
            })
            .collect();
        assert!(cases.iter().any(|(_, valid)| *valid), "{bounds}");
        assert_member_cases(&keywords, &cases);
    }
}

// Invariant: an `enum` member that fails a sibling bound or pattern is not
// generated, so nothing is ledgered for either keyword.
#[test]
fn an_enum_member_failing_a_sibling_constraint_is_not_generated() {
    assert_member_cases(
        r#""enum":[1,5,10],"minimum":3"#,
        &[
            ("1".to_owned(), false),
            ("5".to_owned(), true),
            ("10".to_owned(), true),
        ],
    );
    assert_member_cases(
        r#""enum":["ab","cd"],"pattern":"^a""#,
        &[(quoted("ab"), true), (quoted("cd"), false)],
    );
}

// Invariant: a back-reference, whose accepted set repeats an earlier match and
// is not context-free, is ledgered by name at its pointer, the string stays the
// generic string, and the schema check (`validate-input`) still refuses a
// violating value that the grammar accepts.
#[test]
fn a_back_reference_is_ledgered_and_validate_input_refuses_a_violation() {
    for pattern in [r"^(a|b)\1$", r"^(?<x>a|b)\k<x>$"] {
        let schema = member_schema(&format!(r#""type":"string","pattern":{}"#, quoted(pattern)));
        let (grammar, omitted) = compile(&schema);
        assert_eq!(omitted.len(), 1, "{pattern}");
        assert!(
            omitted[0].starts_with("/properties/v/pattern (back-reference"),
            "{omitted:?}"
        );
        assert!(
            accepts(&grammar, r#"{"v":"ab"}"#),
            "the grammar does not carry the pattern"
        );
        let instance = json::parse(r#"{"v":"ab"}"#).expect("instance");
        let report = Schema::compile(&schema)
            .expect("schema")
            .validate(&instance)
            .expect("validation");
        assert!(!report.valid, "validate-input refuses {pattern}");
    }
}

// Invariant: a look-around is refused by name and never ledgered, so no grammar
// that omits it is returned.
#[test]
fn a_look_around_is_refused_by_name() {
    for pattern in [r"a(?=b)", r"a(?!b)", r"(?<=a)b", r"(?<!a)b"] {
        let refused = try_compile(&member_schema(&format!(
            r#""type":"string","pattern":{}"#,
            quoted(pattern)
        )));
        let Err(GbnfError::Unsupported {
            pointer, reason, ..
        }) = refused
        else {
            panic!("{pattern}: {refused:?}");
        };
        assert_eq!(pointer, "/properties/v/pattern");
        assert!(
            reason.contains("look-around is not yet translated"),
            "{reason}"
        );
        assert!(reason.contains("issue 224"), "{reason}");
    }
}

// Invariant: a size or divisibility keyword a grammar can state is refused as
// not yet translated, uniqueness as not expressible, never ledgered or dropped,
// and an enum left with no member by its sibling constraint is unsatisfiable.
#[test]
fn untranslated_keywords_and_an_empty_enum_are_refused_by_name() {
    for (keywords, reason) in [
        (
            r#""type":"string","minLength":2"#,
            "minLength is not yet translated",
        ),
        (
            r#""type":"string","maxLength":2"#,
            "maxLength is not yet translated",
        ),
        (
            r#""type":"object","minProperties":1"#,
            "minProperties is not yet translated",
        ),
        (
            r#""type":"object","maxProperties":1"#,
            "maxProperties is not yet translated",
        ),
        (
            r#""type":"integer","multipleOf":2"#,
            "multipleOf is not yet translated",
        ),
        (
            r#""type":"array","items":{"type":"string"},"uniqueItems":true"#,
            "uniqueItems is not expressible by a context-free grammar",
        ),
        (
            r#""enum":[1,2],"minimum":5"#,
            "no enum member satisfies its pattern or bounds",
        ),
    ] {
        let refused = try_compile(&member_schema(keywords));
        let Err(GbnfError::Unsupported { reason: found, .. }) = refused else {
            panic!("{keywords}: {refused:?}");
        };
        assert_eq!(found, reason, "{keywords}");
    }
}

// Invariant: a pattern whose automaton has thousands of states ends as a
// resource stop under a small work budget, the same way every time, and never
// as a grammar or a hang.
#[test]
fn a_pattern_with_a_large_automaton_stops_on_the_budget() {
    let schema = member_schema(r#""type":"string","pattern":"^[ab]*a[ab]{16}$""#);
    let stop = || {
        let mut governor = Governor::new(Effort::Quick.budget());
        let node = json::parse_with(&schema, &mut governor).expect("schema JSON");
        match Builder::new("t").grammar(&node, &mut governor) {
            Err(GbnfError::Resource(reason)) => reason,
            other => panic!("expected a resource stop: {other:?}"),
        }
    };
    assert_eq!(stop(), stop());
    assert_eq!(stop(), StopReason::BudgetExceeded(Resource::Work));
}

/// Whether the lowered automaton of `pattern` accepts `text`.
fn automaton_accepts(automaton: &super::regex::Automaton, text: &str) -> bool {
    let mut state = 0;
    for character in text.chars() {
        let Some(&(_, _, next)) = automaton.states[state]
            .edges
            .iter()
            .find(|(first, last, _)| (*first..=*last).contains(&character))
        else {
            return false;
        };
        state = next;
    }
    automaton.states[state].accepting
}

// Differential: over generated strings, the strings the lowered automaton
// accepts are exactly those `Pattern::is_match` accepts, for no anchor, one
// anchor, an anchor inside an alternation, `\b`, a negated class and a class
// over astral characters.
#[test]
fn differential_automaton_acceptance_equals_pattern_is_match() {
    use purrassist_core::pattern::Pattern;
    let patterns = [
        r"ab+",
        r"^ab",
        r"ab$",
        r"^a|b$",
        r"\bab\b",
        r"[^a]b",
        r"[\u{1F600}-\u{1F64F}]a",
    ];
    let alphabet = ['a', 'b', ' ', '_', '\u{1F600}', '\u{1F680}', 'é'];
    let mut strings = vec![String::new()];
    let mut frontier = vec![String::new()];
    for _ in 0..4 {
        frontier = frontier
            .iter()
            .flat_map(|prefix| alphabet.iter().map(move |c| format!("{prefix}{c}")))
            .collect();
        strings.extend(frontier.iter().cloned());
    }
    let mut state = 0x2545_F491_4F6C_DD1D_u64;
    for _ in 0..500 {
        let mut text = String::new();
        for _ in 0..12 {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            text.push(alphabet[usize::try_from(state >> 33).expect("fits") % alphabet.len()]);
        }
        strings.push(text);
    }
    for source in patterns {
        let mut governor = Governor::new(Effort::Deep.budget());
        let pattern = Pattern::compile_governed(source, &mut governor).expect("pattern");
        let super::regex::Lowered::Automaton(automaton) =
            super::regex::lower(source, &mut governor).expect("lowered")
        else {
            panic!("{source} is regular");
        };
        for text in &strings {
            assert_eq!(
                automaton_accepts(&automaton, text),
                pattern.is_match(text, &mut governor).expect("match"),
                "{source} on {text:?}"
            );
        }
    }
}

// Invariant: a match containing a double quote, a backslash and a control
// character is spelled with JSON escapes in the grammar, and the decoded value
// matches the pattern.
#[test]
fn a_quote_a_backslash_and_a_control_character_are_spelled_escaped() {
    let keywords = r#""type":"string","pattern":"^\"\\\\\\x01$""#;
    let (grammar, _) = compile(&member_schema(keywords));
    for escaped in [r#""\\\"""#, r#""\\\\""#, r#""\\u0001""#] {
        assert!(grammar.contains(escaped), "{escaped} in\n{grammar}");
    }
    assert_member_cases(
        keywords,
        &[(quoted("\"\\\u{1}"), true), (quoted("\"\\"), false)],
    );
}

// Invariant: a refused keyword is refused in every placement, never routed to
// the ledger: beside an enum, beside anyOf, and as uniqueness.
#[test]
fn refused_keywords_are_refused_beside_enum_and_anyof_too() {
    for keywords in [
        r#""enum":["a"],"minLength":1"#,
        r#""enum":[2],"multipleOf":2"#,
        r#""const":"a","maxLength":3"#,
        r#""anyOf":[{"type":"array","items":{"type":"string"}}],"maxItems":2"#,
        r#""anyOf":[{"type":"array","items":{"type":"string"}}],"uniqueItems":true"#,
    ] {
        let refused = try_compile(&member_schema(keywords));
        assert!(
            matches!(refused, Err(GbnfError::Unsupported { .. })),
            "{keywords}: {refused:?}"
        );
    }
}

// Invariant: through `lanes::gbnf`, an open input shape records the
// `input-shape-not-expressible` entry for the GBNF target, and the ledger passes
// the registered loss profile. (The upstream schema compiler refuses a
// back-reference in a graph's `sh:pattern`, so the row's coverage at this entry
// point comes from open input shapes.)
#[test]
fn the_gbnf_lane_ledgers_input_shape_and_the_ledger_is_sound() {
    use purrdf_core::loss::check_ledger_sound_in;
    let turtle = worked_turtle().replace(
        "ex:QueryInput a sh:NodeShape ; sh:targetClass ex:QueryArguments ; sh:closed true ;",
        "ex:QueryInput a sh:NodeShape ; sh:targetClass ex:QueryArguments ;",
    );
    let (vocabulary, graph) = load_example(&turtle);
    let mut governor = Governor::new(Budget::default());
    let (_, ledger) = lanes::gbnf(&graph, &vocabulary, &mut governor).expect("gbnf lane");
    assert!(
        ledger.entries().iter().any(|entry| {
            entry.code == "input-shape-not-expressible" && entry.to == "llama-gbnf"
        })
    );
    check_ledger_sound_in(
        vocabulary.loss_profile(),
        &ledger,
        crate::SOURCE_REPRESENTATION,
        "llama-gbnf",
    )
    .expect("sound ledger");
}
