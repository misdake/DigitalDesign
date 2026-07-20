//! rcc-dbg: a lightweight web debugger for rcc binaries.
//!
//! serves a single-page UI over plain HTTP (no websocket): commands via POST,
//! state via GET. With no input it serves a single-file rcc playground.

use cpu_v2::debugger::DebugSession;
use cpu_v2::frontend::compile_program_named;
use cpu_v2::{Compiler, CompilerOptions};
use std::collections::HashMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::{Arc, Mutex};

const UI: &str = include_str!("rcc-dbg-ui.html");
const PLAYGROUND_UI: &str = include_str!("rcc-playground-ui.html");
const MAX_SOURCE_BYTES: usize = 1024 * 1024;
type SharedSession = Arc<Mutex<Option<DebugSession>>>;

fn main() -> ExitCode {
    let mut args = std::env::args().skip(1);
    let mut input: Option<PathBuf> = None;
    let mut port = 8321u16;
    let mut playground = false;
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
            "--playground" => playground = true,
            "-h" | "--help" => {
                println!("usage: rcc-dbg [input.bin] [--playground] [--port 8321]");
                println!("no input file starts the single-file playground");
                return ExitCode::SUCCESS;
            }
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
    if playground && input.is_some() {
        eprintln!("--playground does not take an input binary");
        return ExitCode::FAILURE;
    }
    playground |= input.is_none();

    let session = match input.as_deref() {
        Some(path) => match DebugSession::load(path) {
            Ok(s) => Some(s),
            Err(e) => {
                eprintln!("rcc-dbg: {e}");
                return ExitCode::FAILURE;
            }
        },
        None => None,
    };
    let session: SharedSession = Arc::new(Mutex::new(session));

    let listener = match TcpListener::bind(("127.0.0.1", port)) {
        Ok(l) => l,
        Err(e) => {
            eprintln!("rcc-dbg: cannot bind port {port}: {e}");
            return ExitCode::FAILURE;
        }
    };
    match input {
        Some(input) => println!("rcc-dbg: serving {} on http://127.0.0.1:{port}", input.display()),
        None => println!("rcc-dbg: playground on http://127.0.0.1:{port}"),
    }

    for conn in listener.incoming() {
        match conn {
            Ok(stream) => {
                let session = session.clone();
                std::thread::spawn(move || {
                    let _ = handle(stream, session, playground);
                });
            }
            Err(e) => eprintln!("rcc-dbg: accept error: {e}"),
        }
    }
    ExitCode::SUCCESS
}

