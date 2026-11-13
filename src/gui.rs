//! Embedded localhost GUI. The Rust engine owns all game state and move legality.

use crate::board::Color;
use crate::form::form;
use crate::game::{quote, Game, Mode};
use crate::search::{search_with_control, Config, MAX_DEPTH};
use std::collections::BTreeMap;
use std::io::{self, BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::{
    atomic::{AtomicBool, AtomicUsize, Ordering},
    Arc, Condvar, Mutex,
};
use std::thread;
use std::time::{Duration, Instant};

const MAX_HEADER: usize = 8192;
const MAX_BODY: usize = 16_384;
type Shared = Arc<(Mutex<Game>, Condvar)>;

fn engine_loop(shared: Shared) {
    loop {
        let (lock, wake) = &*shared;
        let (board, history, version, cancel, milliseconds) = {
            let mut game = lock.lock().unwrap();
            while !game.needs_engine() {
                game = wake.wait(game).unwrap();
            }
            game.thinking = true;
            game.analysis = None;
            game.cancel = Arc::new(AtomicBool::new(false));
            (
                game.board.clone(),
                game.history(),
                game.version,
                Arc::clone(&game.cancel),
                if game.board.side == Color::White {
                    game.white_ms
                } else {
                    game.black_ms
                },
            )
        };
        let result = search_with_control(
            &board,
            Config {
                depth: Some(MAX_DEPTH),
                node_limit: u64::MAX,
                time_limit: Some(Duration::from_millis(milliseconds)),
            },
            &history,
            &cancel,
            |iteration| {
                let mut game = lock.lock().unwrap();
                if game.version == version && !cancel.load(Ordering::Relaxed) {
                    game.analysis = Some((iteration.clone(), board.side));
                }
            },
        );
        let mut game = lock.lock().unwrap();
        // A paused, reset, loaded or undone position cannot accept an old move.
        if game.version != version || cancel.load(Ordering::Relaxed) {
            continue;
        }
        game.thinking = false;
        game.step = false;
        game.analysis = Some((result.clone(), board.side));
        if let Some(m) = result.best_move {
            game.play(m);
        } else {
            game.outcome = result.outcome;
            game.running = false;
        }
        let completed_version = game.version;
        let until = Instant::now() + Duration::from_millis(200);
        while game.mode == Mode::Engines
            && game.needs_engine()
            && game.version == completed_version
            && Instant::now() < until
        {
            game = wake
                .wait_timeout(game, until.saturating_duration_since(Instant::now()))
                .unwrap()
                .0;
        }
    }
}

struct Request {
    method: String,
    path: String,
    headers: BTreeMap<String, String>,
    body: String,
}

fn read_request(stream: &TcpStream) -> Result<Request, (u16, String)> {
    let mut reader = BufReader::new(stream);
    let mut header = Vec::new();
    loop {
        if header.len() >= MAX_HEADER {
            return Err((431, "Request headers are too large".into()));
        }
        let available = reader.fill_buf().map_err(|e| (400, e.to_string()))?;
        if available.is_empty() {
            return Err((400, "Incomplete request".into()));
        }
        let byte = available[0];
        reader.consume(1);
        header.push(byte);
        if header.ends_with(b"\r\n\r\n") {
            break;
        }
    }
    let header = String::from_utf8(header).map_err(|_| (400, "Invalid request encoding".into()))?;
    let mut lines = header.split("\r\n");
    let first: Vec<_> = lines.next().unwrap_or("").split_whitespace().collect();
    if first.len() != 3 || !matches!(first[2], "HTTP/1.1" | "HTTP/1.0") {
        return Err((400, "Invalid HTTP request".into()));
    }
    let mut headers = BTreeMap::new();
    for line in lines.filter(|line| !line.is_empty()) {
        let (key, value) = line
            .split_once(':')
            .ok_or((400, "Invalid HTTP header".into()))?;
        if headers
            .insert(key.to_ascii_lowercase(), value.trim().to_string())
            .is_some()
        {
            return Err((400, "Duplicate HTTP header".into()));
        }
    }
    if headers.contains_key("transfer-encoding") {
        return Err((400, "Chunked requests are not supported".into()));
    }
    let length = headers
        .get("content-length")
        .map(|value| value.parse::<usize>())
        .transpose()
        .map_err(|_| (400, "Invalid content length".into()))?
        .unwrap_or(0);
    if length > MAX_BODY {
        return Err((413, "Request body is too large".into()));
    }
    let mut body = vec![0; length];
    reader
        .read_exact(&mut body)
        .map_err(|e| (400, e.to_string()))?;
    Ok(Request {
        method: first[0].into(),
        path: first[1].split('?').next().unwrap().into(),
        headers,
        body: String::from_utf8(body).map_err(|_| (400, "Invalid body encoding".into()))?,
    })
}

fn origin_allowed(request: &Request, port: u16) -> bool {
    let allowed = [format!("127.0.0.1:{port}"), format!("localhost:{port}")];
    let host = request
        .headers
        .get("host")
        .map(String::as_str)
        .unwrap_or("");
    allowed.iter().any(|value| value == host)
        && request.headers.get("origin").is_none_or(|origin| {
            allowed
                .iter()
                .any(|host| origin == &format!("http://{host}"))
        })
        && (request.method != "POST"
            || request
                .headers
                .get("sec-fetch-site")
                .is_none_or(|site| site != "cross-site"))
}

fn respond(stream: &mut TcpStream, status: u16, content_type: &str, body: &str) -> io::Result<()> {
    let reason = match status {
        200 => "OK",
        400 => "Bad Request",
        403 => "Forbidden",
        404 => "Not Found",
        405 => "Method Not Allowed",
        409 => "Conflict",
        413 => "Payload Too Large",
        431 => "Request Header Fields Too Large",
        _ => "Service Unavailable",
    };
    write!(stream, "HTTP/1.1 {status} {reason}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\nCache-Control: no-store\r\nX-Content-Type-Options: nosniff\r\nContent-Security-Policy: default-src 'self'; script-src 'self'; style-src 'self'; img-src 'self' data:; connect-src 'self'; frame-ancestors 'none'; base-uri 'none'; form-action 'self'\r\n\r\n{body}", body.len())?;
    stream.flush()
}
fn error(stream: &mut TcpStream, status: u16, message: &str) -> io::Result<()> {
    respond(
        stream,
        status,
        "application/json; charset=utf-8",
        &format!("{{\"error\":{}}}", quote(message)),
    )
}
fn handle(mut stream: TcpStream, shared: Shared, port: u16) -> io::Result<()> {
    stream.set_read_timeout(Some(Duration::from_secs(5)))?;
    stream.set_write_timeout(Some(Duration::from_secs(5)))?;
    let request = match read_request(&stream) {
        Ok(r) => r,
        Err((code, message)) => return error(&mut stream, code, &message),
    };
    if !origin_allowed(&request, port) {
        return error(&mut stream, 403, "Only the local GUI origin is allowed");
    }
    match (request.method.as_str(), request.path.as_str()) {
        ("GET", "/") => respond(
            &mut stream,
            200,
            "text/html; charset=utf-8",
            include_str!("../web/index.html"),
        ),
        ("GET", "/app.css") => respond(
            &mut stream,
            200,
            "text/css; charset=utf-8",
            include_str!("../web/app.css"),
        ),
        ("GET", "/app.js") => respond(
            &mut stream,
            200,
            "text/javascript; charset=utf-8",
            include_str!("../web/app.js"),
        ),
        ("GET", "/transport.js") => respond(
            &mut stream,
            200,
            "text/javascript; charset=utf-8",
            include_str!("../web/transport.js"),
        ),
        ("GET", "/sound.js") => respond(
            &mut stream,
            200,
            "text/javascript; charset=utf-8",
            include_str!("../web/sound.js"),
        ),
        ("GET", "/runtime.js") => respond(
            &mut stream,
            200,
            "text/javascript; charset=utf-8",
            "export const native = true;",
        ),
        ("GET", "/favicon.svg") => respond(
            &mut stream,
            200,
            "image/svg+xml",
            include_str!("../web/favicon.svg"),
        ),
        ("GET", "/api/state") => {
            let json = shared.0.lock().unwrap().json();
            respond(&mut stream, 200, "application/json; charset=utf-8", &json)
        }
        ("POST", "/api/action") => {
            if !request
                .headers
                .get("content-type")
                .is_some_and(|v| v.split(';').next() == Some("application/x-www-form-urlencoded"))
            {
                return error(&mut stream, 400, "Expected form-encoded action");
            }
            let fields = match form(&request.body) {
                Ok(f) => f,
                Err(e) => return error(&mut stream, 400, &e),
            };
            let (lock, wake) = &*shared;
            let mut game = lock.lock().unwrap();
            let result = game.action(&fields);
            let json = game.json();
            drop(game);
            match result {
                Ok(()) => {
                    wake.notify_one();
                    respond(&mut stream, 200, "application/json; charset=utf-8", &json)
                }
                Err((code, message)) => error(&mut stream, code, &message),
            }
        }
        (_, "/api/action") => error(&mut stream, 405, "Use POST for game actions"),
        _ => error(&mut stream, 404, "Not found"),
    }
}

struct Connection(Arc<AtomicUsize>);
impl Drop for Connection {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::Relaxed);
    }
}

