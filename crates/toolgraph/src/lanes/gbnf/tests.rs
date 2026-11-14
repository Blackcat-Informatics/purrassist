// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0

//! Behaviour of the GBNF lane: accepted strings validate, losses are ledgered,
//! unsupported constructs are refused and output is deterministic.
//!
//! The sampler below reads the emitted grammar text itself, so a test exercises
//! the language the grammar defines and not the compiler's internal state.

use std::collections::BTreeMap;

use purrassist_core::{Budget, Governor, json, schema::Schema};
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
    Repeat(Box<Self>, usize, usize),
}

/// A recursive-descent reader for the subset of GBNF the compiler emits.
struct Reader<'a> {
    chars: std::iter::Peekable<std::str::Chars<'a>>,
}

impl Reader<'_> {
    fn escaped(&mut self) -> char {
        match self.chars.next().expect("escape") {
            'n' => '\n',
            'r' => '\r',
            't' => '\t',
            'x' => {
                let hex: String = (0..2).map(|_| self.chars.next().expect("hex")).collect();
                char::from_u32(u32::from_str_radix(&hex, 16).expect("hex")).expect("char")
            }
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
                    '*' => (0, 3),
                    '+' => (1, 3),
                    _ => (0, 1),
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
}

impl Sampler<'_> {
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
                let pick = self.next(branches.len());
                self.emit(&branches[pick], output);
            }
            Expr::Repeat(inner, low, high) => {
                for _ in 0..low + self.next(high - low + 1) {
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

fn compile(schema: &str) -> (String, Vec<String>) {
    try_compile(schema).expect("schema lowers")
}

fn try_compile(schema: &str) -> Result<(String, Vec<String>), GbnfError> {
    let mut governor = Governor::new(Budget::default());
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
    let mut sampler = Sampler {
        rules: &rules,
        state: 0x9E37_79B9_7F4A_7C15,
    };
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

// Invariant: the worked example's grammars accept only schema-valid strings except
// for keywords the ledger names (`pattern`, `minimum`, `maximum`), the output is
// byte-identical across runs, and the strings with surrogate escapes and the
// unexpressed bounds are ledgered, never dropped.
#[test]
fn the_worked_example_grammars_validate_except_for_ledgered_keywords() {
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
    let section = TOOLS
        .split("## End-to-end example")
        .nth(1)
        .expect("section");
    let turtle = section
        .split("```turtle\n")
        .nth(1)
        .and_then(|fence| fence.split("\n```").next())
        .expect("turtle fence");
    let mut governor = Governor::new(Budget::default());
    let graph = ToolGraph::load(
        turtle.as_bytes(),
        "text/turtle",
        None,
        &vocabulary,
        &mut governor,
    )
    .expect("graph loads");
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
    let notes: Vec<String> = ledger
        .entries()
        .iter()
        .map(|e| e.note.to_string())
        .collect();
    for grammar in &grammars {
        let declaration = declarations
            .iter()
            .find(|d| d.get("name").and_then(json::Node::as_str) == Some(grammar.tool()))
            .expect("declaration");
        let schema = declaration
            .get("inputSchema")
            .expect("input schema")
            .to_json();
        let failures = rejected(&schema, grammar.as_str(), 200);
        // The ledgered bounds are really unenforced, so the check is not vacuous.
        assert_eq!(failures.is_empty(), grammar.tool() == "edit_file");
        for (text, keywords) in failures {
            for keyword in keywords {
                assert!(
                    notes
                        .iter()
                        .any(|note| note.contains(&format!("/{keyword}"))),
                    "{text} fails unledgered keyword {keyword}"
                );
            }
        }
    }
    let codes = |code: &str| ledger.entries().iter().filter(|e| e.code == code).count();
    assert_eq!(codes("json-string-validity-not-expressible"), 2);
    assert_eq!(codes("output-shape-not-expressible"), 2);
    assert_eq!(codes("input-shape-not-expressible"), 1);
    assert!(
        notes
            .iter()
            .any(|n| n.contains("/properties/limit/maximum"))
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
        r#"{"type":"object","properties":{"s":{"type":"string","minLength":2},"i":{"type":"integer","minimum":1}},"required":["s"]}"#,
    );
    assert_eq!(
        omitted,
        [
            "/additionalProperties (members outside properties are not generated)",
            "/properties/s/minLength",
            "/properties/i/minimum",
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
