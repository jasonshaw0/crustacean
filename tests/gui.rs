//! Real HTTP/process checks. Each test owns a separate ephemeral localhost server.
use crustacean::board::{Board, START_FEN};
use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpStream;
use std::process::{Child, Command, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

struct Server {
    child: Child,
    port: u16,
}
impl Server {
    fn new() -> Self {
        let mut child = Command::new(env!("CARGO_BIN_EXE_crustacean"))
            .args(["gui", "--port", "0"])
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .unwrap();
        let out = child.stdout.take().unwrap();
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            for line in BufReader::new(out).lines() {
                let Ok(line) = line else {
                    break;
                };
                if tx.send(line).is_err() {
                    break;
                }
            }
        });
        let line = rx.recv_timeout(Duration::from_secs(5)).unwrap();
        let port = line
            .strip_prefix("Crustacean GUI: http://127.0.0.1:")
            .unwrap()
            .parse()
            .unwrap();
        Self { child, port }
    }
    fn raw(&self, request: &str) -> (u16, String, String) {
        let mut stream = TcpStream::connect(("127.0.0.1", self.port)).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        stream.write_all(request.as_bytes()).unwrap();
        let mut response = String::new();
        // The server deliberately closes each connection after its response.
        stream.read_to_string(&mut response).unwrap();
        let (headers, body) = response.split_once("\r\n\r\n").unwrap();
        let code = headers.split_whitespace().nth(1).unwrap().parse().unwrap();
        (code, headers.into(), body.into())
    }
    fn get(&self, path: &str) -> (u16, String) {
        let (status, _, body) = self.raw(&format!(
            "GET {path} HTTP/1.1\r\nHost: 127.0.0.1:{}\r\n\r\n",
            self.port
        ));
        (status, body)
    }
    fn state(&self) -> String {
        let (code, body) = self.get("/api/state");
        assert_eq!(code, 200);
        body
    }
    fn action(&self, body: &str) -> (u16, String) {
        let (status, _, body) = self.raw(&format!("POST /api/action HTTP/1.1\r\nHost: 127.0.0.1:{}\r\nContent-Type: application/x-www-form-urlencoded\r\nContent-Length: {}\r\n\r\n{body}", self.port, body.len()));
        (status, body)
    }
    fn ok(&self, body: &str) -> String {
        let (code, result) = self.action(body);
        assert_eq!(code, 200, "{result}");
        result
    }
    fn until(&self, predicate: impl Fn(&str) -> bool) -> String {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            let value = self.state();
            if predicate(&value) {
                return value;
            }
            assert!(
                Instant::now() < deadline,
                "GUI condition timed out: {value}"
            );
            std::thread::sleep(Duration::from_millis(10));
        }
    }
}
impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
fn text<'a>(json: &'a str, key: &str) -> &'a str {
    json.split(&format!("\"{key}\":\""))
        .nth(1)
        .unwrap()
        .split('"')
        .next()
        .unwrap()
}
fn number(json: &str, key: &str) -> u64 {
    json.split(&format!("\"{key}\":"))
        .nth(1)
        .unwrap()
        .split(|c: char| !c.is_ascii_digit())
        .next()
        .unwrap()
        .parse()
        .unwrap()
}
fn plies(json: &str) -> usize {
    json.matches("\"uci\":").count()
}
fn replay(json: &str) -> Board {
    let mut board = Board::from_fen(text(json, "starting_fen")).unwrap();
    for field in json.split("\"uci\":\"").skip(1) {
        board.play_uci(field.split('"').next().unwrap()).unwrap();
    }
    assert_eq!(board.to_fen(), text(json, "fen"));
    board
}
fn engines(server: &Server, ms: u64) -> String {
    server.ok(&format!(
        "action=new&mode=engines&human=white&white_ms={ms}&black_ms={ms}"
    ))
}

#[test]
fn embedded_assets_and_initial_state_are_served() {
    let s = Server::new();
    for path in [
        "/",
        "/app.css",
        "/app.js",
        "/transport.js",
        "/runtime.js",
        "/favicon.svg",
    ] {
        assert_eq!(s.get(path).0, 200, "{path}");
    }
    assert!(s.get("/runtime.js").1.contains("native = true"));
    let state = s.state();
    assert_eq!(text(&state, "fen"), START_FEN);
    assert_eq!(plies(&state), 0);
    let (_, headers, _) = s.raw(&format!(
        "GET / HTTP/1.1\r\nHost: 127.0.0.1:{}\r\nSec-Fetch-Site: cross-site\r\n\r\n",
        s.port
    ));
    assert!(headers.contains("Content-Security-Policy:"));
    assert!(headers.contains("Cache-Control: no-store"));
}

