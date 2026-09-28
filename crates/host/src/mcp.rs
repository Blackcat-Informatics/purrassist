// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0
//! Bounded newline-delimited stdio MCP transport with explicit lifecycle state.
use crate::tools::Tools;
use purrassist::{
    CancellationToken,
    json::{self, Node, Value},
    wire::{array, object, string},
};
use std::{
    io::{BufRead, Write},
    sync::Arc,
};

/// Stateful MCP session. No listener or background service is started.
#[derive(Debug)]
pub struct Session {
    tools: Arc<Tools>,
    pending: Vec<Pending>,
    initialized: bool,
    ready: bool,
}
impl Session {
    /// Create a session over already-authorized, root-scoped tools.
    #[must_use]
    pub fn new(tools: Tools) -> Self {
        Self {
            tools: Arc::new(tools),
            pending: Vec::new(),
            initialized: false,
            ready: false,
        }
    }
    /// Process one JSON-RPC frame. Notifications do not generate responses.
    #[must_use]
    pub fn handle(&mut self, source: &str) -> Option<Node> {
        match self.dispatch(source) {
            Dispatch::Immediate(response) => response,
            Dispatch::Work(work) => self
                .complete(work.run())
                .unwrap_or_else(|error| Some(failure(Node::new(Value::Null), -32603, &error))),
        }
    }

