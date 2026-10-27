// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0
//! Tool-call recovery throughput and charged work per byte; no external
//! benchmark framework.
//!
//! Recovers one write call whose string argument is 1 KiB, 64 KiB or
//! 1 MiB, printed as Qwen3-Coder XML, Hermes JSON, `OpenAI` stringified
//! arguments, Llama 3 JSON, Mistral v3, Mistral v11, DeepSeek-V3.1,
//! DeepSeek-V3, Kimi-K2, Anthropic `tool_use`, Harmony, bare JSON, and a
//! Qwen3-Coder XML call followed by a Hermes call (each carrying half the argument)
//! under a Qwen3-Coder + Hermes allow-list. The
//! single-dialect shapes use the default request (every built-in dialect).
//! Every recovery runs under `ToolRecoveryRequest::sized` and must be
//! `Resolved` with the argument intact. Each workload reports the median
//! wall-clock time, throughput, and the governor's work and peak scratch
//! per input byte (functions of the input alone). The `push_chunk_*`
//! workloads feed a 64 KiB Qwen3-Coder XML output to a `Recognizer` in
//! chunks of 1, 16 and 4096 bytes and assert that every chunking charges
//! identical usage.
//!
//! `--list` prints the workload names; `--bench` (or no argument) runs
//! every workload; a workload name runs that one. Results are a JSON array
//! on stdout and a Markdown table on stderr. This bench is not part of the
//! pinned qualification sets of `scripts/check-resource-limits.py`; its
//! timings are illustrative (`docs/RESOURCES.md`).
use purrassist_core::contract::Usage;
use purrassist_core::json::{self, Node};
use purrassist_core::tool_call::{
    DialectId, DialectSet, Effects, GenerationStop, Param, ReasoningPolicy, Recognizer,
    ToolDefinition, ToolRecoveryRequest, ToolSet,
};
use purrassist_core::wire::{array, integer, object, string};
use purrassist_core::{Budget, Effort, Governor, Outcome};
use std::hint::black_box;
use std::time::Instant;

const SIZES: [(&str, usize); 3] = [("1kib", 1 << 10), ("64kib", 1 << 16), ("1mib", 1 << 20)];
const SHAPES: [&str; 13] = [
    "qwen_xml",
    "hermes",
    "openai_stringified",
    "llama3_json",
    "mistral_v3",
    "mistral_v11",
    "deepseek_v31",
    "deepseek_v3",
    "kimi_k2",
    "anthropic_tool_use",
    "harmony",
    "bare_json",
    "mixed_qwen_hermes",
];
const CHUNKS: [usize; 3] = [1, 16, 4096];

fn tools() -> ToolSet {
    ToolSet::compile(
        [ToolDefinition::new("write")
            .effects(Effects::ReadOnly)
            .param("content", Param::string().required())],
        Budget::default(),
    )
    .expect("bench tools")
}

/// An escape-free argument of exactly `size` bytes.
fn argument(size: usize) -> String {
    "lorem ipsum dolor sit amet "
        .chars()
        .cycle()
        .take(size)
        .collect()
}

fn qwen(content: &str) -> String {
    format!(
        "<tool_call>\n<function=write>\n<parameter=content>\n{content}\n</parameter>\n</function>\n</tool_call>"
    )
}

fn hermes(content: &str) -> String {
    format!(
        "<tool_call>\n{{\"name\": \"write\", \"arguments\": {{\"content\": {}}}}}\n</tool_call>",
        json::quote(content)
    )
}

fn openai(content: &str) -> String {
    let arguments = format!("{{\"content\": {}}}", json::quote(content));
    format!(
        "{{\"id\": \"call_1\", \"type\": \"function\", \"function\": {{\"name\": \"write\", \"arguments\": {}}}}}",
        json::quote(&arguments)
    )
}

fn llama3(content: &str) -> String {
    format!(
        "{{\"name\": \"write\", \"parameters\": {{\"content\": {}}}}}",
        json::quote(content)
    )
}

fn mistral(content: &str) -> String {
    format!(
        "[TOOL_CALLS] [{{\"name\": \"write\", \"arguments\": {{\"content\": {}}}, \"id\": \"a1B2c3D4e\"}}]",
        json::quote(content)
    )
}

fn mistral11(content: &str) -> String {
    format!(
        "[TOOL_CALLS]write[CALL_ID]a1B2c3D4e[ARGS]{{\"content\": {}}}",
        json::quote(content)
    )
}

fn deepseek31(content: &str) -> String {
    format!(
        "<｜tool▁calls▁begin｜><｜tool▁call▁begin｜>write<｜tool▁sep｜>{{\"content\": {}}}<｜tool▁call▁end｜><｜tool▁calls▁end｜>",
        json::quote(content)
    )
}

