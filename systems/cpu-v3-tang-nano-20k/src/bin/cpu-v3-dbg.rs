//! cpu-v3-dbg: a lightweight web debugger for CpuV3 rcc programs.
//!
//! serves a single-page UI over plain HTTP (no websocket): commands via POST,
//! state via GET. Compiles the input source in-process and drives the
//! architectural `Machine` directly.

use cpu_v3::rcc_backend::{self, CompilerOptions as CpuV3Options};
use cpu_v3_tang_nano_20k::debugger::V3DebugSession;
use rcc::frontend::compile_program_named;
use std::collections::HashMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

const UI: &str = include_str!("cpu-v3-dbg-ui.html");

fn main() -> ExitCode {
    let mut input: Option<PathBuf> = None;
    let mut port = 8322u16;
    let mut code_base: Option<u16> = None;
    let mut stack_init: Option<u16> = None;
    let mut no_open = false;
    let mut args = std::env::args().skip(1);
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
            "--code-base" => {
                let Some(v) = args.next() else {
                    eprintln!("--code-base needs a value");
                    return ExitCode::FAILURE;
                };
                let Some(n) = parse_u16(&v) else {
                    eprintln!("invalid --code-base: {v}");
                    return ExitCode::FAILURE;
                };
                code_base = Some(n);
            }
            "--stack-init" => {
                let Some(v) = args.next() else {
                    eprintln!("--stack-init needs a value");
                    return ExitCode::FAILURE;
                };
                let Some(n) = parse_u16(&v) else {
                    eprintln!("invalid --stack-init: {v}");
                    return ExitCode::FAILURE;
                };
                stack_init = Some(n);
            }
            "-h" | "--help" => {
                println!("usage: cpu-v3-dbg <input.rs | input-dir> [--code-base N] [--stack-init N] [--port 8322] [--no-open]");
                return ExitCode::SUCCESS;
            }
            "--no-open" => no_open = true,
            _ if a.starts_with('-') => {
                eprintln!("unknown option: {a}");
                return ExitCode::FAILURE;
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
        eprintln!("no input file (see --help)");
        return ExitCode::FAILURE;
    };

    let session = match compile_input(&input, code_base, stack_init) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("cpu-v3-dbg: {e}");
            return ExitCode::FAILURE;
        }
    };

    let listener = match TcpListener::bind(("127.0.0.1", port)) {
        Ok(l) => l,
        Err(e) => {
            eprintln!("cpu-v3-dbg: cannot bind port {port}: {e}");
            return ExitCode::FAILURE;
        }
    };
    println!(
        "cpu-v3-dbg: serving {} on http://127.0.0.1:{port}",
        input.display()
    );
    if !no_open {
        let url = format!("http://127.0.0.1:{port}");
        if let Err(e) = std::process::Command::new("cmd")
            .args(["/C", "start", "", &url])
            .spawn()
        {
            eprintln!("cpu-v3-dbg: could not open a browser: {e}");
        }
    }

    // single-threaded on purpose: `Machine` is not `Send` (its device trait
    // object is not thread-safe), and a debugger is single-user anyway.
    let mut session = session;
    for conn in listener.incoming() {
        match conn {
            Ok(stream) => {
                let _ = handle(stream, &mut session);
            }
            Err(e) => eprintln!("cpu-v3-dbg: accept error: {e}"),
        }
    }
    ExitCode::SUCCESS
}

/// resolve the input path (a directory means `<dir>/main.rs`) and compile it
fn compile_input(
    input: &Path,
    code_base: Option<u16>,
    stack_init: Option<u16>,
) -> Result<V3DebugSession, String> {
    let src_path = if input.is_dir() {
        input.join("main.rs")
    } else {
        input.to_path_buf()
    };
    let src = std::fs::read_to_string(&src_path)
        .map_err(|e| format!("cannot read {}: {e}", src_path.display()))?;

    let mut opts = CpuV3Options::default();
    if let Some(cb) = code_base {
        opts.code_base = cb;
    }
    if let Some(si) = stack_init {
        opts.stack_init = si;
    }

    let dir = src_path.parent().unwrap_or(Path::new(".")).to_path_buf();
    let mut loader = |name: &str| -> Result<String, String> {
        for cand in [
            dir.join(format!("{name}.rs")),
            dir.join(format!("{name}.dsl.rs")),
            dir.join(name).join("mod.rs"),
        ] {
            if let Ok(text) = std::fs::read_to_string(&cand) {
                return Ok(text);
            }
        }
        Err(format!("module file not found next to {}", src_path.display()))
    };

    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let program = compile_program_named(&src_path.display().to_string(), &src, &opts, &mut loader)
            .map_err(|error| error.to_string())?;
        if !program.funcs.iter().any(|f| f.name == "main") {
            return Err("program needs a `fn main` entry point".to_string());
        }
        let program = rcc_backend::compile(program, &opts, "main");
        Ok(V3DebugSession::from_program(program))
    }))
    .map_err(|payload| {
        payload
            .downcast_ref::<String>()
            .cloned()
            .or_else(|| payload.downcast_ref::<&str>().map(|m| (*m).to_string()))
            .unwrap_or_else(|| "compiler panicked".to_string())
    })?
}