fn handle(mut stream: TcpStream, session: SharedSession, playground: bool) -> std::io::Result<()> {
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
    if content_length > MAX_SOURCE_BYTES {
        write_response(&mut stream, "413 Payload Too Large", "text/plain", b"source exceeds 1 MiB")?;
        return Ok(());
    }
    let mut body = vec![0u8; content_length];
    reader.read_exact(&mut body)?;

    let (status, content_type, payload) = route(method, path, &body, &session, playground);
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
    body: &[u8],
    session: &SharedSession,
    playground_home: bool,
) -> (&'static str, &'static str, Vec<u8>) {
    let (path, query) = path.split_once('?').unwrap_or((path, ""));
    match (method, path) {
        ("GET", "/") => (
            "200 OK",
            "text/html; charset=utf-8",
            if playground_home { PLAYGROUND_UI } else { UI }.as_bytes().to_vec(),
        ),
        ("GET", "/playground") => (
            "200 OK",
            "text/html; charset=utf-8",
            PLAYGROUND_UI.as_bytes().to_vec(),
        ),
        ("GET", "/debugger") => (
            "200 OK",
            "text/html; charset=utf-8",
            UI.as_bytes().to_vec(),
        ),
        ("POST", "/api/compile") => {
            let optimize = parse_query(query).get("opt").is_none_or(|value| value != "0");
            let source = match std::str::from_utf8(body) {
                Ok(source) => source,
                Err(_) => return ("400 Bad Request", "text/plain; charset=utf-8", b"source is not UTF-8".to_vec()),
            };
            match compile_source(source, optimize) {
                Ok(compiled) => {
                    let json = compiled.state_json();
                    *session.lock().unwrap() = Some(compiled);
                    ("200 OK", "application/json", json.into_bytes())
                }
                Err(error) => ("400 Bad Request", "text/plain; charset=utf-8", error.into_bytes()),
            }
        }
        ("GET", "/api/state") => {
            let guard = session.lock().unwrap();
            let Some(s) = guard.as_ref() else {
                return no_session();
            };
            let json = s.state_json();
            ("200 OK", "application/json", json.into_bytes())
        }
        ("GET", "/api/mem") => {
            let q = parse_query(query);
            let addr = q.get("addr").and_then(|v| v.parse().ok()).unwrap_or(0);
            let len = q.get("len").and_then(|v| v.parse().ok()).unwrap_or(128);
            let guard = session.lock().unwrap();
            let Some(s) = guard.as_ref() else {
                return no_session();
            };
            let json = s.mem_json(addr, len);
            ("200 OK", "application/json", json.into_bytes())
        }
        ("POST", "/api/cmd") => {
            let q = parse_query(query);
            let mut guard = session.lock().unwrap();
            let Some(s) = guard.as_mut() else {
                return no_session();
            };
            match q.get("cmd").map(|s| s.as_str()) {
                Some("step") => s.step(),
                Some("next") => {
                    let _ = s.next_line(1_000_000);
                }
                Some("over") => {
                    let _ = s.step_over(1_000_000);
                }
                Some("out") => {
                    let _ = s.step_out(1_000_000);
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
            let mut guard = session.lock().unwrap();
            let Some(s) = guard.as_mut() else {
                return no_session();
            };
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
            let mut guard = session.lock().unwrap();
            let Some(s) = guard.as_mut() else {
                return no_session();
            };
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

fn no_session() -> (&'static str, &'static str, Vec<u8>) {
    ("409 Conflict", "text/plain", b"compile a program first".to_vec())
}

fn compile_source(source: &str, optimize: bool) -> Result<DebugSession, String> {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let mut opts = CompilerOptions::default();
        if !optimize {
            opts.opt = cpu_v2::Opts::disabled();
        }
        let program = compile_program_named("playground.rs", source, &opts, &mut |name| {
            Err(format!(
                "module `{name}` is unavailable: the playground currently supports one source file"
            ))
        })
        .map_err(|error| error.to_string())?;
        if !program.funcs.iter().any(|function| function.name == "main") {
            return Err("playground program needs a `fn main` entry point".to_string());
        }

        let mut compiler = Compiler::new();
        compiler.opts = opts;
        compiler.set_debug(program.debug);
        for function in program.funcs {
            compiler.add_func(function);
        }
        let (instructions, listing, debug) = compiler.finish_with_debug("main");
        let file = debug
            .files
            .iter()
            .position(|name| name == "playground.rs")
            .ok_or_else(|| "compiler did not emit the playground source file".to_string())?;
        let mut sources = HashMap::new();
        sources.insert(file as u16, source.to_string());
        Ok(DebugSession::from_compiled(
            instructions,
            listing,
            debug,
            sources,
        ))
    }))
    .map_err(|payload| {
        payload
            .downcast_ref::<String>()
            .cloned()
            .or_else(|| payload.downcast_ref::<&str>().map(|message| (*message).to_string()))
            .unwrap_or_else(|| "compiler panicked".to_string())
    })?
}

fn parse_query(query: &str) -> std::collections::HashMap<String, String> {
    query
        .split('&')
        .filter_map(|kv| kv.split_once('=').map(|(k, v)| (k.to_string(), v.to_string())))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn playground_compile_creates_an_in_memory_debug_session() {
        let source = "fn main() {\n    halt(7);\n}\n";
        let shared: SharedSession = Arc::new(Mutex::new(None));
        let (status, content_type, payload) =
            route("POST", "/api/compile", source.as_bytes(), &shared, true);
        assert_eq!(status, "200 OK");
        assert_eq!(content_type, "application/json");
        assert!(String::from_utf8(payload).unwrap().contains("halt(7);"));

        let mut guard = shared.lock().unwrap();
        let session = guard.as_mut().unwrap();
        assert_eq!(session.continue_run(100).1, Some(7));
        drop(guard);

        let replacement = "fn main() {\n    halt(9);\n}\n";
        let (status, _, _) = route(
            "POST",
            "/api/compile",
            replacement.as_bytes(),
            &shared,
            true,
        );
        assert_eq!(status, "200 OK");
        let mut guard = shared.lock().unwrap();
        assert_eq!(guard.as_mut().unwrap().continue_run(100).1, Some(9));
    }

    #[test]
    fn playground_rejects_module_files() {
        let error = match compile_source("mod other; fn main() { halt(0); }", true) {
            Ok(_) => panic!("module unexpectedly compiled"),
            Err(error) => error,
        };
        assert!(error.contains("supports one source file"), "{error}");
    }

    #[test]
    fn playground_reports_a_missing_main_function() {
        let error = match compile_source("fn helper() { halt(0); }", true) {
            Ok(_) => panic!("program without main unexpectedly compiled"),
            Err(error) => error,
        };
        assert!(error.contains("fn main"), "{error}");
    }

    #[test]
    fn playground_no_opt_keeps_scalar_locals_readable() {
        let source = concat!(
            "fn id(x: u16) -> u16 { x }\n",
            "fn main() {\n",
            "    let a = id(7);\n",
            "    let b = id(9);\n",
            "    halt(a + b);\n",
            "}\n",
        );

        let optimized = compile_source(source, true).unwrap();
        let optimized_main = optimized
            .debug
            .functions
            .iter()
            .find(|function| function.name == "main")
            .unwrap();
        assert!(optimized_main.locals.iter().any(|local| {
            local.name == "a" && matches!(local.loc, cpu_v2::VarLoc::Ssa)
        }));

        let shared: SharedSession = Arc::new(Mutex::new(None));
        let (status, _, _) = route(
            "POST",
            "/api/compile?opt=0",
            source.as_bytes(),
            &shared,
            true,
        );
        assert_eq!(status, "200 OK");
        let mut guard = shared.lock().unwrap();
        let session = guard.as_mut().unwrap();
        let main = session
            .debug
            .functions
            .iter()
            .find(|function| function.name == "main")
            .unwrap();
        let a = main.locals.iter().find(|local| local.name == "a").unwrap().clone();
        let b = main.locals.iter().find(|local| local.name == "b").unwrap().clone();
        assert!(matches!(a.loc, cpu_v2::VarLoc::Frame(_)));
        assert!(matches!(b.loc, cpu_v2::VarLoc::Frame(_)));

        assert!(session.toggle_breakpoint_line(0, 5, true).is_some());
        assert_eq!(session.continue_run(1_000).1, None);
        assert!(matches!(session.var_value(&a), cpu_v2::debugger::VarValue::Mem(_, ref words) if words == &[7]));
        assert!(matches!(session.var_value(&b), cpu_v2::debugger::VarValue::Mem(_, ref words) if words == &[9]));
    }
}