fn deepseek3(content: &str) -> String {
    format!(
        "<｜tool▁calls▁begin｜><｜tool▁call▁begin｜>function<｜tool▁sep｜>write\n```json\n{{\"content\": {}}}\n```<｜tool▁call▁end｜><｜tool▁calls▁end｜>",
        json::quote(content)
    )
}

fn kimi(content: &str) -> String {
    format!(
        "<|tool_calls_section_begin|><|tool_call_begin|>functions.write:0<|tool_call_argument_begin|>{{\"content\": {}}}<|tool_call_end|><|tool_calls_section_end|>",
        json::quote(content)
    )
}

fn anthropic(content: &str) -> String {
    format!(
        "{{\"type\":\"tool_use\",\"id\":\"toolu_01\",\"name\":\"write\",\"input\":{{\"content\":{}}}}}",
        json::quote(content)
    )
}

fn harmony(content: &str) -> String {
    format!(
        "<|start|>assistant<|channel|>commentary to=functions.write <|constrain|>json<|message|>{{\"content\": {}}}<|call|>",
        json::quote(content)
    )
}

fn bare(content: &str) -> String {
    format!(
        "{{\"name\": \"write\", \"arguments\": {{\"content\": {}}}}}",
        json::quote(content)
    )
}

fn dialects(ids: &[DialectId]) -> DialectSet {
    DialectSet::new(ids.iter().cloned(), &mut Governor::new(Budget::default()))
        .expect("bench dialects")
}

/// The output, its request, and the argument values it must recover.
fn workload(shape: &str, size: usize) -> (String, ToolRecoveryRequest, Vec<String>, DialectSet) {
    let content = argument(size);
    let (output, expected, set) = match shape {
        "qwen_xml" => (
            qwen(&content),
            vec![content],
            dialects(&DialectId::BUILT_IN),
        ),
        "hermes" => (
            hermes(&content),
            vec![content],
            dialects(&DialectId::BUILT_IN),
        ),
        "openai_stringified" => (
            openai(&content),
            vec![content],
            dialects(&DialectId::BUILT_IN),
        ),
        "llama3_json" => (
            llama3(&content),
            vec![content],
            dialects(&DialectId::BUILT_IN),
        ),
        "mistral_v3" => (
            mistral(&content),
            vec![content],
            dialects(&DialectId::BUILT_IN),
        ),
        "mistral_v11" => (
            mistral11(&content),
            vec![content],
            dialects(&DialectId::BUILT_IN),
        ),
        "deepseek_v31" => (
            deepseek31(&content),
            vec![content],
            dialects(&DialectId::BUILT_IN),
        ),
        "deepseek_v3" => (
            deepseek3(&content),
            vec![content],
            dialects(&DialectId::BUILT_IN),
        ),
        "kimi_k2" => (
            kimi(&content),
            vec![content],
            dialects(&DialectId::BUILT_IN),
        ),
        "anthropic_tool_use" => (
            anthropic(&content),
            vec![content],
            dialects(&DialectId::BUILT_IN),
        ),
        "harmony" => (
            harmony(&content),
            vec![content],
            dialects(&DialectId::BUILT_IN),
        ),
        "bare_json" => (
            bare(&content),
            vec![content],
            dialects(&DialectId::BUILT_IN),
        ),
        "mixed_qwen_hermes" => {
            let first = argument(size / 2);
            let second = argument(size - size / 2);
            (
                format!("{}\n{}", qwen(&first), hermes(&second)),
                vec![first, second],
                dialects(&[DialectId::Qwen3CoderXml, DialectId::Hermes]),
            )
        }
        other => panic!("unknown shape {other}"),
    };
    let request = ToolRecoveryRequest::sized_for(Effort::Standard, output.as_bytes(), &set);
    (output, request, expected, set)
}

const fn samples(size: usize) -> usize {
    match size {
        0..=4096 => 200,
        4097..=131_072 => 30,
        _ => 7,
    }
}

/// Hundredths of `amount / bytes`, as a decimal string.
fn per_byte(amount: u64, bytes: usize) -> String {
    let hundredths = amount * 100 / bytes as u64;
    format!("{}.{:02}", hundredths / 100, hundredths % 100)
}

struct Row {
    name: String,
    bytes: usize,
    samples: usize,
    median_ns: u64,
    usage: Usage,
}

impl Row {
    fn json(&self) -> Node {
        object([
            ("case", string(&self.name)),
            ("bytes", integer(self.bytes as u64)),
            ("samples", integer(self.samples as u64)),
            ("median_ns", integer(self.median_ns)),
            ("work", integer(self.usage.work)),
            ("peak_scratch_bytes", integer(self.usage.peak_scratch_bytes)),
        ])
    }

