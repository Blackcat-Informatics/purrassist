// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0
//! Process-owned stdin reader and bounded MCP worker execution.
use purrassist_host::{
    mcp::{Completion, Dispatch, Session, read_frame},
    tools::Tools,
};
use std::{
    io::{self, Write},
    sync::mpsc::{self, Receiver, SyncSender},
    thread::{self, JoinHandle},
};

enum Event {
    Frame(String),
    Closed(Result<(), String>),
    Done(usize, Completion),
    Failed,
}

#[expect(
    clippy::redundant_pub_crate,
    reason = "binary command module needs sibling visibility"
)]
pub(super) fn run(tools: Tools) -> Result<(), String> {
    let mut session = Session::new(tools);
    // Four queued events plus one in-progress input frame. Workers are separately
    // limited by session admission, so a flood cannot create unbounded threads.
    let (send, receive) = mpsc::sync_channel(4);
    let input_send = send.clone();
    let reader = thread::Builder::new()
        .name("mcp-stdin".into())
        .spawn(move || {
            let result = std::panic::catch_unwind(|| {
                let mut input = io::stdin().lock();
                loop {
                    match read_frame(&mut input) {
                        Ok(Some(frame)) => {
                            if input_send.send(Event::Frame(frame)).is_err() {
                                return; // The owning CLI is shutting down.
                            }
                        }
                        result => {
                            let closed = result.map(|_| ()).map_err(|error| error.to_string());
                            let _ = input_send.send(Event::Closed(closed));
                            return;
                        }
                    }
                }
            });
            if result.is_err() {
                let _ = input_send.send(Event::Closed(Err("MCP stdin reader panicked".into())));
            }
        })
        .map_err(|error| error.to_string())?;
    let mut workers: [Option<JoinHandle<()>>; 4] = std::array::from_fn(|_| None);
    let result = pump(
        &mut session,
        &receive,
        &send,
        &mut workers,
        &mut io::stdout().lock(),
    );
    if result.is_err() {
        session.cancel_all();
    }
    // Unblock any worker publishing its final result before joining it. Journaled
    // mutations finish their transaction even if stdout has failed.
    drop(receive);
    drop(send);
    let mut final_result = result;
    for worker in workers.into_iter().flatten() {
        if worker.join().is_err() {
            final_result = Err("MCP worker panicked".into());
        }
    }
    if (final_result.is_ok() || reader.is_finished()) && reader.join().is_err() {
        return Err("MCP stdin reader panicked".into());
    }
    // On output failure stdin may still be blocked. Only this private CLI runtime
    // owns that reader; returning exits the process, never an embedding session.
    final_result
}

fn pump(
    session: &mut Session,
    receive: &Receiver<Event>,
    send: &SyncSender<Event>,
    workers: &mut [Option<JoinHandle<()>>; 4],
    output: &mut impl Write,
) -> Result<(), String> {
    let mut eof = false;
    while !eof || session.active_calls() != 0 {
        let response = match receive.recv().map_err(|error| error.to_string())? {
            Event::Frame(frame) => match session.dispatch(&frame) {
                Dispatch::Immediate(response) => response,
                Dispatch::Work(work) => {
                    let slot = workers
                        .iter()
                        .position(Option::is_none)
                        .ok_or("MCP worker admission invariant failed")?;
                    let done = send.clone();
                    workers[slot] = Some(
                        thread::Builder::new()
                            .name("mcp-tool".into())
                            .spawn(move || {
                                let event = std::panic::catch_unwind(|| work.run())
                                    .map_or(Event::Failed, |completion| {
                                        Event::Done(slot, completion)
                                    });
                                let _ = done.send(event); // Receiver closure means shutdown.
                            })
                            .map_err(|error| error.to_string())?,
                    );
                    None
                }
            },
            Event::Closed(result) => {
                result?;
                eof = true;
                None
            }
            Event::Done(slot, completion) => {
                workers[slot]
                    .take()
                    .ok_or("missing MCP worker")?
                    .join()
                    .map_err(|_| "MCP worker panicked")?;
                session.complete(completion)?
            }
            Event::Failed => return Err("MCP tool worker panicked".into()),
        };
        if let Some(response) = response {
            writeln!(output, "{}", response.to_json()).map_err(|error| error.to_string())?;
            output.flush().map_err(|error| error.to_string())?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    struct ReleaseOnWrite {
        bytes: Vec<u8>,
        release: Option<mpsc::Sender<()>>,
    }
    impl Write for ReleaseOnWrite {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            self.bytes.extend_from_slice(bytes);
            if let Some(release) = self.release.take() {
                release.send(()).unwrap();
            }
            Ok(bytes.len())
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn pump_services_cancellation_and_ping_before_worker_completion() {
        let root = std::env::temp_dir().join(format!("purrassist-mcp-pump-{}", std::process::id()));
        std::fs::create_dir(&root).unwrap();
        let mut session = Session::new(Tools::new(&root).unwrap());
        let _ = session.handle(r#"{"jsonrpc":"2.0","id":0,"method":"initialize","params":{"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"test","version":"1"}}}"#);
        let _ = session.handle(r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#);
        let Dispatch::Work(work) = session.dispatch(r#"{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"decode_json","arguments":{"input":"[[42]]","schema":{"type":"integer"}}}}"#) else { panic!("work") };
        let (send, receive) = mpsc::sync_channel(4);
        let (release, released) = mpsc::channel();
        let done = send.clone();
        let worker = thread::spawn(move || {
            released.recv().unwrap(); // Only a control response can release this worker.
            done.send(Event::Done(0, work.run())).unwrap();
        });
        let mut workers = [Some(worker), None, None, None];
        send.send(Event::Frame(
            r#"{"jsonrpc":"2.0","method":"notifications/cancelled","params":{"requestId":1}}"#
                .into(),
        ))
        .unwrap();
        send.send(Event::Frame(
            r#"{"jsonrpc":"2.0","id":2,"method":"ping"}"#.into(),
        ))
        .unwrap();
        send.send(Event::Closed(Ok(()))).unwrap();
        let mut output = ReleaseOnWrite {
            bytes: Vec::new(),
            release: Some(release),
        };
        pump(&mut session, &receive, &send, &mut workers, &mut output).unwrap();
        let text = String::from_utf8(output.bytes).unwrap();
        assert_eq!(text.lines().count(), 1);
        assert_eq!(
            purrassist::json::parse(&text)
                .unwrap()
                .get("id")
                .unwrap()
                .to_json(),
            "2"
        );
        assert_eq!(session.active_calls(), 0);
        assert!(workers.iter().all(Option::is_none));
        std::fs::remove_dir_all(root).unwrap();
    }
}
