use crustacean::board::{perft, Board};
use crustacean::search::{search, Config, SearchResult};
use std::io::{self, Write};
use std::time::Duration;

fn report(result: &SearchResult) {
    println!(
        "best={} score={}cp depth={} nodes={} budget_exhausted={}",
        result
            .best_move
            .map(|m| m.to_string())
            .unwrap_or_else(|| "none".into()),
        result.score,
        result.completed_depth,
        result.nodes,
        result.budget_exhausted
    );
    println!(
        "time={}ms qnodes={} seldepth={} pv={}",
        result.elapsed.as_millis(),
        result.qnodes,
        result.selective_depth,
        result
            .pv
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join(" ")
    );
    if let Some(reason) = result.stop_reason {
        println!("search stopped: {reason:?}");
    }
    if let Some(outcome) = result.outcome {
        println!("{outcome}");
    }
}
fn engine_turn(board: &mut Board, positions: &mut Vec<Board>) {
    let history: Vec<_> = positions.iter().map(Board::key).collect();
    let result = search(
        board,
        Config {
            depth: Some(crustacean::search::MAX_DEPTH),
            node_limit: u64::MAX,
            time_limit: Some(Duration::from_secs(1)),
        },
        &history,
    );
    report(&result);
    if let Some(m) = result.best_move {
        *board = board.after(m);
        positions.push(board.clone());
        println!("{board}");
        let history: Vec<_> = positions.iter().map(Board::key).collect();
        if let Some(outcome) = board.outcome(&history) {
            println!("{outcome}");
        }
    }
}
fn play() -> Result<(), String> {
    let mut board = Board::default();
    let mut positions = vec![board.clone()];
    println!("Crustacean: you play White. Enter e2e4, a7a8q, moves, fen, undo, new, go, help, quit.\n{board}");
    loop {
        print!("> ");
        io::stdout().flush().map_err(|e| e.to_string())?;
        let mut line = String::new();
        if io::stdin()
            .read_line(&mut line)
            .map_err(|e| e.to_string())?
            == 0
        {
            break;
        }
        let line = line.trim();
        match line {
            "quit" | "exit" => break,
            "" => {},
            "help" => println!("Coordinate moves only; promotion suffix q/r/b/n required. 'go' asks the engine to move for the current side. 'undo' removes one ply; 'new' resets. Draw claims are automatic."),
            "fen" => println!("{}", board.to_fen()),
            "moves" => println!("{}", board.legal_moves().iter().map(ToString::to_string).collect::<Vec<_>>().join(" ")),
            "new" => { board = Board::default(); positions = vec![board.clone()]; println!("{board}"); },
            "undo" => { if positions.len() > 1 { positions.pop(); board = positions.last().unwrap().clone(); } println!("{board}"); },
            "go" => engine_turn(&mut board, &mut positions),
            _ => {
                let history: Vec<_> = positions.iter().map(Board::key).collect();
                if let Some(outcome) = board.outcome(&history) { println!("{outcome}; enter new or undo"); continue; }
                match board.play_uci(line) {
                    Ok(_) => { positions.push(board.clone()); println!("{board}"); engine_turn(&mut board, &mut positions); },
                    Err(e) => println!("{e}"),
                }
            }
        }
    }
    Ok(())
}
fn run() -> Result<(), String> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str).unwrap_or("uci") {
        "uci" if args.len() <= 1 => {
            crustacean::uci::serve(io::stdin().lock(), io::stdout()).map_err(|e| e.to_string())
        }
        "play" if args.len() == 1 => play(),
        "gui" => {
            let port = match args.as_slice() {
                [_] => 8787,
                [_, flag, value] if flag == "--port" => value
                    .parse::<u16>()
                    .map_err(|_| "GUI port must be 0..65535")?,
                _ => return Err("usage: crustacean gui [--port PORT]".into()),
            };
            crustacean::gui::serve(port).map_err(|e| e.to_string())
        }
        "perft" if (2..=3).contains(&args.len()) => {
            let depth: u8 = args[1]
                .parse()
                .map_err(|_| "perft requires a depth from 0 through 5")?;
            if depth > 5 {
                return Err("perft depth is capped at 5; it has no node budget".into());
            }
            let board = if args.len() == 3 {
                Board::from_fen(&args[2])?
            } else {
                Board::default()
            };
            println!("{}", perft(&board, depth));
            Ok(())
        }
        "analyze" => {
            let mut board = Board::default();
            let mut go = Vec::new();
            let mut i = 1;
            while i < args.len() {
                let value = args.get(i + 1).ok_or("analyze option needs a value")?;
                match args[i].as_str() {
                    "--fen" => board = Board::from_fen(value)?,
                    "--depth" => {
                        go.push("depth");
                        go.push(value.as_str());
                    }
                    "--nodes" => {
                        go.push("nodes");
                        go.push(value.as_str());
                    }
                    "--movetime" => {
                        go.push("movetime");
                        go.push(value.as_str());
                    }
                    other => return Err(format!("unknown analyze option: {other}")),
                }
                i += 2;
            }
            let config = crustacean::uci::parse_go(&go)?;
            println!("{board}");
            report(&search(&board, config, &[]));
            Ok(())
        }
        "--help" | "-h" | "help" => {
            println!("Crustacean: Rust chess engine\n\n  crustacean gui [--port PORT]\n  crustacean play\n  crustacean analyze [--depth 1..64] [--nodes N] [--movetime MS] [--fen \"FEN\"]\n  crustacean perft 0..5 [\"FEN\"]\n  crustacean uci   (also the default with no arguments)\n\nDefault analysis: depth 6, node budget 2000000. Terminal play: one second per move.\nUse --depth for fixed-depth analysis. Use --movetime for timed analysis.\nSee README.md for usage and protocol support.");
            Ok(())
        }
        _ => Err("invalid command or arguments; run crustacean --help".into()),
    }
}
fn main() {
    if let Err(e) = run() {
        eprintln!("error: {e}");
        std::process::exit(2);
    }
}
