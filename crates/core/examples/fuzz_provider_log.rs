// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0

//! Check that provider event framing and tool recovery agree across chunkings.
//! The first control byte selects the provider and an optional short input
//! budget. The second selects chunk width.

use purrassist_core::{
    Budget,
    tool_call::{DialectSet, Param, ToolDefinition, ToolRecoveryRequest, ToolSet},
};
use std::io::{self, Read as _};

fn main() {
    let mut input = Vec::new();
    io::stdin().take(4097).read_to_end(&mut input).unwrap();
    assert!(input.len() <= 4096, "probe input exceeds 4096 bytes");
    let control = input.first().copied().unwrap_or(0);
    let width = 1 + usize::from(input.get(1).copied().unwrap_or(0));
    let log = input.get(2..).unwrap_or_default();
    let tools = ToolSet::compile(
        [
            ToolDefinition::new("get_weather").param("city", Param::string().required()),
            ToolDefinition::new("GetWeatherArgs")
                .param("city", Param::string().required())
                .param("country", Param::string().required())
                .param("units", Param::string().required()),
        ],
        Budget::default(),
    )
    .expect("weather tool declarations");
    let dialects = DialectSet::built_in();
    let mut request = ToolRecoveryRequest::default();
    if control & 2 != 0 {
        request.budget.input_bytes = 128;
    }
    let whole = log.len().max(1);
    if control & 1 == 0 {
        let read = |width: usize| {
            let mut stream = tools.stream_anthropic_events(request.clone(), dialects);
            let mut events = Vec::new();
            for chunk in log.chunks(width) {
                stream.feed(chunk, &mut events);
            }
            let result = stream.finish(&mut events);
            assert!(log.starts_with(&result.log));
            (
                result.recovery,
                result.log,
                result.faults,
                result.blocks,
                events,
            )
        };
        assert_eq!(read(whole), read(width));
    } else {
        let read = |width: usize| {
            let mut stream = tools.stream_openai_chat(request.clone(), dialects);
            let mut events = Vec::new();
            for chunk in log.chunks(width) {
                stream.feed(chunk, &mut events);
            }
            let result = stream.finish(&mut events);
            assert!(log.starts_with(&result.log));
            (
                result.recovery,
                result.log,
                result.faults,
                result.calls,
                events,
            )
        };
        assert_eq!(read(whole), read(width));
    }
    println!("ok");
}
