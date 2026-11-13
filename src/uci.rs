//! UCI input remains responsive while a scoped worker searches a position snapshot.
use crate::board::{Board, Color, PositionKey};
use crate::search::{
    search_with_control, Config, SearchResult, StopReason, MATE, MAX_DEPTH, MAX_PLY,
};
use std::io::{self, BufRead, Write};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex,
};
use std::thread::{self, ScopedJoinHandle};
use std::time::Duration;

/// Parse into temporary state so a bad move cannot partly replace the position.
pub fn parse_position(words: &[&str]) -> Result<(Board, Vec<PositionKey>), String> {
    let (mut board, mut index) = match words.first().copied() {
        Some("startpos") => (Board::default(), 1),
        Some("fen") if words.len() >= 7 => (Board::from_fen(&words[1..7].join(" "))?, 7),
        _ => return Err("position requires startpos or fen <six fields>".into()),
    };
    let mut history = vec![board.key()];
    if index < words.len() {
        if words[index] != "moves" {
            return Err("expected moves after position".into());
        }
        index += 1;
        for word in &words[index..] {
            board.play_uci(word)?;
            history.push(board.key());
        }
    }
    Ok((board, history))
}

/// Parse limits for CLI analysis (which has no opponent clock).
pub fn parse_go(words: &[&str]) -> Result<Config, String> {
    parse_go_for_side(words, Color::White)
}

pub fn parse_go_for_side(words: &[&str], side: Color) -> Result<Config, String> {
    let mut config = Config::default();
    let mut nodes = None;
    let (mut wtime, mut btime, mut winc, mut binc, mut movetime, mut movestogo) =
        (None, None, 0u64, 0u64, None, None);
    let mut infinite = false;
    let mut i = 0;
    while i < words.len() {
        let key = words[i];
        if key == "infinite" {
            infinite = true;
            i += 1;
            continue;
        }
        if !matches!(
            key,
            "depth" | "nodes" | "movetime" | "wtime" | "btime" | "winc" | "binc" | "movestogo"
        ) {
            return Err(format!("unsupported go parameter: {key}"));
        }
        let value = words.get(i + 1).ok_or("go parameter needs a value")?;
        let value: u64 = value.parse().map_err(|_| format!("invalid {key}"))?;
        match key {
            "depth" => {
                if !(1..=u64::from(MAX_DEPTH)).contains(&value) {
                    return Err(format!("depth must be 1..={MAX_DEPTH}"));
                }
                config.depth = Some(value as u8);
            }
            "nodes" => nodes = Some(value),
            "movetime" => movetime = Some(value),
            "wtime" => wtime = Some(value),
            "btime" => btime = Some(value),
            "winc" => winc = value,
            "binc" => binc = value,
            "movestogo" => {
                if value == 0 {
                    return Err("movestogo must be positive".into());
                }
                movestogo = Some(value);
            }
            _ => unreachable!(),
        }
        i += 2;
    }
    let clock_request =
        wtime.is_some() || btime.is_some() || winc != 0 || binc != 0 || movestogo.is_some();
    if infinite {
        if config.depth.is_some() || nodes.is_some() || movetime.is_some() || clock_request {
            return Err("infinite cannot be combined with finite limits".into());
        }
        config.depth = Some(MAX_DEPTH);
        config.node_limit = u64::MAX;
        return Ok(config);
    }
    let time = if let Some(ms) = movetime {
        Some(ms)
    } else if clock_request {
        let (remaining, increment) = if side == Color::White {
            (wtime, winc)
        } else {
            (btime, binc)
        };
        let remaining = remaining.ok_or("time for the side to move is required")?;
        let reserve = (remaining / 20).clamp(1, 50);
        let available = remaining.saturating_sub(reserve);
        Some(
            (remaining / movestogo.unwrap_or(30))
                .saturating_add(increment.saturating_mul(3) / 4)
                .max(1)
                .min(available),
        )
    } else {
        None
    };
    if let Some(ms) = time {
        config.time_limit = Some(Duration::from_millis(ms));
        config.depth = Some(config.depth.unwrap_or(MAX_DEPTH));
        config.node_limit = u64::MAX;
    }
    if let Some(nodes) = nodes {
        config.node_limit = nodes;
        config.depth = Some(config.depth.unwrap_or(MAX_DEPTH));
    }
    Ok(config)
}

fn emit<W: Write>(output: &Arc<Mutex<W>>, message: &str) -> io::Result<()> {
    let mut output = output
        .lock()
        .map_err(|_| io::Error::other("UCI output lock poisoned"))?;
    writeln!(output, "{message}")?;
    output.flush()
}

fn info(result: &SearchResult) -> String {
    let score = if result.score.abs() >= MATE - MAX_PLY as i32 {
        let distance = (MATE - result.score.abs() + 1) / 2;
        format!("mate {}", distance * result.score.signum())
    } else {
        format!("cp {}", result.score)
    };
    let millis = result.elapsed.as_millis();
    let nps = u128::from(result.nodes) * 1000 / millis.max(1);
    let pv = result
        .pv
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join(" ");
    format!(
        "info depth {} seldepth {} score {score} nodes {} time {millis} nps {nps} pv {pv}",
        result.completed_depth, result.selective_depth, result.nodes
    )
}