    fn handle_immediate(&mut self, request: &Node) -> Option<Node> {
        if !unique_members(request) {
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
        if !valid_id(&id) {
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
        let (name, args) = tool_arguments(params)?;
        Ok(tool_result(self.tools.call(name, args)))
    }
    /// Admit a frame without running a tool; continue dispatching notifications
    /// while executing returned work in a caller-owned executor.
    ///
    /// At most four tool calls remain admitted until `complete` consumes their
    /// completions. Duplicate active IDs and excess calls receive errors. Invalid,
    /// unknown, completed, and initialize cancellation notifications are ignored.
    ///
    /// ```no_run
    /// use purrassist_host::{tools::Tools, mcp::{Session, Dispatch}};
    /// let mut session = Session::new(Tools::new(".")?);
    /// let _ = session.handle(r#"{"jsonrpc":"2.0","id":0,"method":"initialize","params":{"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"example","version":"1"}}}"#);
    /// let _ = session.handle(r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#);
    /// if let Dispatch::Work(work) = session.dispatch(r#"{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"decode_json","arguments":{"input":"42","schema":{"type":"integer"}}}}"#) {
    ///     let completion = work.run(); // May run on the caller's worker thread.
    ///     let response = session.complete(completion)?;
    /// }
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    #[must_use]
    pub fn dispatch(&mut self, source: &str) -> Dispatch {
        let request = match json::parse(source) {
            Ok(request) => request,
            Err(error) => {
                return Dispatch::Immediate(Some(failure(
                    Node::new(Value::Null),
                    -32700,
                    &error.to_string(),
                )));
            }
        };
        let method = request.get("method").and_then(Node::as_str);
        if method == Some("notifications/cancelled") && request.get("id").is_none() {
            if unique_members(&request)
                && request.get("jsonrpc").and_then(Node::as_str) == Some("2.0")
                && let Some(params) = request.get("params").filter(|node| unique_members(node))
                && params
                    .get("reason")
                    .is_none_or(|reason| reason.as_str().is_some())
                && let Some(id) = params.get("requestId").filter(|id| valid_id(id))
                && let Some(pending) = self
                    .pending
                    .iter()
                    .find(|pending| pending.id.equivalent(id))
            {
                pending.token.cancel();
            }
            return Dispatch::Immediate(None);
        }
        let Some(id) = request.get("id").filter(|id| valid_id(id)) else {
            return Dispatch::Immediate(self.handle_immediate(&request));
        };
        if !unique_members(&request)
            || request.get("jsonrpc").and_then(Node::as_str) != Some("2.0")
            || request
                .get("params")
                .is_some_and(|params| !unique_members(params))
        {
            return Dispatch::Immediate(self.handle_immediate(&request));
        }
        if self.pending.iter().any(|pending| pending.id.equivalent(id)) {
            return Dispatch::Immediate(Some(failure(
                id.clone(),
                -32600,
                "request id is already active",
            )));
        }
        if method != Some("tools/call") || !self.ready {
            return Dispatch::Immediate(self.handle_immediate(&request));
        }
        let (name, args) = match tool_arguments(request.get("params")) {
            Ok(arguments) => arguments,
            Err(error) => return Dispatch::Immediate(Some(failure(id.clone(), -32602, &error))),
        };
        if self.pending.len() >= 4 {
            return Dispatch::Immediate(Some(failure(
                id.clone(),
                -32000,
                "four tool requests are already active",
            )));
        }
        let token = CancellationToken::new();
        let ticket = Arc::new(());
        self.pending.push(Pending {
            id: id.clone(),
            token: token.clone(),
            ticket: ticket.clone(),
        });
        Dispatch::Work(Box::new(Work {
            tools: self.tools.clone(),
            name: name.into(),
            args: args.clone(),
            id: id.clone(),
            token,
            ticket,
        }))
    }

    /// Release one admitted call and return its response, if processing was not
    /// cancelled. A late cancellation does not discard a completed transaction.
    ///
    /// # Errors
    /// Rejects a completion created by another session; no active call is removed.
    pub fn complete(&mut self, completion: Completion) -> Result<Option<Node>, String> {
        let index = self
            .pending
            .iter()
            .position(|pending| Arc::ptr_eq(&pending.ticket, &completion.ticket))
            .ok_or("completion does not belong to this session")?;
        self.pending.remove(index);
        Ok(completion.response)
    }

    /// Request cooperative cancellation for every active call during shutdown.
    /// Completed transactions are not rolled back; consume their completions.
    pub fn cancel_all(&self) {
        for pending in &self.pending {
            pending.token.cancel();
        }
    }

    /// Number of calls awaiting completion, including cancelled calls still running.
    #[must_use]
    pub const fn active_calls(&self) -> usize {
        self.pending.len()
    }

    /// Synchronously serve until EOF with at most 16 MiB in any input frame.
    /// Flush each response. Use `dispatch`/`complete` for concurrent cancellation;
    /// the shipped CLI uses that interface while keeping stdin responsive.
    ///
    /// # Errors
    /// Returns transport I/O, oversized-frame, or invalid-UTF-8 failures. An
    /// oversized frame terminates the session without allocating its remainder.
    pub fn serve(
        &mut self,
        mut input: impl BufRead,
        mut output: impl Write,
    ) -> std::io::Result<()> {
        while let Some(source) = read_frame(&mut input)? {
            if let Some(response) = self.handle(&source) {
                writeln!(output, "{}", response.to_json())?;
                output.flush()?;
            }
        }
        Ok(())
    }
}
impl Drop for Session {
    fn drop(&mut self) {
        self.cancel_all();
    }
}

#[derive(Debug)]
struct Pending {
    id: Node,
    token: CancellationToken,
    ticket: Arc<()>,
}

/// Result of dispatching one frame; notifications may have no immediate response.
#[derive(Debug)]
pub enum Dispatch {
    /// Send this response, if present, without running a tool.
    Immediate(Option<Node>),
    /// Execute on a caller-owned worker and return the completion to its session.
    Work(Box<Work>),
}

/// One bounded, admitted tool call. Dropping its session requests cancellation.
/// Execute it exactly once and pass the completion to the admitting session.
#[derive(Debug)]
pub struct Work {
    tools: Arc<Tools>,
    name: String,
    args: Node,
    id: Node,
    token: CancellationToken,
    ticket: Arc<()>,
}
impl Work {
    /// Run the real tool with its admitted cancellation token.
    ///
    /// An observed cancellation has no MCP response. Completed outcomes, including
    /// journaled file transactions, retain their response despite later signals.
    /// No thread or listener is started by this method.
    #[must_use]
    pub fn run(self) -> Completion {
        let result = self
            .tools
            .call_with_cancellation(&self.name, &self.args, &self.token);
        let cancelled = self.token.is_cancelled()
            && match &result {
                Err(error) => error == "cancelled",
                Ok(result) => {
                    result
                        .pointer("/receipt/termination/kind")
                        .and_then(Node::as_str)
                        == Some("cancelled")
                }
            };
        let response = (!cancelled).then(|| {
            object([
                ("jsonrpc", string("2.0")),
                ("id", self.id),
                ("result", tool_result(result)),
            ])
        });
        Completion {
            ticket: self.ticket,
            response,
        }
    }
}

/// Opaque, session-bound completion. Only `Session::complete` releases its slot.
#[derive(Debug)]
pub struct Completion {
    ticket: Arc<()>,
    response: Option<Node>,
}

fn valid_id(id: &Node) -> bool {
    matches!(&id.value, Value::String(_))
        || matches!(&id.value, Value::Number(number) if number.is_integer())
}

/// Read one newline-terminated UTF-8 frame, including its newline, bounded to 16 MiB.
/// Returns `None` only for EOF between frames. Does not allocate an oversized remainder.
///
/// # Errors
/// Returns I/O, invalid UTF-8, oversized-frame, or unterminated-frame errors.
///
/// ```
/// use purrassist_host::mcp::read_frame;
/// let mut input = std::io::Cursor::new(b"{}\n");
/// assert_eq!(read_frame(&mut input)?, Some("{}\n".into()));
/// assert_eq!(read_frame(&mut input)?, None);
/// # Ok::<(), std::io::Error>(())
/// ```
pub fn read_frame(input: &mut impl BufRead) -> std::io::Result<Option<String>> {
    const LIMIT: usize = 16 * 1024 * 1024;
    let mut frame = Vec::new();
    loop {
        let available = input.fill_buf()?;
        if available.is_empty() {
            return if frame.is_empty() {
                Ok(None)
            } else {
                Err(std::io::Error::new(
                    std::io::ErrorKind::UnexpectedEof,
                    "unterminated MCP frame",
                ))
            };
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
            return String::from_utf8(frame)
                .map(Some)
                .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, error));
        }
    }
}

fn tool_arguments(params: Option<&Node>) -> Result<(&str, &Node), String> {
    let params = params.ok_or("tools/call requires params")?;
    if params.get("task").is_some() {
        return Err("task-augmented requests are not supported".into());
    }
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
    Ok((name, args))
}

fn tool_result(result: Result<Node, String>) -> Node {
    match result {
        Ok(result) => object([
            (
                "content",
                array([object([
                    ("type", string("text")),
                    ("text", string(result.to_json())),
                ])]),
            ),
            ("structuredContent", result),
            ("isError", Node::new(Value::Bool(false))),
        ]),
        Err(error) => object([
            (
                "content",
                array([object([("type", string("text")), ("text", string(error))])]),
            ),
            ("isError", Node::new(Value::Bool(true))),
        ]),
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