/// Serve one shared game on loopback. Port zero chooses an available port.
pub fn serve(port: u16) -> io::Result<()> {
    let listener = TcpListener::bind(("127.0.0.1", port))?;
    let port = listener.local_addr()?.port();
    let shared: Shared = Arc::new((Mutex::new(Game::default()), Condvar::new()));
    let worker_state = Arc::clone(&shared);
    thread::Builder::new()
        .name("crustacean-gui-engine".into())
        .spawn(move || engine_loop(worker_state))?;
    println!("Crustacean GUI: http://127.0.0.1:{port}\nPress Ctrl+C to close the GUI server.");
    let connections = Arc::new(AtomicUsize::new(0));
    for connection in listener.incoming() {
        let mut stream = connection?;
        if connections.fetch_add(1, Ordering::Relaxed) >= 32 {
            connections.fetch_sub(1, Ordering::Relaxed);
            stream.set_write_timeout(Some(Duration::from_secs(1)))?;
            let _ = error(&mut stream, 503, "Too many local requests");
            continue;
        }
        let guard = Connection(Arc::clone(&connections));
        let shared = Arc::clone(&shared);
        thread::spawn(move || {
            let _guard = guard;
            let _ = handle(stream, shared, port);
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn form_decoding_is_strict_and_supports_fen() {
        assert_eq!(
            form("action=load&fen=8%2F8+w+%2B").unwrap()["fen"],
            "8/8 w +"
        );
        for body in ["action", "a=%", "a=%zz", "a=%ff", "a=1&a=2"] {
            assert!(form(body).is_err());
        }
    }
    #[test]
    fn only_loopback_hosts_and_same_origin_are_accepted() {
        let mut r = Request {
            method: "POST".into(),
            path: "/api/action".into(),
            headers: BTreeMap::from([("host".into(), "127.0.0.1:8787".into())]),
            body: String::new(),
        };
        assert!(origin_allowed(&r, 8787));
        r.headers
            .insert("origin".into(), "https://example.com".into());
        assert!(!origin_allowed(&r, 8787));
        r.headers
            .insert("origin".into(), "http://127.0.0.1:8787".into());
        assert!(origin_allowed(&r, 8787));
        r.headers.insert("host".into(), "example.com:8787".into());
        assert!(!origin_allowed(&r, 8787));
    }
}