fn parse_u16(s: &str) -> Option<u16> {
    if let Some(h) = s.strip_prefix("0x") {
        u16::from_str_radix(h, 16).ok()
    } else {
        s.parse().ok()
    }
}

fn handle(mut stream: TcpStream, session: &mut V3DebugSession) -> std::io::Result<()> {
    let mut reader = BufReader::new(stream.try_clone()?);
    let mut request_line = String::new();
    reader.read_line(&mut request_line)?;
    let mut parts = request_line.split_whitespace();
    let method = parts.next().unwrap_or("");
    let path = parts.next().unwrap_or("");

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

    let (status, content_type, payload) = route(method, path, session);
    write_response(&mut stream, status, content_type, &payload)
}

fn write_response(
    stream: &mut TcpStream,
    status: &str,
    content_type: &str,
    payload: &[u8],
) -> std::io::Result<()> {
    let head = format!(
        "HTTP/1.1 {status}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n",
        payload.len()
    );
    stream.write_all(head.as_bytes())?;
    stream.write_all(payload)
}

fn route(
    method: &str,
    path: &str,
    session: &mut V3DebugSession,
) -> (&'static str, &'static str, Vec<u8>) {
    let (path, query) = path.split_once('?').unwrap_or((path, ""));
    match (method, path) {
        ("GET", "/") => ("200 OK", "text/html; charset=utf-8", UI.as_bytes().to_vec()),
        ("GET", "/api/state") => {
            let q = parse_query(query);
            let file = q.get("file").and_then(|v| v.parse().ok());
            let json = session.state_json(file);
            ("200 OK", "application/json", json.into_bytes())
        }
        ("GET", "/api/mem") => {
            let q = parse_query(query);
            let addr = q.get("addr").and_then(|v| v.parse().ok()).unwrap_or(0);
            let len = q.get("len").and_then(|v| v.parse().ok()).unwrap_or(128);
            let json = session.mem_json(addr, len);
            ("200 OK", "application/json", json.into_bytes())
        }
        ("POST", "/api/cmd") => {
            let q = parse_query(query);
            match q.get("cmd").map(|s| s.as_str()) {
                Some("step") => session.step(),
                Some("next") => session.next_line(1_000_000),
                Some("over") => session.step_over(1_000_000),
                Some("out") => session.step_out(1_000_000),
                Some("continue") => {
                    let _ = session.continue_run(5_000_000);
                }
                Some("reset") => session.reset(),
                _ => return ("400 Bad Request", "text/plain", b"unknown cmd".to_vec()),
            }
            let json = session.state_json(None);
            ("200 OK", "application/json", json.into_bytes())
        }
        ("POST", "/api/breakline") => {
            let q = parse_query(query);
            let file = q.get("file").and_then(|v| v.parse().ok());
            let line = q.get("line").and_then(|v| v.parse().ok());
            let on = q.get("on").map(|v| v == "1").unwrap_or(true);
            match (file, line) {
                (Some(file), Some(line)) => {
                    if session.toggle_breakpoint_line(file, line, on).is_none() {
                        return (
                            "404 Not Found",
                            "text/plain",
                            b"no instruction for that line".to_vec(),
                        );
                    }
                }
                _ => {
                    return (
                        "400 Bad Request",
                        "text/plain",
                        b"missing file/line".to_vec(),
                    )
                }
            }
            let json = session.state_json(None);
            ("200 OK", "application/json", json.into_bytes())
        }
        _ => ("404 Not Found", "text/plain", b"not found".to_vec()),
    }
}

fn parse_query(query: &str) -> HashMap<String, String> {
    query
        .split('&')
        .filter_map(|kv| {
            kv.split_once('=')
                .map(|(k, v)| (k.to_string(), v.to_string()))
        })
        .collect()
}