struct ActiveSearch<'scope> {
    stop: Arc<AtomicBool>,
    handle: ScopedJoinHandle<'scope, io::Result<()>>,
    unbounded: bool,
}
fn finish(active: &mut Option<ActiveSearch<'_>>, cancel: bool) -> io::Result<()> {
    if let Some(active) = active.take() {
        if cancel {
            active.stop.store(true, Ordering::Relaxed);
        }
        active
            .handle
            .join()
            .map_err(|_| io::Error::other("search worker panicked"))??;
    }
    Ok(())
}

pub fn serve(input: impl BufRead, output: impl Write + Send) -> io::Result<()> {
    let output = Arc::new(Mutex::new(output));
    thread::scope(|scope| {
        let mut board = Board::default();
        let mut history = vec![board.key()];
        let mut active: Option<ActiveSearch<'_>> = None;
        let result = (|| {
            for line in input.lines() {
                let line = line?;
                let words: Vec<_> = line.split_whitespace().collect();
                match words.first().copied() {
                    Some("uci") => emit(
                        &output,
                        "id name Crustacean 0.1\nid author Crustacean contributors\nuciok",
                    )?,
                    Some("isready") => emit(&output, "readyok")?,
                    Some("ucinewgame") => {
                        finish(&mut active, true)?;
                        board = Board::default();
                        history = vec![board.key()];
                    }
                    Some("position") => match parse_position(&words[1..]) {
                        Ok((b, h)) => {
                            finish(&mut active, true)?;
                            board = b;
                            history = h;
                        }
                        Err(e) => emit(&output, &format!("info string error: {e}"))?,
                    },
                    Some("go") => match parse_go_for_side(&words[1..], board.side) {
                        Ok(config) => {
                            finish(&mut active, true)?;
                            let snapshot = board.clone();
                            let history = history.clone();
                            let writer = Arc::clone(&output);
                            let stop = Arc::new(AtomicBool::new(false));
                            let worker_stop = Arc::clone(&stop);
                            let unbounded = config.depth == Some(MAX_DEPTH)
                                && config.node_limit == u64::MAX
                                && config.time_limit.is_none();
                            let handle = thread::Builder::new()
                                .name("crustacean-search".into())
                                .spawn_scoped(scope, move || {
                                    let mut failure = None;
                                    let result = search_with_control(
                                        &snapshot,
                                        config,
                                        &history,
                                        &worker_stop,
                                        |iteration| {
                                            if failure.is_none() {
                                                if let Err(e) = emit(&writer, &info(iteration)) {
                                                    failure = Some(e);
                                                    worker_stop.store(true, Ordering::Relaxed);
                                                }
                                            }
                                        },
                                    );
                                    if let Some(e) = failure {
                                        return Err(e);
                                    }
                                    // An infinite analysis must wait for stop even if
                                    // a terminal position or forced mate ends search.
                                    while unbounded && !worker_stop.load(Ordering::Relaxed) {
                                        thread::park_timeout(Duration::from_millis(5));
                                    }
                                    if let Some(reason) = result.stop_reason {
                                        let message = match reason {
                                            StopReason::Nodes => "node budget exhausted",
                                            StopReason::Time => "time budget exhausted",
                                            StopReason::Cancelled => "search stopped",
                                            StopReason::PlyLimit => "selective depth limit reached",
                                        };
                                        emit(&writer, &format!("info string {message}"))?;
                                    }
                                    if let Some(outcome) = result.outcome {
                                        emit(&writer, &format!("info string {outcome}"))?;
                                    }
                                    emit(&writer, &info(&result))?;
                                    emit(
                                        &writer,
                                        &format!(
                                            "bestmove {}",
                                            result
                                                .best_move
                                                .map(|m| m.to_string())
                                                .unwrap_or_else(|| "0000".into())
                                        ),
                                    )
                                })?;
                            active = Some(ActiveSearch {
                                stop,
                                handle,
                                unbounded,
                            });
                        }
                        Err(e) => {
                            finish(&mut active, true)?;
                            emit(&output, &format!("info string error: {e}\nbestmove 0000"))?;
                        }
                    },
                    Some("stop") => finish(&mut active, true)?,
                    Some("quit") => {
                        finish(&mut active, true)?;
                        break;
                    }
                    Some("d") => emit(&output, &format!("info string fen {}", board.to_fen()))?,
                    Some("debug") => {}
                    Some(other) => emit(
                        &output,
                        &format!("info string unsupported command: {other}"),
                    )?,
                    None => {}
                }
            }
            Ok(())
        })();
        // Finite piped requests finish on EOF; an infinite request is cancelled.
        let cancel = result.is_err() || active.as_ref().is_some_and(|a| a.unbounded);
        let cleanup = finish(&mut active, cancel);
        result.and(cleanup)
    })
}
