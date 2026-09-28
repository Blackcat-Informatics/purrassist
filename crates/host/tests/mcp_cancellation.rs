// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0
//! Deterministic cancellation/completion races without timing or mock tool results.
use purrassist::{
    digest::Digest,
    json::{self, Node},
    wire::{object, string},
};
use purrassist_host::{
    mcp::{Dispatch, Session, Work, read_frame},
    tools::Tools,
};
use std::{
    io::{BufReader, Cursor},
    path::PathBuf,
    sync::atomic::{AtomicUsize, Ordering},
};

struct Root(PathBuf);
impl Root {
    fn new() -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "purrassist-mcp-cancel-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&path).unwrap();
        Self(path)
    }
    fn session(&self) -> Session {
        let mut session = Session::new(Tools::new(&self.0).unwrap());
        assert!(session.handle(r#"{"jsonrpc":"2.0","id":0,"method":"initialize","params":{"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"test","version":"1"}}}"#).unwrap().get("result").is_some());
        assert!(
            session
                .handle(r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#)
                .is_none()
        );
        session
    }
}
impl Drop for Root {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.0).unwrap();
    }
}
fn decode(id: &str) -> String {
    format!(
        r#"{{"jsonrpc":"2.0","id":{id},"method":"tools/call","params":{{"name":"decode_json","arguments":{{"input":"[[42]]","schema":{{"type":"integer"}}}}}}}}"#
    )
}
fn work(session: &mut Session, frame: &str) -> Box<Work> {
    let Dispatch::Work(work) = session.dispatch(frame) else {
        panic!("expected admitted work")
    };
    work
}
fn cancel(session: &mut Session, id: &str) {
    assert!(matches!(session.dispatch(&format!(r#"{{"jsonrpc":"2.0","method":"notifications/cancelled","params":{{"requestId":{id},"reason":"test"}}}}"#)), Dispatch::Immediate(None)));
}

fn replacement_request() -> String {
    object([
        ("jsonrpc", string("2.0")),
        ("id", string("write")),
        ("method", string("tools/call")),
        (
            "params",
            object([
                ("name", string("apply_file")),
                (
                    "arguments",
                    object([
                        ("path", string("tea.txt")),
                        ("expected_digest", string(Digest::of(b"tea").to_string())),
                        ("replacement", string("coffee")),
                    ]),
                ),
            ]),
        ),
    ])
    .to_json()
}

#[test]
fn cancellation_reaches_admitted_work_and_keeps_slots_until_completion() {
    let root = Root::new();
    let mut session = root.session();
    let admitted = work(&mut session, &decode("1"));
    cancel(&mut session, "1e0"); // Mathematical numeric identity, not source spelling.
    assert_eq!(session.active_calls(), 1);
    let Dispatch::Immediate(Some(duplicate)) = session.dispatch(&decode("1.0")) else {
        panic!("duplicate ID")
    };
    assert_eq!(
        duplicate.pointer("/error/code").unwrap().to_json(),
        "-32600"
    );
    assert!(session.complete(admitted.run()).unwrap().is_none());
    assert_eq!(session.active_calls(), 0);
    cancel(&mut session, "1"); // Completed ID cannot cancel a later reuse.
    let retry = work(&mut session, &decode("1"));
    assert!(
        session
            .complete(retry.run())
            .unwrap()
            .unwrap()
            .get("result")
            .is_some()
    );
}

#[test]
fn malformed_unknown_and_differently_typed_ids_do_not_cancel() {
    let root = Root::new();
    let mut session = root.session();
    cancel(&mut session, "99");
    let admitted = work(&mut session, &decode("1"));
    cancel(&mut session, "\"1\"");
    for frame in [
        r#"{"jsonrpc":"2.0","method":"notifications/cancelled","params":{"requestId":1,"reason":false}}"#,
        r#"{"jsonrpc":"2.0","method":"notifications/cancelled","params":{"requestId":1,"requestId":1}}"#,
        r#"{"jsonrpc":"1.0","method":"notifications/cancelled","params":{"requestId":1}}"#,
        r#"{"jsonrpc":"2.0","method":"notifications/cancelled","params":{}}"#,
    ] {
        assert!(matches!(session.dispatch(frame), Dispatch::Immediate(None)));
    }
    let response = session.complete(admitted.run()).unwrap().unwrap();
    assert_eq!(
        response
            .pointer("/result/structuredContent/status")
            .unwrap()
            .as_str(),
        Some("resolved")
    );
}

#[test]
fn admission_is_bounded_and_cancelled_work_still_counts() {
    let root = Root::new();
    let mut session = root.session();
    let jobs: Vec<_> = (1..=4)
        .map(|id| work(&mut session, &decode(&id.to_string())))
        .collect();
    session.cancel_all();
    let Dispatch::Immediate(Some(full)) = session.dispatch(&decode("5")) else {
        panic!("admission limit")
    };
    assert_eq!(full.pointer("/error/code").unwrap().to_json(), "-32000");
    assert!(
        session
            .handle(r#"{"jsonrpc":"2.0","id":"ping","method":"ping"}"#)
            .unwrap()
            .get("result")
            .is_some()
    );
    for job in jobs {
        assert!(session.complete(job.run()).unwrap().is_none());
    }
    assert_eq!(session.active_calls(), 0);
}

#[test]
fn late_cancellation_preserves_completed_file_transaction() {
    let root = Root::new();
    std::fs::write(root.0.join("tea.txt"), "tea").unwrap();
    let mut session = root.session();
    let request = replacement_request();
    let early = work(&mut session, &request);
    cancel(&mut session, "\"write\"");
    assert!(session.complete(early.run()).unwrap().is_none());
    assert_eq!(
        std::fs::read_to_string(root.0.join("tea.txt")).unwrap(),
        "tea"
    );
    let completed = work(&mut session, &request).run();
    cancel(&mut session, "\"write\"");
    let response = session.complete(completed).unwrap().unwrap();
    assert!(
        response
            .pointer("/result/structuredContent/commit")
            .is_some()
    );
    assert_eq!(
        std::fs::read_to_string(root.0.join("tea.txt")).unwrap(),
        "coffee"
    );
}

#[test]
fn completions_are_session_bound_and_session_drop_cancels_work() {
    let root = Root::new();
    let mut first = root.session();
    let mut second = root.session();
    let completion = work(&mut first, &decode("1")).run();
    assert!(second.complete(completion).is_err());
    assert_eq!(first.active_calls(), 1);
    assert_eq!(second.active_calls(), 0);
    std::fs::write(root.0.join("tea.txt"), "tea").unwrap();
    let pending = work(&mut second, &replacement_request());
    drop(second);
    let completion = pending.run();
    assert_eq!(
        std::fs::read_to_string(root.0.join("tea.txt")).unwrap(),
        "tea"
    );
    assert!(first.complete(completion).is_err());
}

#[test]
fn framing_handles_fragmented_unicode_and_rejects_truncation_and_oversize() {
    let mut input = BufReader::with_capacity(1, Cursor::new("\"茶\"\n".as_bytes()));
    let frame = read_frame(&mut input).unwrap().unwrap();
    assert_eq!(json::parse(&frame).unwrap().as_str(), Some("茶"));
    assert!(read_frame(&mut input).unwrap().is_none());
    for bytes in [
        b"{\"x\":".to_vec(),
        vec![255, 10],
        vec![b'x'; 16 * 1024 * 1024 + 1],
    ] {
        assert!(read_frame(&mut Cursor::new(bytes)).is_err());
    }
    let task = r#"{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"decode_json","arguments":{},"task":{}}}"#;
    let root = Root::new();
    let mut session = root.session();
    let Dispatch::Immediate(Some(response)) = session.dispatch(task) else {
        panic!("unsupported task")
    };
    assert_eq!(
        response.pointer("/error/code").map(Node::to_json),
        Some("-32602".into())
    );
}