    fn markdown(&self) -> String {
        // MiB per second from bytes per nanosecond, in hundredths.
        let mib_per_s_hundredths =
            (self.bytes as u128 * 1_000_000_000 * 100) / (u128::from(self.median_ns.max(1)) << 20);
        format!(
            "| `{}` | {} | {} | {}.{:03} | {}.{:02} | {} | {} |",
            self.name,
            self.bytes,
            self.samples,
            self.median_ns / 1_000_000,
            self.median_ns / 1_000 % 1_000,
            mib_per_s_hundredths / 100,
            mib_per_s_hundredths % 100,
            per_byte(self.usage.work, self.bytes),
            per_byte(self.usage.peak_scratch_bytes, self.bytes),
        )
    }
}

fn recover(tools: &ToolSet, shape: &str, label: &str, size: usize) -> Row {
    let (output, request, expected, dialects) = workload(shape, size);
    let count = samples(size);
    let mut times = Vec::with_capacity(count);
    let mut usage = None;
    for _ in 0..count {
        let start = Instant::now();
        let result = tools.recover(black_box(output.as_bytes()), &request, &dialects);
        times.push(u64::try_from(start.elapsed().as_nanos()).unwrap_or(u64::MAX));
        let Outcome::Resolved(calls) = &result.outcome else {
            panic!("{shape} {label}: {:?}", result.receipt.stopped);
        };
        let recovered: Vec<&str> = calls
            .usable()
            .iter()
            .filter_map(|call| call.arguments.get("content").and_then(Node::as_str))
            .collect();
        assert_eq!(recovered, expected, "{shape} {label}: arguments intact");
        // Charges are a function of the input alone.
        assert!(usage.is_none_or(|known| known == result.receipt.usage));
        usage = Some(result.receipt.usage);
    }
    times.sort_unstable();
    Row {
        name: format!("{shape}_{label}"),
        bytes: output.len(),
        samples: count,
        median_ns: times[count / 2],
        usage: usage.expect("at least one sample"),
    }
}

fn push_chunked(tools: &ToolSet, chunk: usize) -> Row {
    let output = qwen(&argument(1 << 16));
    let set = dialects(&[DialectId::Qwen3CoderXml]);
    let budget = ToolRecoveryRequest::sized_for(Effort::Standard, output.as_bytes(), &set).budget;
    let count = 30;
    let mut times = Vec::with_capacity(count);
    let mut usage = None;
    for _ in 0..count {
        let mut governor = Governor::new(budget);
        let start = Instant::now();
        let mut recognizer =
            Recognizer::with_tools(&set, tools, ReasoningPolicy::default(), &mut governor)
                .expect("recognizer");
        for piece in output.as_bytes().chunks(chunk) {
            recognizer
                .push(black_box(piece), &mut governor)
                .expect("push");
        }
        let turn = recognizer
            .finish(GenerationStop::Stop, &mut governor)
            .expect("finish");
        times.push(u64::try_from(start.elapsed().as_nanos()).unwrap_or(u64::MAX));
        assert_eq!(turn.calls().len(), 1);
        assert!(usage.is_none_or(|known| known == governor.usage()));
        usage = Some(governor.usage());
    }
    times.sort_unstable();
    Row {
        name: format!("push_chunk_{chunk}"),
        bytes: output.len(),
        samples: count,
        median_ns: times[count / 2],
        usage: usage.expect("at least one sample"),
    }
}

fn names() -> Vec<String> {
    let mut names = Vec::new();
    for shape in SHAPES {
        for (label, _) in SIZES {
            names.push(format!("{shape}_{label}"));
        }
    }
    for chunk in CHUNKS {
        names.push(format!("push_chunk_{chunk}"));
    }
    names
}

fn run(tools: &ToolSet, name: &str) -> Row {
    if let Some(chunk) = name.strip_prefix("push_chunk_") {
        return push_chunked(tools, chunk.parse().expect("a chunk size"));
    }
    for shape in SHAPES {
        for (label, size) in SIZES {
            if name == format!("{shape}_{label}") {
                return recover(tools, shape, label, size);
            }
        }
    }
    panic!("unknown tool-call workload: {name}")
}

fn main() {
    let mut args = std::env::args().skip(1);
    let selected: Vec<String> = match args.next().as_deref() {
        Some("--list") => {
            for name in names() {
                println!("{name}");
            }
            return;
        }
        Some("--bench") | None => names(),
        Some(name) => vec![name.to_owned()],
    };
    let tools = tools();
    let rows: Vec<Row> = selected.iter().map(|name| run(&tools, name)).collect();
    let chunked: Vec<&Row> = rows
        .iter()
        .filter(|row| row.name.starts_with("push_chunk_"))
        .collect();
    for row in &chunked {
        assert_eq!(
            row.usage, chunked[0].usage,
            "{}: chunking changed the charged usage",
            row.name
        );
    }
    eprintln!("| Workload | Bytes | Samples | Median ms | MiB/s | Work/byte | Peak scratch/byte |");
    eprintln!("| --- | ---: | ---: | ---: | ---: | ---: | ---: |");
    for row in &rows {
        eprintln!("{}", row.markdown());
    }
    println!("{}", array(rows.iter().map(Row::json)).to_json());
}
