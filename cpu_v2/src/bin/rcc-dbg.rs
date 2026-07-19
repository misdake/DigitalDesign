//! rcc-dbg: a lightweight web debugger for rcc binaries.
//!
//! serves a single-page UI over plain HTTP (no websocket): commands via POST,
//! state via GET. usage: rcc-dbg <input.bin> [--port 8321]

use cpu_v2::debugger::DebugSession;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::{Arc, Mutex};

const UI: &str = include_str!("rcc-dbg-ui.html");

fn main() -> ExitCode {
    let mut args = std::env::args().skip(1);
    let mut input: Option<PathBuf> = None;
    let mut port = 8321u16;
    while let Some(a) = args.next() {
        match a.as_str() {
            "--port" => {
                let Some(v) = args.next() else {
                    eprintln!("--port needs a value");
                    return ExitCode::FAILURE;
                };
                let Ok(p) = v.parse() else {
                    eprintln!("invalid port: {v}");
                    return ExitCode::FAILURE;
                };
                port = p;
            }
            _ => {
                if input.is_some() {
                    eprintln!("multiple input files");
                    return ExitCode::FAILURE;
                }
                input = Some(PathBuf::from(a));
            }
        }
    }
    let Some(input) = input else {
        eprintln!("usage: rcc-dbg <input.bin> [--port 8321]");
        return ExitCode::FAILURE;
    };

    let session = match DebugSession::load(&input) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("rcc-dbg: {e}");
            return ExitCode::FAILURE;
        }
    };
    let session = Arc::new(Mutex::new(session));

    let listener = match TcpListener::bind(("127.0.0.1", port)) {
        Ok(l) => l,
        Err(e) => {
            eprintln!("rcc-dbg: cannot bind port {port}: {e}");
            return ExitCode::FAILURE;
        }
    };
    println!("rcc-dbg: serving {} on http://127.0.0.1:{port}", input.display());

    for conn in listener.incoming() {
        match conn {
            Ok(stream) => {
                let session = session.clone();
                std::thread::spawn(move || {
                    let _ = handle(stream, session);
                });
            }
            Err(e) => eprintln!("rcc-dbg: accept error: {e}"),
        }
    }
    ExitCode::SUCCESS
}

fn handle(mut stream: TcpStream, session: Arc<Mutex<DebugSession>>) -> std::io::Result<()> {
    let mut reader = BufReader::new(stream.try_clone()?);
    let mut request_line = String::new();
    reader.read_line(&mut request_line)?;
    let mut parts = request_line.split_whitespace();
    let method = parts.next().unwrap_or("");
    let path = parts.next().unwrap_or("");

    // headers (only content-length matters)
    let mut content_length = 0usize;
    loop {
        let mut line = String::new();
        reader.read_line(&mut line)?;
        let line = line.trim();
        if line.is_empty() {
            break;
        }
        if let Some(v) = line.to_ascii_lowercase().strip_prefix("content-length:") {
            content_length = v.trim().parse().unwrap_or(0);
        }
    }
    let mut body = vec![0u8; content_length];
    reader.read_exact(&mut body)?;

    let (status, content_type, payload) = route(method, path, &body, &session);
    let head = format!(
        "HTTP/1.1 {status}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        payload.len()
    );
    stream.write_all(head.as_bytes())?;
    stream.write_all(&payload)?;
    Ok(())
}

fn route(method: &str, path: &str, _body: &[u8], session: &Arc<Mutex<DebugSession>>) -> (&'static str, &'static str, Vec<u8>) {
    let (path, query) = path.split_once('?').unwrap_or((path, ""));
    match (method, path) {
        ("GET", "/") => (
            "200 OK",
            "text/html; charset=utf-8",
            UI.as_bytes().to_vec(),
        ),
        ("GET", "/api/state") => {
            let json = session.lock().unwrap().state_json();
            ("200 OK", "application/json", json.into_bytes())
        }
        ("GET", "/api/mem") => {
            let q = parse_query(query);
            let addr = q.get("addr").and_then(|v| v.parse().ok()).unwrap_or(0);
            let len = q.get("len").and_then(|v| v.parse().ok()).unwrap_or(128);
            let json = session.lock().unwrap().mem_json(addr, len);
            ("200 OK", "application/json", json.into_bytes())
        }
        ("POST", "/api/cmd") => {
            let q = parse_query(query);
            let mut s = session.lock().unwrap();
            match q.get("cmd").map(|s| s.as_str()) {
                Some("step") => s.step(),
                Some("next") => {
                    let _ = s.next_line(1_000_000);
                }
                Some("continue") => {
                    let _ = s.continue_run(5_000_000);
                }
                Some("reset") => s.reset(),
                _ => return ("400 Bad Request", "text/plain", b"unknown cmd".to_vec()),
            }
            let json = s.state_json();
            ("200 OK", "application/json", json.into_bytes())
        }
        ("POST", "/api/breakline") => {
            let q = parse_query(query);
            let mut s = session.lock().unwrap();
            let file = q.get("file").and_then(|v| v.parse().ok());
            let line = q.get("line").and_then(|v| v.parse().ok());
            let on = q.get("on").map(|v| v == "1").unwrap_or(true);
            match (file, line) {
                (Some(file), Some(line)) => {
                    if s.toggle_breakpoint_line(file, line, on).is_none() {
                        return ("404 Not Found", "text/plain", b"no instruction for that line".to_vec());
                    }
                }
                _ => return ("400 Bad Request", "text/plain", b"missing file/line".to_vec()),
            }
            let json = s.state_json();
            ("200 OK", "application/json", json.into_bytes())
        }
        ("POST", "/api/break") => {
            let q = parse_query(query);
            let mut s = session.lock().unwrap();
            let addr = q.get("addr").and_then(|v| usize::from_str_radix(v, 16).ok());
            let on = q.get("on").map(|v| v == "1").unwrap_or(true);
            match addr {
                Some(addr) => s.toggle_breakpoint(addr, on),
                None => return ("400 Bad Request", "text/plain", b"missing addr".to_vec()),
            }
            let json = s.state_json();
            ("200 OK", "application/json", json.into_bytes())
        }
        _ => ("404 Not Found", "text/plain", b"not found".to_vec()),
    }
}

fn parse_query(query: &str) -> std::collections::HashMap<String, String> {
    query
        .split('&')
        .filter_map(|kv| kv.split_once('=').map(|(k, v)| (k.to_string(), v.to_string())))
        .collect()
}
