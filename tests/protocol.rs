use crustacean::board::{Board, Color};
use crustacean::search::{DEFAULT_DEPTH, MAX_DEPTH};
use crustacean::uci::{parse_go, parse_go_for_side, parse_position, serve};
use std::io::{BufRead, BufReader, Cursor, Write};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{self, Receiver};
use std::time::{Duration, Instant};

fn session(input: &str) -> String {
    let mut output = Vec::new();
    serve(Cursor::new(input), &mut output).unwrap();
    String::from_utf8(output).unwrap()
}

struct Engine {
    child: Child,
    stdin: Option<ChildStdin>,
    lines: Receiver<String>,
}
impl Engine {
    fn new() -> Self {
        let mut child = Command::new(env!("CARGO_BIN_EXE_crustacean"))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .unwrap();
        let stdin = child.stdin.take();
        let stdout = child.stdout.take().unwrap();
        let (tx, lines) = mpsc::channel();
        std::thread::spawn(move || {
            for line in BufReader::new(stdout).lines() {
                let Ok(line) = line else {
                    break;
                };
                if tx.send(line).is_err() {
                    break;
                }
            }
        });
        Self {
            child,
            stdin,
            lines,
        }
    }
    fn send(&mut self, text: &str) {
        let input = self.stdin.as_mut().unwrap();
        writeln!(input, "{text}").unwrap();
        input.flush().unwrap();
    }
    fn until(&self, prefix: &str) -> Vec<String> {
        let deadline = Instant::now() + Duration::from_secs(10);
        let mut lines = Vec::new();
        loop {
            let line = self
                .lines
                .recv_timeout(deadline.saturating_duration_since(Instant::now()))
                .unwrap_or_else(|e| panic!("waiting for {prefix}: {e}; received {lines:?}"));
            let found = line.starts_with(prefix);
            lines.push(line);
            if found {
                return lines;
            }
        }
    }
    fn handshake(&mut self) {
        self.send("uci\nisready");
        let lines = self.until("readyok");
        assert!(lines.iter().any(|line| line == "uciok"));
    }
    fn exit(&mut self) {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            if let Some(status) = self.child.try_wait().unwrap() {
                assert!(status.success());
                return;
            }
            assert!(Instant::now() < deadline, "engine failed to exit");
            std::thread::sleep(Duration::from_millis(5));
        }
    }
}
impl Drop for Engine {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
fn best(lines: &[String]) -> &str {
    lines.last().unwrap().strip_prefix("bestmove ").unwrap()
}

#[test]
fn handshake_position_and_search() {
    let output =
        session("uci\nisready\nposition startpos moves e2e4 e7e5\ngo depth 2 nodes 5000\n");
    assert!(output.contains("uciok\n"));
    assert!(output.contains("readyok\n"));
    let best = output
        .lines()
        .find_map(|l| l.strip_prefix("bestmove "))
        .unwrap();
    let (mut board, _) = parse_position(&["startpos", "moves", "e2e4", "e7e5"]).unwrap();
    board.play_uci(best).unwrap();
    assert!(output.contains(" seldepth "));
    assert!(output.contains(" pv "));
}
#[test]
fn invalid_position_does_not_partially_update_state() {
    let output =
        session("position startpos moves e2e4\nposition startpos moves d2d4 bogus\nd\nquit\n");
    let mut expected = Board::default();
    expected.play_uci("e2e4").unwrap();
    assert!(output.contains("info string error:"));
    assert!(output.contains(&expected.to_fen()));
}
#[test]
fn fen_and_newgame_and_eof() {
    let output = session("position fen 7k/8/8/8/8/8/8/4K3 w - - 0 1\ngo\nucinewgame\nd\n");
    assert!(output.contains("bestmove 0000"));
    assert!(output.contains(&Board::default().to_fen()));
}
#[test]
fn malformed_and_unsupported_search_requests_are_reported() {
    for words in [
        vec!["depth"],
        vec!["depth", "0"],
        vec!["depth", "65"],
        vec!["nodes", "-1"],
        vec!["ponder"],
        vec!["searchmoves", "e2e4"],
        vec!["movestogo", "0"],
        vec!["infinite", "depth", "1"],
        vec!["winc", "100"],
    ] {
        assert!(parse_go(&words).is_err(), "{words:?}");
    }
    let output = session("go ponder\nisready\nquit\n");
    assert!(output.contains("error: unsupported"));
    assert!(output.contains("bestmove 0000"));
    assert!(output.contains("readyok"));
}
#[test]
fn clock_allocation_uses_the_side_to_move_and_increment() {
    let words = [
        "wtime",
        "9000",
        "btime",
        "3000",
        "winc",
        "1000",
        "binc",
        "200",
        "movestogo",
        "10",
    ];
    let white = parse_go_for_side(&words, Color::White).unwrap();
    let black = parse_go_for_side(&words, Color::Black).unwrap();
    assert_eq!(white.time_limit, Some(Duration::from_millis(1650)));
    assert_eq!(black.time_limit, Some(Duration::from_millis(450)));
    assert_eq!(white.depth, Some(MAX_DEPTH));
    assert_eq!(white.node_limit, u64::MAX);
    let tiny = parse_go(&["wtime", "0"]).unwrap();
    assert_eq!(tiny.time_limit, Some(Duration::ZERO));
    assert!(parse_go_for_side(&["wtime", "1000"], Color::Black).is_err());
}
#[test]
fn depth_nodes_and_time_combine_without_hidden_small_caps() {
    let c = parse_go(&["depth", "32", "nodes", "9000000", "movetime", "500"]).unwrap();
    assert_eq!(c.depth, Some(32));
    assert_eq!(c.node_limit, 9_000_000);
    assert_eq!(c.time_limit, Some(Duration::from_millis(500)));
    let c = parse_go(&["nodes", "9000000"]).unwrap();
    assert_eq!(c.depth, Some(MAX_DEPTH));
    let c = parse_go(&["infinite"]).unwrap();
    assert_eq!(c.depth, Some(MAX_DEPTH));
    assert_eq!(c.node_limit, u64::MAX);
    assert_eq!(c.time_limit, None);
    assert_eq!(parse_go(&[]).unwrap().depth, None);
    assert_eq!(DEFAULT_DEPTH, 6);
}
#[test]
fn repetition_is_preserved_through_uci_moves() {
    let output = session("position startpos moves g1f3 g8f6 f3g1 f6g8 g1f3 g8f6 f3g1 f6g8\ngo\n");
    assert!(output.contains("threefold"));
    assert!(output.contains("bestmove 0000"));
}
#[test]
fn actual_binary_uci_process_smoke() {
    let mut e = Engine::new();
    e.handshake();
    e.send("go depth 1");
    let lines = e.until("bestmove ");
    let mut b = Board::default();
    b.play_uci(best(&lines)).unwrap();
    assert!(lines.iter().any(|line| line.starts_with("info depth 1 ")));
    e.send("quit");
    e.exit();
}
#[test]
fn actual_binary_play_and_cli_errors() {
    let mut child = Command::new(env!("CARGO_BIN_EXE_crustacean"))
        .arg("play")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(b"e2e4\nfen\nundo\nnew\nquit\n")
        .unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(output.status.success());
    let text = String::from_utf8(output.stdout).unwrap();
    assert!(text.contains("best="));
    assert!(text.contains("White to move"));
    let output = Command::new(env!("CARGO_BIN_EXE_crustacean"))
        .args(["analyze", "--depth", "0"])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
}
#[test]
fn actual_binary_help_and_analysis_use_standard_engine_output() {
    let help = Command::new(env!("CARGO_BIN_EXE_crustacean"))
        .arg("--help")
        .output()
        .unwrap();
    assert!(help.status.success());
    let text = String::from_utf8(help.stdout).unwrap();
    assert!(text.starts_with("Crustacean: Rust chess engine\n\n"));
    assert!(text.contains("Use --depth for fixed-depth analysis."));
    assert!(text.contains("--movetime"));
    let analysis = Command::new(env!("CARGO_BIN_EXE_crustacean"))
        .args(["analyze", "--depth", "1"])
        .output()
        .unwrap();
    assert!(analysis.status.success());
    let text = String::from_utf8(analysis.stdout).unwrap();
    let summary = text.lines().find(|line| line.starts_with("best=")).unwrap();
    let fields: Vec<_> = summary.split_whitespace().collect();
    assert_eq!(fields.len(), 5);
    assert!(fields[1].starts_with("score="));
    assert_eq!(fields[2], "depth=1");
    assert_eq!(fields[3], "nodes=20");
    assert_eq!(fields[4], "budget_exhausted=false");
    let timed = Command::new(env!("CARGO_BIN_EXE_crustacean"))
        .args(["analyze", "--movetime", "0"])
        .output()
        .unwrap();
    assert!(timed.status.success());
    assert!(String::from_utf8(timed.stdout).unwrap().contains("depth=0"));
}
#[test]
fn uci_node_budget_is_reported_and_enforced() {
    let output = session("position startpos\ngo nodes 50\n");
    assert!(output.contains("info string node budget exhausted\n"));
    let info = output
        .lines()
        .rfind(|line| line.starts_with("info depth "))
        .unwrap();
    let fields: Vec<_> = info.split_whitespace().collect();
    let index = fields.iter().position(|&v| v == "nodes").unwrap();
    assert_eq!(fields[index + 1], "50");
    assert!(output.contains("bestmove "));
}
#[test]
fn stop_and_isready_are_responsive_during_infinite_search() {
    let mut e = Engine::new();
    e.handshake();
    e.send("go infinite");
    e.until("info depth 1 ");
    let started = Instant::now();
    e.send("isready");
    let lines = e.until("readyok");
    assert!(!lines.iter().any(|line| line.starts_with("bestmove ")));
    e.send("stop");
    let lines = e.until("bestmove ");
    assert!(started.elapsed() < Duration::from_secs(3));
    Board::default().play_uci(best(&lines)).unwrap();
    e.send("quit");
    e.exit();
}
#[test]
fn timed_and_clock_based_searches_return_legal_moves() {
    let mut e = Engine::new();
    e.handshake();
    for command in [
        "go movetime 30",
        "go wtime 1000 btime 1000 winc 20 binc 20 movestogo 20",
    ] {
        let started = Instant::now();
        e.send(command);
        let lines = e.until("bestmove ");
        assert!(started.elapsed() < Duration::from_secs(3));
        Board::default().play_uci(best(&lines)).unwrap();
    }
    e.send("quit");
    e.exit();
}
#[test]
fn replacing_a_position_cancels_the_previous_worker() {
    let mut e = Engine::new();
    e.handshake();
    e.send("go infinite");
    e.until("info depth 1 ");
    e.send("position startpos moves e2e4\ngo depth 1");
    let previous = e.until("bestmove ");
    Board::default().play_uci(best(&previous)).unwrap();
    let current = e.until("bestmove ");
    let mut b = Board::default();
    b.play_uci("e2e4").unwrap();
    b.play_uci(best(&current)).unwrap();
    e.send("quit");
    e.exit();
}
#[test]
fn terminal_infinite_analysis_waits_for_stop() {
    let mut e = Engine::new();
    e.handshake();
    e.send("position fen 7k/6Q1/6K1/8/8/8/8/8 b - - 0 1\ngo infinite\nisready");
    let ready = e.until("readyok");
    assert!(!ready.iter().any(|line| line.starts_with("bestmove ")));
    assert!(e.lines.recv_timeout(Duration::from_millis(50)).is_err());
    e.send("stop");
    let lines = e.until("bestmove ");
    assert_eq!(best(&lines), "0000");
    e.send("quit");
    e.exit();
}
#[test]
fn infinite_search_is_cancelled_on_eof_and_quit() {
    for quit in [false, true] {
        let mut e = Engine::new();
        e.handshake();
        e.send("go infinite");
        e.until("info depth 1 ");
        if quit {
            e.send("quit");
        } else {
            e.stdin.take();
        }
        e.exit();
    }
}
