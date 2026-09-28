// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0
//! Bounded newline-delimited stdio MCP transport with explicit lifecycle state.
use crate::tools::Tools;
use purrassist::{
    json::{self, Node, Value},
    wire::{array, object, string},
};
use std::io::{BufRead, Write};

/// Stateful MCP session. No listener or background service is started.
#[derive(Debug)]
pub struct Session {
    tools: Tools,
    initialized: bool,
    ready: bool,
}
impl Session {
    /// Create a session over already-authorized, root-scoped tools.
    #[must_use]
    pub const fn new(tools: Tools) -> Self {
        Self {
            tools,
            initialized: false,
            ready: false,
        }
    }
    /// Process one JSON-RPC frame. Notifications do not generate responses.
    #[must_use]
    pub fn handle(&mut self, source: &str) -> Option<Node> {
        let request = match json::parse(source) {
            Ok(node) => node,
            Err(error) => return Some(failure(Node::new(Value::Null), -32700, &error.to_string())),
        };
        if !unique_members(&request) {
            return Some(failure(
                Node::new(Value::Null),
                -32600,
                "request must be an object with unique member names",
            ));
        }
        let id = request.get("id").cloned();
        if request.get("jsonrpc").and_then(Node::as_str) != Some("2.0")
            || request.get("method").and_then(Node::as_str).is_none()
        {
            return Some(failure(
                id.unwrap_or_else(|| Node::new(Value::Null)),
                -32600,
                "invalid JSON-RPC request",
            ));
        }
        let method = request.get("method").and_then(Node::as_str)?;
        if id.is_none() {
            if method == "notifications/initialized" && self.initialized {
                self.ready = true;
            }
            return None;
        }
        let id = id?;
        if !matches!(&id.value, Value::String(_))
            && !matches!(&id.value, Value::Number(number) if number.is_integer())
        {
            return Some(failure(
                Node::new(Value::Null),
                -32600,
                "request id must be a string or integer",
            ));
        }
        let params = request.get("params");
        if params.is_some_and(|params| !unique_members(params)) {
            return Some(failure(
                id,
                -32602,
                "params must be an object with unique member names",
            ));
        }
        let result = match method {
            "initialize" if !self.initialized => {
                if !params.is_some_and(valid_initialization) {
                    return Some(failure(
                        id,
                        -32602,
                        "initialize requires protocolVersion, capabilities, and clientInfo name/version",
                    ));
                }
                self.initialized = true;
                Ok(object([
                    ("protocolVersion", string("2025-11-25")),
                    (
                        "capabilities",
                        object([(
                            "tools",
                            object([("listChanged", Node::new(Value::Bool(false)))]),
                        )]),
                    ),
                    (
                        "serverInfo",
                        object([
                            ("name", string("purrassist")),
                            ("version", string(env!("CARGO_PKG_VERSION"))),
                        ]),
                    ),
                ]))
            }
            "ping" => Ok(object(Vec::<(String, Node)>::new())),
            _ if !self.ready => return Some(failure(id, -32002, "session is not initialized")),
            "tools/list" => {
                if params.and_then(|node| node.get("cursor")).is_some() {
                    return Some(failure(
                        id,
                        -32602,
                        "this server returns one complete tool page; no cursor is valid",
                    ));
                }
                Tools::definitions().map(|tools| object([("tools", tools)]))
            }
            "tools/call" => self.call(params),
            _ => return Some(failure(id, -32601, "unknown method")),
        };
        Some(match result {
            Ok(result) => object([("jsonrpc", string("2.0")), ("id", id), ("result", result)]),
            Err(error) => failure(id, -32602, &error),
        })
    }
    fn call(&self, params: Option<&Node>) -> Result<Node, String> {
        let params = params.ok_or("tools/call requires params")?;
        let name = params
            .get("name")
            .and_then(Node::as_str)
            .ok_or("tools/call requires name")?;
        if !Tools::contains(name) {
            return Err(format!("unknown tool {name:?}"));
        }
        let args = params
            .get("arguments")
            .ok_or("tools/call requires arguments")?;
        match self.tools.call(name, args) {
            Ok(result) => Ok(object([
                (
                    "content",
                    array([object([
                        ("type", string("text")),
                        ("text", string(result.to_json())),
                    ])]),
                ),
                ("structuredContent", result),
                ("isError", Node::new(Value::Bool(false))),
            ])),
            Err(error) => Ok(object([
                (
                    "content",
                    array([object([("type", string("text")), ("text", string(error))])]),
                ),
                ("isError", Node::new(Value::Bool(true))),
            ])),
        }
    }
    /// Serve until EOF with at most 16 MiB in any input frame. Flush each response.
    ///
    /// # Errors
    /// Returns transport I/O, oversized-frame, or invalid-UTF-8 failures. An
    /// oversized frame terminates the session without allocating its remainder.
    pub fn serve(
        &mut self,
        mut input: impl BufRead,
        mut output: impl Write,
    ) -> std::io::Result<()> {
        const LIMIT: usize = 16 * 1024 * 1024;
        let mut frame = Vec::new();
        loop {
            let available = input.fill_buf()?;
            if available.is_empty() {
                break;
            }
            let count = available
                .iter()
                .position(|byte| *byte == b'\n')
                .map_or(available.len(), |position| position + 1);
            if frame.len().saturating_add(count) > LIMIT {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "MCP frame exceeds 16 MiB",
                ));
            }
            let complete = available[count - 1] == b'\n';
            frame.extend_from_slice(&available[..count]);
            input.consume(count);
            if complete {
                let source = std::str::from_utf8(&frame)
                    .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
                if let Some(response) = self.handle(source) {
                    writeln!(output, "{}", response.to_json())?;
                    output.flush()?;
                }
                frame.clear();
            }
        }
        if !frame.is_empty() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::UnexpectedEof,
                "unterminated MCP frame",
            ));
        }
        Ok(())
    }
}
fn unique_members(node: &Node) -> bool {
    let Value::Object(members) = &node.value else {
        return false;
    };
    let mut names = std::collections::BTreeSet::new();
    members.iter().all(|member| names.insert(&member.key))
}
fn valid_initialization(params: &Node) -> bool {
    params
        .get("protocolVersion")
        .and_then(Node::as_str)
        .is_some()
        && params.get("capabilities").is_some_and(unique_members)
        && params.get("clientInfo").is_some_and(|info| {
            unique_members(info)
                && info.get("name").and_then(Node::as_str).is_some()
                && info.get("version").and_then(Node::as_str).is_some()
        })
}
fn failure(id: Node, code: i32, message: &str) -> Node {
    let code = Node::new(Value::Number(
        code.to_string().parse().expect("integer error code"),
    ));
    object([
        ("jsonrpc", string("2.0")),
        ("id", id),
        (
            "error",
            object([("code", code), ("message", string(message))]),
        ),
    ])
}