#[test]
fn separate_servers_have_isolated_sessions_and_new_games_preserve_session_id() {
    let a = Server::new();
    let b = Server::new();
    let state_a = a.state();
    let state_b = b.state();
    assert_ne!(text(&state_a, "session"), text(&state_b, "session"));
    a.ok("action=move&move=e2e4&version=0");
    assert_eq!(text(&b.state(), "fen"), START_FEN);
    let reset = engines(&a, 100);
    assert_eq!(text(&reset, "session"), text(&state_a, "session"));
}
#[test]
fn player_move_gets_a_legal_engine_reply_and_undo_restores_the_turn() {
    let s = Server::new();
    let moved = s.ok("action=move&move=e2e4&version=0");
    assert_eq!(plies(&moved), 1);
    assert!(moved.contains("\"san\":\"e4\""));
    let replied = s.until(|state| plies(state) == 2);
    replay(&replied);
    assert!(replied.contains("\"side\":\"white\""));
    assert!(number(&replied, "depth") > 0);
    let undone = s.ok("action=undo");
    assert_eq!(text(&undone, "fen"), START_FEN);
    assert!(undone.contains("\"running\":false"));
}
#[test]
fn human_black_receives_the_engine_opening() {
    let s = Server::new();
    s.ok("action=new&mode=player&human=black&white_ms=100&black_ms=100");
    let opened = s.until(|state| plies(state) == 1);
    replay(&opened);
    assert_eq!(text(&opened, "human"), "black");
    assert_eq!(text(&opened, "side"), "black");
}
#[test]
fn engine_match_step_resume_pause_and_undo_are_authoritative() {
    let s = Server::new();
    let ready = engines(&s, 100);
    assert!(ready.contains("\"running\":false"));
    s.ok("action=step");
    let stepped = s.until(|state| plies(state) == 1 && state.contains("\"thinking\":false"));
    replay(&stepped);
    std::thread::sleep(Duration::from_millis(250));
    assert_eq!(plies(&s.state()), 1);
    s.ok("action=resume");
    s.until(|state| plies(state) >= 3);
    let paused = s.ok("action=pause");
    let count = plies(&paused);
    replay(&paused);
    std::thread::sleep(Duration::from_millis(250));
    assert_eq!(plies(&s.state()), count);
    let undone = s.ok("action=undo");
    assert_eq!(plies(&undone), count - 1);
    replay(&undone);
}
#[test]
fn reset_cancels_an_in_flight_search_without_applying_a_stale_move() {
    let s = Server::new();
    let game = s.ok("action=new&mode=player&human=white&white_ms=5000&black_ms=5000");
    s.ok(&format!(
        "action=move&move=e2e4&version={}",
        number(&game, "version")
    ));
    s.until(|state| {
        state.contains("\"thinking\":true") && state.contains("\"analysis\":{\"depth\":")
    });
    s.ok("action=new&mode=player&human=white&white_ms=100&black_ms=100");
    std::thread::sleep(Duration::from_millis(250));
    let state = s.state();
    assert_eq!(plies(&state), 0);
    assert_eq!(text(&state, "fen"), START_FEN);
}
#[test]
fn invalid_input_and_stale_move_versions_preserve_state() {
    let s = Server::new();
    let before = s.state();
    assert_eq!(s.action("action=load&fen=invalid").0, 400);
    assert_eq!(s.state(), before);
    assert_eq!(s.action("action=move&move=e2e5&version=0").0, 400);
    assert_eq!(s.state(), before);
    engines(&s, 100);
    assert_eq!(s.action("action=move&move=e2e4&version=0").0, 409);
}
#[test]
fn promotion_and_terminal_positions_use_the_same_rules_as_the_engine() {
    let s = Server::new();
    let loaded = s.ok("action=load&fen=7k%2FP7%2F8%2F8%2F8%2F8%2F8%2F7K+w+-+-+0+1");
    let promoted = s.ok(&format!(
        "action=move&move=a7a8n&version={}",
        number(&loaded, "version")
    ));
    assert!(promoted.contains("\"san\":\"a8=N\""));
    assert!(promoted.contains("\"finished\":true"));
    assert_eq!(text(&promoted, "result"), "1/2-1/2");
    replay(&promoted);
    let mate = s.ok("action=load&fen=7k%2F6Q1%2F6K1%2F8%2F8%2F8%2F8%2F8+b+-+-+0+1");
    assert_eq!(text(&mate, "result"), "1-0");
    assert!(mate.contains("\"finished\":true"));
    assert_eq!(s.action("action=resume").0, 409);
}
#[test]
fn cross_origin_actions_and_oversized_requests_are_rejected() {
    let s = Server::new();
    let request = format!("POST /api/action HTTP/1.1\r\nHost: 127.0.0.1:{}\r\nOrigin: https://example.com\r\nContent-Length: 0\r\n\r\n", s.port);
    assert_eq!(s.raw(&request).0, 403);
    let huge = format!(
        "POST /api/action HTTP/1.1\r\nHost: 127.0.0.1:{}\r\nContent-Length: 999999\r\n\r\n",
        s.port
    );
    assert_eq!(s.raw(&huge).0, 413);
    let duplicate = format!(
        "GET / HTTP/1.1\r\nHost: 127.0.0.1:{}\r\nHost: localhost:{}\r\n\r\n",
        s.port, s.port
    );
    assert_eq!(s.raw(&duplicate).0, 400);
    assert_eq!(s.get("/api/action").0, 405);
    assert_eq!(s.get("/missing").0, 404);
    assert_eq!(text(&s.state(), "fen"), START_FEN);
}
